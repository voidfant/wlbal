mod cli;
mod cli_client;
mod config;
mod enforcer;
mod telegram;
mod timer;
mod tray;

use config::{Config, StatusLogEntry};
use serde::Serialize;
use std::{
    env,
    path::Path,
    process::Command,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::State;
use telegram::{TelegramBridge, TelegramChatSummary, TelegramConnectionStatus, TelegramDownloadedFile, TelegramMessage, TelegramTopicSummary};
use timer::{Phase, TimerHandle, TimerSnapshot};
use tokio::sync::RwLock;

#[derive(Clone)]
struct AppState {
    config: Arc<RwLock<Config>>,
    timer: TimerHandle,
    enforcement_enabled: Arc<RwLock<bool>>,
    telegram: TelegramBridge,
}

#[derive(Debug, Clone, Serialize)]
struct TelegramClearDownloadsResult {
    removed_files: u64,
    removed_bytes: u64,
}

#[tauri::command]
async fn get_state(state: State<'_, AppState>) -> Result<TimerSnapshot, String> {
    Ok(state.timer.snapshot().await)
}

#[tauri::command]
async fn get_config(state: State<'_, AppState>) -> Result<Config, String> {
    Ok(state.config.read().await.clone())
}

#[tauri::command]
fn get_status_events(start: String, end: String) -> Vec<StatusLogEntry> {
    config::read_status_log_window(&start, &end)
}

#[tauri::command]
async fn save_config(config: Config, app: tauri::AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let previous = state.config.read().await.clone();
    let schedule_changed = previous.schedule != config.schedule;
    let should_stop_telegram = !(config.telegram.enabled && config.telegram.block_official_clients_during_work);
    config::save_config_to_disk(&config).map_err(|err| err.to_string())?;
    *state.config.write().await = config;
    let current = state.config.read().await.clone();
    if schedule_changed {
        if *state.enforcement_enabled.read().await {
            state.timer.restart_current_phase(&current).await;
        } else {
            state.timer.stop_at_work(&current).await;
        }
    }
    let armed = *state.enforcement_enabled.read().await;
    if armed {
        enforcer::apply_website_rules_now(&current, &state.timer, true, true).await?;
    }
    if should_stop_telegram {
        state.telegram.stop(&current, &app);
    }
    Ok(())
}

#[tauri::command]
async fn get_enforcement_enabled(state: State<'_, AppState>) -> Result<bool, String> {
    Ok(*state.enforcement_enabled.read().await)
}

#[tauri::command]
async fn set_enforcement_enabled(enabled: bool, state: State<'_, AppState>) -> Result<bool, String> {
    *state.enforcement_enabled.write().await = enabled;
    let current = state.config.read().await.clone();
    if enabled {
        state.timer.start_work(&current).await;
    } else {
        state.timer.stop_at_work(&current).await;
    }
    enforcer::apply_website_rules_now(&current, &state.timer, enabled, true).await?;
    config::append_log(
        "admin_override",
        if enabled {
            "Enforcement armed from GUI"
        } else {
            "Enforcement disarmed from GUI"
        },
    );
    Ok(enabled)
}

#[tauri::command]
fn get_installed_apps() -> Vec<enforcer::AppInfo> {
    enforcer::get_installed_apps()
}

#[tauri::command]
async fn get_running_phase(state: State<'_, AppState>) -> Result<Phase, String> {
    Ok(state.timer.snapshot().await.phase)
}

#[tauri::command]
async fn get_session_count(state: State<'_, AppState>) -> Result<u32, String> {
    Ok(state.timer.snapshot().await.session_count)
}

#[tauri::command]
fn check_permissions() -> enforcer::PermissionCheck {
    enforcer::check_permissions()
}

#[tauri::command]
fn install_cli_binary() -> Result<String, String> {
    enforcer::install_cli_binary()
}

#[tauri::command]
async fn get_telegram_status(state: State<'_, AppState>) -> Result<TelegramConnectionStatus, String> {
    let config = state.config.read().await.clone();
    Ok(state.telegram.status(&config))
}

#[tauri::command]
async fn start_telegram_bridge(
    api_id: Option<i32>,
    api_hash: Option<String>,
    tdjson_path: Option<String>,
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> Result<TelegramConnectionStatus, String> {
    let mut config = state.config.read().await.clone();
    if api_id.is_some() || api_hash.is_some() || tdjson_path.is_some() {
        config.telegram.api_id = api_id.or(config.telegram.api_id);
        if let Some(api_hash) = api_hash {
            config.telegram.api_hash = api_hash;
        }
        if let Some(tdjson_path) = tdjson_path {
            config.telegram.tdjson_path = tdjson_path;
        }
        config::save_config_to_disk(&config).map_err(|err| err.to_string())?;
        *state.config.write().await = config.clone();
    }
    state.telegram.start(config.clone(), app)?;
    Ok(state.telegram.status(&config))
}

#[tauri::command]
async fn stop_telegram_bridge(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<TelegramConnectionStatus, String> {
    let config = state.config.read().await.clone();
    state.telegram.stop(&config, &app);
    Ok(state.telegram.status(&config))
}

#[tauri::command]
async fn telegram_set_phone_number(phone_number: String, state: State<'_, AppState>) -> Result<(), String> {
    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.set_phone_number(phone_number))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn telegram_check_code(code: String, state: State<'_, AppState>) -> Result<(), String> {
    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.check_code(code))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn telegram_check_password(password: String, state: State<'_, AppState>) -> Result<(), String> {
    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.check_password(password))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn get_telegram_chats(state: State<'_, AppState>) -> Result<Vec<TelegramChatSummary>, String> {
    let config = state.config.read().await.clone();
    let status = state.telegram.status(&config);
    let selected_ids = config
        .telegram
        .work_allowed_chats
        .iter()
        .map(|chat| chat.id.clone())
        .collect::<Vec<_>>();

    if !status.connected {
        return Ok(config
            .telegram
            .work_allowed_chats
            .into_iter()
            .map(|chat| TelegramChatSummary {
                id: chat.id,
                title: chat.title,
                selected: true,
                unread_count: 0,
                is_forum: false,
            })
            .collect());
    }

    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.get_chats(&selected_ids, 100))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn get_telegram_topics(chat_id: String, state: State<'_, AppState>) -> Result<Vec<TelegramTopicSummary>, String> {
    let config = state.config.read().await.clone();
    let selected_rules = config
        .telegram
        .work_allowed_chats
        .iter()
        .map(|chat| (chat.id.clone(), chat.topic_id))
        .collect::<Vec<_>>();
    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.get_topics(chat_id, &selected_rules))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn get_telegram_messages(chat_id: String, topic_id: Option<i64>, from_message_id: Option<i64>, state: State<'_, AppState>) -> Result<Vec<TelegramMessage>, String> {
    let allowed = state
        .config
        .read()
        .await
        .telegram
        .work_allowed_chats
        .iter()
        .any(|chat| chat.id == chat_id && (chat.topic_id.is_none() || chat.topic_id == topic_id));
    if !allowed {
        return Err("This chat is not allowed in wlbal".into());
    }

    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.get_messages(chat_id, topic_id, from_message_id.unwrap_or(0), 50))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn search_telegram_messages(chat_id: String, topic_id: Option<i64>, query: String, from_message_id: Option<i64>, state: State<'_, AppState>) -> Result<Vec<TelegramMessage>, String> {
    let allowed = state
        .config
        .read()
        .await
        .telegram
        .work_allowed_chats
        .iter()
        .any(|chat| chat.id == chat_id && (chat.topic_id.is_none() || chat.topic_id == topic_id));
    if !allowed {
        return Err("This chat is not allowed in wlbal".into());
    }

    let query = query.trim().to_string();
    if query.is_empty() {
        return Ok(Vec::new());
    }

    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.search_messages(chat_id, topic_id, query, from_message_id.unwrap_or(0), 30))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn download_telegram_media(file_id: i64, state: State<'_, AppState>) -> Result<TelegramDownloadedFile, String> {
    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.download_file(file_id))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn clear_telegram_downloads(app: tauri::AppHandle, state: State<'_, AppState>) -> Result<TelegramClearDownloadsResult, String> {
    let config = state.config.read().await.clone();
    if state.telegram.status(&config).bridge_running {
        state.telegram.stop(&config, &app);
    }

    tokio::task::spawn_blocking(move || {
        let files_dir = config::config_dir().join("telegram").join("files");
        let (removed_files, removed_bytes) = dir_stats(&files_dir);
        if files_dir.exists() {
            std::fs::remove_dir_all(&files_dir)
                .map_err(|err| format!("Failed to clear Telegram downloads: {err}"))?;
        }
        std::fs::create_dir_all(&files_dir)
            .map_err(|err| format!("Failed to recreate Telegram downloads directory: {err}"))?;
        Ok(TelegramClearDownloadsResult {
            removed_files,
            removed_bytes,
        })
    })
    .await
    .map_err(|err| err.to_string())?
}

#[tauri::command]
fn open_telegram_media(path: String) -> Result<(), String> {
    let requested = Path::new(&path);
    let canonical = requested
        .canonicalize()
        .map_err(|err| format!("Media file is not available locally: {err}"))?;
    let media_root = config::config_dir()
        .join("telegram")
        .join("files")
        .canonicalize()
        .map_err(|err| format!("Telegram files directory is not available: {err}"))?;
    if !canonical.starts_with(media_root) {
        return Err("Refusing to open a file outside the Telegram files directory".into());
    }

    #[cfg(target_os = "macos")]
    let mut command = {
        let mut command = Command::new("open");
        command.arg(&canonical);
        command
    };

    #[cfg(target_os = "windows")]
    let mut command = {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", &canonical.to_string_lossy()]);
        command
    };

    #[cfg(all(unix, not(target_os = "macos")))]
    let mut command = {
        let mut command = Command::new("xdg-open");
        command.arg(&canonical);
        command
    };

    command
        .spawn()
        .map_err(|err| format!("Failed to open media file: {err}"))?;
    Ok(())
}

#[tauri::command]
fn stage_telegram_media_bytes(file_name: Option<String>, bytes: Vec<u8>) -> Result<String, String> {
    if bytes.is_empty() {
        return Err("Clipboard media is empty".into());
    }
    let upload_dir = config::config_dir().join("telegram").join("uploads");
    std::fs::create_dir_all(&upload_dir)
        .map_err(|err| format!("Failed to prepare Telegram uploads: {err}"))?;
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| err.to_string())?
        .as_millis();
    let name = sanitize_upload_name(file_name.as_deref().unwrap_or("pasted-media"));
    let path = upload_dir.join(format!("{timestamp}-{name}"));
    std::fs::write(&path, bytes)
        .map_err(|err| format!("Failed to stage Telegram media: {err}"))?;
    Ok(path.to_string_lossy().to_string())
}

fn sanitize_upload_name(name: &str) -> String {
    let sanitized = name
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_') {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string();
    if sanitized.is_empty() {
        "pasted-media".into()
    } else {
        sanitized
    }
}

fn dir_stats(path: &Path) -> (u64, u64) {
    let Ok(entries) = std::fs::read_dir(path) else {
        return (0, 0);
    };
    let mut files = 0;
    let mut bytes = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        if meta.is_dir() {
            let (nested_files, nested_bytes) = dir_stats(&path);
            files += nested_files;
            bytes += nested_bytes;
        } else {
            files += 1;
            bytes += meta.len();
        }
    }
    (files, bytes)
}

#[tauri::command]
async fn send_telegram_message(chat_id: String, topic_id: Option<i64>, reply_to_message_id: Option<String>, text: String, state: State<'_, AppState>) -> Result<TelegramMessage, String> {
    let allowed = state
        .config
        .read()
        .await
        .telegram
        .work_allowed_chats
        .iter()
        .any(|chat| chat.id == chat_id && (chat.topic_id.is_none() || chat.topic_id == topic_id));
    if !allowed {
        return Err("This chat is not allowed in wlbal".into());
    }

    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.send_message(chat_id, topic_id, reply_to_message_id, text))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn send_telegram_media(chat_id: String, topic_id: Option<i64>, reply_to_message_id: Option<String>, file_paths: Vec<String>, caption: String, state: State<'_, AppState>) -> Result<Vec<TelegramMessage>, String> {
    let allowed = state
        .config
        .read()
        .await
        .telegram
        .work_allowed_chats
        .iter()
        .any(|chat| chat.id == chat_id && (chat.topic_id.is_none() || chat.topic_id == topic_id));
    if !allowed {
        return Err("This chat is not allowed in wlbal".into());
    }

    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.send_media(chat_id, topic_id, reply_to_message_id, file_paths, caption))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn forward_telegram_message(chat_id: String, topic_id: Option<i64>, from_chat_id: String, message_id: String, state: State<'_, AppState>) -> Result<Vec<TelegramMessage>, String> {
    let allowed = state
        .config
        .read()
        .await
        .telegram
        .work_allowed_chats
        .iter()
        .any(|chat| chat.id == chat_id && (chat.topic_id.is_none() || chat.topic_id == topic_id));
    if !allowed {
        return Err("This chat is not allowed in wlbal".into());
    }

    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.forward_message(chat_id, topic_id, from_chat_id, message_id))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn react_to_telegram_message(chat_id: String, message_id: String, emoji: String, state: State<'_, AppState>) -> Result<(), String> {
    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.add_reaction(chat_id, message_id, emoji))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn mark_telegram_messages_read(chat_id: String, message_ids: Vec<String>, state: State<'_, AppState>) -> Result<(), String> {
    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.view_messages(chat_id, message_ids))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn run_get_to_work_script(state: State<'_, AppState>) -> Result<String, String> {
    let script = state
        .config
        .read()
        .await
        .actions
        .get_to_work_script
        .trim()
        .to_string();

    if script.is_empty() {
        return Err("No get-to-work script configured".into());
    }

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let output = tokio::task::spawn_blocking(move || {
        Command::new(shell)
            .args(["-lc", &script])
            .output()
            .map_err(|err| err.to_string())
    })
    .await
    .map_err(|err| err.to_string())??;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        config::append_log("get_to_work", "Get-to-work script executed successfully");
        Ok(if stdout.is_empty() { "Script executed".into() } else { stdout })
    } else {
        let message = if stderr.is_empty() {
            format!("Script exited with {}", output.status)
        } else {
            stderr
        };
        config::append_log("get_to_work_error", message.clone());
        Err(message)
    }
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let initial_config = config::load_or_create_config().unwrap_or_else(|err| {
        config::append_log("config_error", format!("Falling back to defaults: {err}"));
        Config::default()
    });
    let config = Arc::new(RwLock::new(initial_config.clone()));
    let timer = TimerHandle::new(config.clone(), &initial_config);
    let enforcement_enabled = Arc::new(RwLock::new(false));
    let telegram = TelegramBridge::new();
    let state = AppState {
        config: config.clone(),
        timer: timer.clone(),
        enforcement_enabled: enforcement_enabled.clone(),
        telegram,
    };

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .manage(state)
        .setup(move |app| {
            let handle = app.handle().clone();
            timer::spawn_timer(timer.clone(), handle.clone());
            cli::spawn_cli_server(timer.clone());
            enforcer::spawn_enforcer(
                config.clone(),
                timer.clone(),
                enforcement_enabled.clone(),
                handle.clone(),
            );
            config::spawn_config_watcher(config.clone(), handle.clone());
            tray::setup_tray(&handle, timer.clone())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_state,
            get_config,
            get_status_events,
            save_config,
            get_enforcement_enabled,
            set_enforcement_enabled,
            get_installed_apps,
            get_running_phase,
            get_session_count,
            check_permissions,
            install_cli_binary,
            get_telegram_status,
            start_telegram_bridge,
            stop_telegram_bridge,
            telegram_set_phone_number,
            telegram_check_code,
            telegram_check_password,
            get_telegram_chats,
            get_telegram_topics,
            get_telegram_messages,
            search_telegram_messages,
            download_telegram_media,
            clear_telegram_downloads,
            open_telegram_media,
            stage_telegram_media_bytes,
            send_telegram_message,
            send_telegram_media,
            forward_telegram_message,
            react_to_telegram_message,
            mark_telegram_messages_read,
            run_get_to_work_script
        ])
        .run(tauri::generate_context!())
        .expect("error while running wlbal");
}

pub fn run_cli_client() {
    cli_client::run();
}
