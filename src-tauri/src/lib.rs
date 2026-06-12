mod cli;
mod cli_client;
mod config;
mod enforcer;
mod telegram;
mod timer;
mod tray;

use config::Config;
use std::{env, process::Command, sync::Arc};
use tauri::State;
use telegram::{TelegramBridge, TelegramChatSummary, TelegramConnectionStatus, TelegramMessage};
use timer::{Phase, TimerHandle, TimerSnapshot};
use tokio::sync::RwLock;

#[derive(Clone)]
struct AppState {
    config: Arc<RwLock<Config>>,
    timer: TimerHandle,
    enforcement_enabled: Arc<RwLock<bool>>,
    telegram: TelegramBridge,
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
async fn save_config(config: Config, state: State<'_, AppState>) -> Result<(), String> {
    let previous = state.config.read().await.clone();
    let schedule_changed = previous.schedule != config.schedule;
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
            })
            .collect());
    }

    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.get_chats(&selected_ids, 100))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn get_telegram_messages(chat_id: String, from_message_id: Option<i64>, state: State<'_, AppState>) -> Result<Vec<TelegramMessage>, String> {
    let allowed = state
        .config
        .read()
        .await
        .telegram
        .work_allowed_chats
        .iter()
        .any(|chat| chat.id == chat_id);
    if !allowed {
        return Err("This chat is not allowed in wlbal".into());
    }

    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.get_messages(chat_id, from_message_id.unwrap_or(0), 50))
        .await
        .map_err(|err| err.to_string())?
}

#[tauri::command]
async fn send_telegram_message(chat_id: String, text: String, state: State<'_, AppState>) -> Result<TelegramMessage, String> {
    let allowed = state
        .config
        .read()
        .await
        .telegram
        .work_allowed_chats
        .iter()
        .any(|chat| chat.id == chat_id);
    if !allowed {
        return Err("This chat is not allowed in wlbal".into());
    }

    let bridge = state.telegram.clone();
    tokio::task::spawn_blocking(move || bridge.send_message(chat_id, text))
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
            telegram_set_phone_number,
            telegram_check_code,
            telegram_check_password,
            get_telegram_chats,
            get_telegram_messages,
            send_telegram_message,
            run_get_to_work_script
        ])
        .run(tauri::generate_context!())
        .expect("error while running wlbal");
}

pub fn run_cli_client() {
    cli_client::run();
}
