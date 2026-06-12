use crate::config::{config_dir, Config};
use libloading::Library;
use serde::Serialize;
use serde_json::{json, Value};
use std::{
    collections::HashMap,
    env,
    ffi::{c_char, c_double, c_int, CStr, CString},
    fs,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex},
    thread,
    time::Duration,
};
use tauri::{AppHandle, Emitter};

type TdCreateClientId = unsafe extern "C" fn() -> c_int;
type TdSend = unsafe extern "C" fn(c_int, *const c_char);
type TdReceive = unsafe extern "C" fn(c_double) -> *const c_char;
type TdExecute = unsafe extern "C" fn(*const c_char) -> *const c_char;

#[derive(Debug, Clone, Serialize)]
pub struct TelegramConnectionStatus {
    pub enabled: bool,
    pub connected: bool,
    pub configured: bool,
    pub bridge_running: bool,
    pub auth_state: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelegramChatSummary {
    pub id: String,
    pub title: String,
    pub selected: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelegramMessage {
    pub id: String,
    pub chat_id: String,
    pub sender: String,
    pub date: i64,
    pub outgoing: bool,
    pub text: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct TelegramUpdatePayload {
    pub status: TelegramConnectionStatus,
}

#[derive(Debug, Clone, Default)]
struct BridgeSnapshot {
    running: bool,
    connected: bool,
    auth_state: String,
    message: String,
}

#[derive(Clone)]
pub struct TelegramBridge {
    command_tx: Arc<Mutex<Option<mpsc::Sender<BridgeCommand>>>>,
    snapshot: Arc<Mutex<BridgeSnapshot>>,
}

enum BridgeCommand {
    Request(Value, mpsc::Sender<Result<Value, String>>),
}

struct TdJson {
    _library: Library,
    create_client_id: TdCreateClientId,
    send: TdSend,
    receive: TdReceive,
    execute: TdExecute,
}

impl TelegramBridge {
    pub fn new() -> Self {
        Self {
            command_tx: Arc::new(Mutex::new(None)),
            snapshot: Arc::new(Mutex::new(BridgeSnapshot {
                auth_state: "idle".into(),
                message: "Telegram bridge is stopped.".into(),
                ..BridgeSnapshot::default()
            })),
        }
    }

    pub fn status(&self, config: &Config) -> TelegramConnectionStatus {
        let snapshot = self.snapshot.lock().unwrap_or_else(|err| err.into_inner()).clone();
        TelegramConnectionStatus {
            enabled: config.telegram.enabled,
            connected: snapshot.connected,
            configured: config.telegram.api_id.is_some() && !config.telegram.api_hash.trim().is_empty(),
            bridge_running: snapshot.running,
            auth_state: snapshot.auth_state,
            message: snapshot.message,
        }
    }

    pub fn start(&self, config: Config, app: AppHandle) -> Result<(), String> {
        if self
            .command_tx
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .is_some()
        {
            return Ok(());
        }

        let api_id = config
            .telegram
            .api_id
            .ok_or_else(|| "Telegram API ID is required. Get it from https://my.telegram.org.".to_string())?;
        let api_hash = config.telegram.api_hash.trim().to_string();
        if api_hash.is_empty() {
            return Err("Telegram API hash is required. Get it from https://my.telegram.org.".into());
        }

        let (command_tx, command_rx) = mpsc::channel();
        *self.command_tx.lock().unwrap_or_else(|err| err.into_inner()) = Some(command_tx);
        set_snapshot(
            &self.snapshot,
            BridgeSnapshot {
                running: true,
                connected: false,
                auth_state: "starting".into(),
                message: "Starting Telegram bridge...".into(),
            },
            &config,
            &app,
        );

        let snapshot = self.snapshot.clone();
        let command_tx_slot = self.command_tx.clone();
        thread::spawn(move || {
            if let Err(err) = run_worker(command_rx, snapshot.clone(), config.clone(), api_id, api_hash, app.clone()) {
                set_snapshot(
                    &snapshot,
                    BridgeSnapshot {
                        running: false,
                        connected: false,
                        auth_state: "error".into(),
                        message: err,
                    },
                    &config,
                    &app,
                );
            }
            *command_tx_slot.lock().unwrap_or_else(|err| err.into_inner()) = None;
        });

        Ok(())
    }

    pub fn set_phone_number(&self, phone_number: String) -> Result<(), String> {
        self.request(json!({
            "@type": "setAuthenticationPhoneNumber",
            "phone_number": phone_number,
        }))
        .map(|_| ())
    }

    pub fn check_code(&self, code: String) -> Result<(), String> {
        self.request(json!({
            "@type": "checkAuthenticationCode",
            "code": code,
        }))
        .map(|_| ())
    }

    pub fn check_password(&self, password: String) -> Result<(), String> {
        self.request(json!({
            "@type": "checkAuthenticationPassword",
            "password": password,
        }))
        .map(|_| ())
    }

    pub fn get_chats(&self, selected_ids: &[String], limit: i32) -> Result<Vec<TelegramChatSummary>, String> {
        let selected: std::collections::HashSet<&str> = selected_ids.iter().map(String::as_str).collect();
        let response = self.request(json!({
            "@type": "getChats",
            "chat_list": { "@type": "chatListMain" },
            "limit": limit.clamp(1, 200),
        }))?;
        let ids = response
            .get("chat_ids")
            .and_then(Value::as_array)
            .ok_or_else(|| response_error(&response, "Telegram did not return a chat list"))?;

        let mut chats = Vec::new();
        for id in ids {
            let Some(chat_id) = json_id(id) else {
                continue;
            };
            let chat = self.request(json!({
                "@type": "getChat",
                "chat_id": chat_id,
            }))?;
            let title = chat
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("Untitled chat")
                .to_string();
            chats.push(TelegramChatSummary {
                selected: selected.contains(chat_id.as_str()),
                id: chat_id,
                title,
            });
        }

        Ok(chats)
    }

    pub fn get_messages(&self, chat_id: String, from_message_id: i64, limit: i32) -> Result<Vec<TelegramMessage>, String> {
        let response = self.request(json!({
            "@type": "getChatHistory",
            "chat_id": chat_id,
            "from_message_id": from_message_id,
            "offset": 0,
            "limit": limit.clamp(1, 100),
            "only_local": false,
        }))?;
        let messages = response
            .get("messages")
            .and_then(Value::as_array)
            .ok_or_else(|| response_error(&response, "Telegram did not return message history"))?;

        Ok(messages.iter().filter_map(parse_message).collect())
    }

    pub fn send_message(&self, chat_id: String, text: String) -> Result<TelegramMessage, String> {
        let response = self.request(json!({
            "@type": "sendMessage",
            "chat_id": chat_id,
            "input_message_content": {
                "@type": "inputMessageText",
                "text": {
                    "@type": "formattedText",
                    "text": text,
                    "entities": [],
                },
                "clear_draft": true,
            },
        }))?;
        parse_message(&response).ok_or_else(|| response_error(&response, "Telegram did not return the sent message"))
    }

    fn request(&self, query: Value) -> Result<Value, String> {
        let tx = self
            .command_tx
            .lock()
            .unwrap_or_else(|err| err.into_inner())
            .clone()
            .ok_or_else(|| "Telegram bridge is not running".to_string())?;
        let (reply_tx, reply_rx) = mpsc::channel();
        tx.send(BridgeCommand::Request(query, reply_tx))
            .map_err(|_| "Telegram bridge worker stopped".to_string())?;
        reply_rx
            .recv_timeout(Duration::from_secs(20))
            .map_err(|_| "Timed out waiting for Telegram".to_string())?
    }
}

fn run_worker(
    command_rx: mpsc::Receiver<BridgeCommand>,
    snapshot: Arc<Mutex<BridgeSnapshot>>,
    config: Config,
    api_id: i32,
    api_hash: String,
    app: AppHandle,
) -> Result<(), String> {
    let td = unsafe { TdJson::load(config.telegram.tdjson_path.trim())? };
    unsafe {
        let verbosity = CString::new(json!({ "@type": "setLogVerbosityLevel", "new_verbosity_level": 1 }).to_string())
            .map_err(|err| err.to_string())?;
        (td.execute)(verbosity.as_ptr());
    }

    let client_id = unsafe { (td.create_client_id)() };
    let mut pending: HashMap<String, mpsc::Sender<Result<Value, String>>> = HashMap::new();
    let mut request_id = 0u64;

    send_raw(&td, client_id, json!({ "@type": "getAuthorizationState" }))?;

    loop {
        while let Ok(command) = command_rx.try_recv() {
            match command {
                BridgeCommand::Request(mut query, reply_tx) => {
                    request_id += 1;
                    let extra = format!("wlbal-{request_id}");
                    if let Some(object) = query.as_object_mut() {
                        object.insert("@extra".into(), Value::String(extra.clone()));
                    }
                    pending.insert(extra, reply_tx);
                    send_raw(&td, client_id, query)?;
                }
            }
        }

        let received = unsafe { (td.receive)(0.1) };
        if received.is_null() {
            continue;
        }

        let raw = unsafe { CStr::from_ptr(received) }
            .to_string_lossy()
            .to_string();
        let value: Value = match serde_json::from_str(&raw) {
            Ok(value) => value,
            Err(err) => {
                set_snapshot(
                    &snapshot,
                    BridgeSnapshot {
                        running: true,
                        connected: false,
                        auth_state: "error".into(),
                        message: format!("Failed to parse Telegram update: {err}"),
                    },
                    &config,
                    &app,
                );
                continue;
            }
        };

        if let Some(extra) = value.get("@extra").and_then(Value::as_str) {
            if extra == "wlbal-set-tdlib-parameters" {
                if value.get("@type").and_then(Value::as_str) == Some("error") {
                    let error = response_error(&value, "unknown error");
                    if error.contains("Parameters aren't specified") {
                        let data_dir = telegram_data_dir();
                        let database_dir = data_dir.join("database");
                        let files_dir = data_dir.join("files");
                        send_raw(
                            &td,
                            client_id,
                            json!({
                                "@type": "setTdlibParameters",
                                "@extra": "wlbal-set-tdlib-parameters-nested",
                                "parameters": tdlib_parameters(api_id, &api_hash, &database_dir, &files_dir),
                            }),
                        )?;
                        set_snapshot(
                            &snapshot,
                            BridgeSnapshot {
                                running: true,
                                connected: false,
                                auth_state: "authorizationStateWaitTdlibParameters".into(),
                                message: "Retrying TDLib initialization with legacy parameter format...".into(),
                            },
                            &config,
                            &app,
                        );
                        continue;
                    }
                    set_snapshot(
                        &snapshot,
                        BridgeSnapshot {
                            running: true,
                            connected: false,
                            auth_state: "authorizationStateWaitTdlibParameters".into(),
                            message: format!("TDLib parameters rejected: {error}"),
                        },
                        &config,
                        &app,
                    );
                } else if value.get("@type").and_then(Value::as_str) == Some("ok") {
                    send_raw(&td, client_id, json!({ "@type": "getAuthorizationState" }))?;
                    set_snapshot(
                        &snapshot,
                        BridgeSnapshot {
                            running: true,
                            connected: false,
                            auth_state: "authorizationStateWaitTdlibParameters".into(),
                            message: "TDLib parameters accepted; opening local database...".into(),
                        },
                        &config,
                        &app,
                    );
                }
                continue;
            }
            if extra == "wlbal-set-tdlib-parameters-nested" {
                if value.get("@type").and_then(Value::as_str) == Some("error") {
                    set_snapshot(
                        &snapshot,
                        BridgeSnapshot {
                            running: true,
                            connected: false,
                            auth_state: "authorizationStateWaitTdlibParameters".into(),
                            message: format!("TDLib parameters rejected: {}", response_error(&value, "unknown error")),
                        },
                        &config,
                        &app,
                    );
                } else if value.get("@type").and_then(Value::as_str) == Some("ok") {
                    send_raw(&td, client_id, json!({ "@type": "getAuthorizationState" }))?;
                    set_snapshot(
                        &snapshot,
                        BridgeSnapshot {
                            running: true,
                            connected: false,
                            auth_state: "authorizationStateWaitTdlibParameters".into(),
                            message: "TDLib parameters accepted; opening local database...".into(),
                        },
                        &config,
                        &app,
                    );
                }
                continue;
            }
            if let Some(reply_tx) = pending.remove(extra) {
                if value.get("@type").and_then(Value::as_str) == Some("error") {
                    let _ = reply_tx.send(Err(response_error(&value, "Telegram request failed")));
                } else {
                    let _ = reply_tx.send(Ok(value.clone()));
                }
            }
        }

        handle_update(&td, client_id, &snapshot, &config, api_id, &api_hash, &app, &value)?;
    }
}

fn handle_update(
    td: &TdJson,
    client_id: c_int,
    snapshot: &Arc<Mutex<BridgeSnapshot>>,
    config: &Config,
    api_id: i32,
    api_hash: &str,
    app: &AppHandle,
    value: &Value,
) -> Result<(), String> {
    if auth_state_type(value).is_none() {
        if value.get("@type").and_then(Value::as_str) == Some("updateNewMessage") {
            if let Some(message) = value.get("message").and_then(parse_message) {
                let _ = app.emit("telegram-message", &message);
            }
        }
        return Ok(());
    }

    let auth_state = auth_state_type(value).unwrap_or("authorizationStateUnknown");

    match auth_state {
        "authorizationStateWaitTdlibParameters" => {
            let data_dir = telegram_data_dir();
            let database_dir = data_dir.join("database");
            let files_dir = data_dir.join("files");
            fs::create_dir_all(&database_dir).map_err(|err| format!("Failed to create Telegram database directory: {err}"))?;
            fs::create_dir_all(&files_dir).map_err(|err| format!("Failed to create Telegram files directory: {err}"))?;
            send_raw(
                td,
                client_id,
                json!({
                    "@type": "setTdlibParameters",
                    "@extra": "wlbal-set-tdlib-parameters",
                    "use_test_dc": false,
                    "database_directory": database_dir.to_string_lossy(),
                    "files_directory": files_dir.to_string_lossy(),
                    "database_encryption_key": "",
                    "use_file_database": true,
                    "use_chat_info_database": true,
                    "use_message_database": true,
                    "use_secret_chats": false,
                    "api_id": api_id,
                    "api_hash": api_hash,
                    "system_language_code": "en",
                    "device_model": "wlbal desktop",
                    "system_version": env::consts::OS,
                    "application_version": env!("CARGO_PKG_VERSION"),
                    "enable_storage_optimizer": true,
                    "ignore_file_names": false,
                }),
            )?;
            set_snapshot(
                snapshot,
                BridgeSnapshot {
                    running: true,
                    connected: false,
                    auth_state: auth_state.into(),
                    message: "Initializing Telegram local database...".into(),
                },
                config,
                app,
            );
        }
        "authorizationStateWaitPhoneNumber" => set_snapshot(
            snapshot,
            BridgeSnapshot {
                running: true,
                connected: false,
                auth_state: auth_state.into(),
                message: "Enter your Telegram phone number.".into(),
            },
            config,
            app,
        ),
        "authorizationStateWaitEncryptionKey" => {
            send_raw(
                td,
                client_id,
                json!({
                    "@type": "checkDatabaseEncryptionKey",
                    "encryption_key": "",
                }),
            )?;
            set_snapshot(
                snapshot,
                BridgeSnapshot {
                    running: true,
                    connected: false,
                    auth_state: auth_state.into(),
                    message: "Opening Telegram local database...".into(),
                },
                config,
                app,
            );
        }
        "authorizationStateWaitCode" => set_snapshot(
            snapshot,
            BridgeSnapshot {
                running: true,
                connected: false,
                auth_state: auth_state.into(),
                message: "Enter the Telegram login code.".into(),
            },
            config,
            app,
        ),
        "authorizationStateWaitPassword" => set_snapshot(
            snapshot,
            BridgeSnapshot {
                running: true,
                connected: false,
                auth_state: auth_state.into(),
                message: "Enter your Telegram two-step verification password.".into(),
            },
            config,
            app,
        ),
        "authorizationStateReady" => set_snapshot(
            snapshot,
            BridgeSnapshot {
                running: true,
                connected: true,
                auth_state: auth_state.into(),
                message: "Telegram bridge connected.".into(),
            },
            config,
            app,
        ),
        "authorizationStateClosed" => set_snapshot(
            snapshot,
            BridgeSnapshot {
                running: false,
                connected: false,
                auth_state: auth_state.into(),
                message: "Telegram bridge closed.".into(),
            },
            config,
            app,
        ),
        _ => set_snapshot(
            snapshot,
            BridgeSnapshot {
                running: true,
                connected: false,
                auth_state: auth_state.into(),
                message: auth_state.replace("authorizationState", "Telegram: "),
            },
            config,
            app,
        ),
    }

    Ok(())
}

unsafe impl Send for TdJson {}
unsafe impl Sync for TdJson {}

impl TdJson {
    unsafe fn load(configured_path: &str) -> Result<Self, String> {
        let mut candidates = Vec::new();
        if !configured_path.is_empty() {
            candidates.push(configured_path.to_string());
        }
        if let Ok(path) = env::var("WLBAL_TDJSON_PATH") {
            candidates.push(path);
        }
        candidates.extend([
            "/opt/homebrew/lib/libtdjson.dylib".into(),
            "/usr/local/lib/libtdjson.dylib".into(),
            "/opt/local/lib/libtdjson.dylib".into(),
            "libtdjson.dylib".into(),
            "tdjson.dylib".into(),
            "libtdjson.so".into(),
            "tdjson.dll".into(),
        ]);

        let mut errors = Vec::new();
        for candidate in candidates {
            match unsafe { Library::new(&candidate) } {
                Ok(library) => {
                    let create_client_id = *unsafe { library.get::<TdCreateClientId>(b"td_create_client_id") }
                        .map_err(|err| err.to_string())?;
                    let send = *unsafe { library.get::<TdSend>(b"td_send") }.map_err(|err| err.to_string())?;
                    let receive = *unsafe { library.get::<TdReceive>(b"td_receive") }.map_err(|err| err.to_string())?;
                    let execute = *unsafe { library.get::<TdExecute>(b"td_execute") }.map_err(|err| err.to_string())?;
                    return Ok(Self {
                        _library: library,
                        create_client_id,
                        send,
                        receive,
                        execute,
                    });
                }
                Err(err) => errors.push(format!("{candidate}: {err}")),
            }
        }

        Err(format!(
            "Could not load TDLib JSON library. Install TDLib and set WLBAL_TDJSON_PATH if needed. Tried: {}",
            errors.join("; ")
        ))
    }
}

fn send_raw(td: &TdJson, client_id: c_int, query: Value) -> Result<(), String> {
    let raw = CString::new(query.to_string()).map_err(|err| err.to_string())?;
    unsafe {
        (td.send)(client_id, raw.as_ptr());
    }
    Ok(())
}

fn set_snapshot(snapshot: &Arc<Mutex<BridgeSnapshot>>, next: BridgeSnapshot, config: &Config, app: &AppHandle) {
    *snapshot.lock().unwrap_or_else(|err| err.into_inner()) = next.clone();
    let status = TelegramConnectionStatus {
        enabled: config.telegram.enabled,
        connected: next.connected,
        configured: config.telegram.api_id.is_some() && !config.telegram.api_hash.trim().is_empty(),
        bridge_running: next.running,
        auth_state: next.auth_state,
        message: next.message,
    };
    let _ = app.emit("telegram-state-changed", TelegramUpdatePayload { status });
}

fn auth_state_type(value: &Value) -> Option<&str> {
    if value.get("@type").and_then(Value::as_str) == Some("updateAuthorizationState") {
        return value
            .get("authorization_state")
            .and_then(|state| state.get("@type"))
            .and_then(Value::as_str);
    }

    value
        .get("@type")
        .and_then(Value::as_str)
        .filter(|kind| kind.starts_with("authorizationState"))
}

fn telegram_data_dir() -> PathBuf {
    config_dir().join("telegram")
}

fn tdlib_parameters(api_id: i32, api_hash: &str, database_dir: &Path, files_dir: &Path) -> Value {
    json!({
        "@type": "tdlibParameters",
        "use_test_dc": false,
        "database_directory": database_dir.to_string_lossy(),
        "files_directory": files_dir.to_string_lossy(),
        "database_encryption_key": "",
        "use_file_database": true,
        "use_chat_info_database": true,
        "use_message_database": true,
        "use_secret_chats": false,
        "api_id": api_id,
        "api_hash": api_hash,
        "system_language_code": "en",
        "device_model": "wlbal desktop",
        "system_version": env::consts::OS,
        "application_version": env!("CARGO_PKG_VERSION"),
        "enable_storage_optimizer": true,
        "ignore_file_names": false,
    })
}

fn response_error(value: &Value, fallback: &str) -> String {
    if let Some(message) = value.get("message").and_then(Value::as_str) {
        if message.contains("UPDATE_APP_TO_LOGIN") {
            return "TDLib is too old for this Telegram login flow. Use a newer libtdjson build; phone-code login needs TDLib 1.8.11+.".into();
        }
        return message.to_string();
    }
    if let Some(code) = value.get("code").and_then(Value::as_i64) {
        return format!("{fallback} ({code})");
    }
    fallback.into()
}

fn json_id(value: &Value) -> Option<String> {
    if let Some(text) = value.as_str() {
        return Some(text.to_string());
    }
    if let Some(number) = value.as_i64() {
        return Some(number.to_string());
    }
    if let Some(number) = value.as_u64() {
        return Some(number.to_string());
    }
    None
}

fn parse_message(value: &Value) -> Option<TelegramMessage> {
    let id = value.get("id").and_then(json_id)?;
    let chat_id = value.get("chat_id").and_then(json_id)?;
    let sender = value
        .get("sender_id")
        .and_then(|sender| sender.get("@type").and_then(Value::as_str))
        .unwrap_or("sender")
        .replace("messageSender", "");
    let text = message_text(value);
    Some(TelegramMessage {
        id,
        chat_id,
        sender,
        date: value.get("date").and_then(Value::as_i64).unwrap_or_default(),
        outgoing: value.get("is_outgoing").and_then(Value::as_bool).unwrap_or(false),
        text,
    })
}

fn message_text(value: &Value) -> String {
    let Some(content) = value.get("content") else {
        return String::new();
    };
    match content.get("@type").and_then(Value::as_str).unwrap_or_default() {
        "messageText" => content
            .get("text")
            .and_then(|text| text.get("text"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        "messagePhoto" => caption_or_label(content, "Photo"),
        "messageVideo" => caption_or_label(content, "Video"),
        "messageDocument" => caption_or_label(content, "Document"),
        "messageAudio" => caption_or_label(content, "Audio"),
        "messageVoiceNote" => caption_or_label(content, "Voice note"),
        "messageSticker" => "[Sticker]".into(),
        other => format!("[{}]", other.trim_start_matches("message")),
    }
}

fn caption_or_label(content: &Value, label: &str) -> String {
    let caption = content
        .get("caption")
        .and_then(|caption| caption.get("text"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if caption.is_empty() {
        format!("[{label}]")
    } else {
        format!("[{label}] {caption}")
    }
}
