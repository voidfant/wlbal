use chrono::Utc;
use serde::{Deserialize, Serialize};
use std::{
    env,
    fs,
    io,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};
use tauri::{AppHandle, Emitter};
use tokio::{sync::RwLock, time::{interval, Duration}};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ScheduleMode {
    Pomodoro,
    Custom,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ScheduleConfig {
    pub mode: ScheduleMode,
    pub work_mins: u32,
    pub leisure_mins: u32,
    pub long_break_mins: u32,
    pub sessions_before_long_break: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AppRules {
    pub work_blocked: Vec<String>,
    pub leisure_blocked: Vec<String>,
    pub work_allowed_only: Vec<String>,
    pub leisure_allowed_only: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SiteRules {
    pub work_blocked: Vec<String>,
    pub leisure_blocked: Vec<String>,
    #[serde(default)]
    pub hosts_blocking_enabled: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TelegramChatRule {
    pub id: String,
    pub title: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct TelegramConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default)]
    pub block_official_clients_during_work: bool,
    #[serde(default)]
    pub api_id: Option<i32>,
    #[serde(default)]
    pub api_hash: String,
    #[serde(default)]
    pub work_allowed_chats: Vec<TelegramChatRule>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ResumeAfterOverride {
    Continue,
    FreshCycle,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OverrideConfig {
    pub resume_after_override: ResumeAfterOverride,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OnboardingConfig {
    pub completed: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ActionConfig {
    #[serde(default)]
    pub get_to_work_script: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    pub schedule: ScheduleConfig,
    pub apps: AppRules,
    pub sites: SiteRules,
    #[serde(default)]
    pub telegram: TelegramConfig,
    #[serde(rename = "override")]
    pub override_config: OverrideConfig,
    pub notifications: bool,
    pub log_blocked_attempts: bool,
    pub onboarding: OnboardingConfig,
    #[serde(default)]
    pub actions: ActionConfig,
}

impl Default for ActionConfig {
    fn default() -> Self {
        Self {
            get_to_work_script: String::new(),
        }
    }
}

impl Default for TelegramConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            block_official_clients_during_work: true,
            api_id: None,
            api_hash: String::new(),
            work_allowed_chats: Vec::new(),
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            schedule: ScheduleConfig {
                mode: ScheduleMode::Pomodoro,
                work_mins: 25,
                leisure_mins: 5,
                long_break_mins: 15,
                sessions_before_long_break: 4,
            },
            apps: AppRules {
                work_blocked: vec![
                    "com.apple.MobileSMS".into(),
                    "com.spotify.client".into(),
                ],
                leisure_blocked: Vec::new(),
                work_allowed_only: Vec::new(),
                leisure_allowed_only: Vec::new(),
            },
            sites: SiteRules {
                work_blocked: vec!["reddit.com".into(), "twitter.com".into(), "youtube.com".into()],
                leisure_blocked: Vec::new(),
                hosts_blocking_enabled: false,
            },
            telegram: TelegramConfig::default(),
            override_config: OverrideConfig {
                resume_after_override: ResumeAfterOverride::Continue,
            },
            notifications: true,
            log_blocked_attempts: true,
            onboarding: OnboardingConfig { completed: false },
            actions: ActionConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AuditLogEntry {
    pub timestamp: String,
    pub kind: String,
    pub message: String,
}

pub fn config_dir() -> PathBuf {
    if let Ok(home) = env::var("HOME") {
        return Path::new(&home).join(".config").join("wlbal");
    }
    PathBuf::from(".").join(".config").join("wlbal")
}

pub fn config_path() -> PathBuf {
    config_dir().join("config.json")
}

pub fn log_path() -> PathBuf {
    config_dir().join("log.json")
}

pub fn load_or_create_config() -> io::Result<Config> {
    let path = config_path();
    if !path.exists() {
        let config = Config::default();
        save_config_to_disk(&config)?;
        return Ok(config);
    }

    let raw = fs::read_to_string(path)?;
    let config = serde_json::from_str::<Config>(&raw)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    Ok(config)
}

pub fn save_config_to_disk(config: &Config) -> io::Result<()> {
    fs::create_dir_all(config_dir())?;
    let raw = serde_json::to_string_pretty(config)
        .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
    fs::write(config_path(), raw)?;
    Ok(())
}

pub fn append_log(kind: &str, message: impl Into<String>) {
    let _ = fs::create_dir_all(config_dir());
    let path = log_path();
    let mut entries = if path.exists() {
        fs::read_to_string(&path)
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<AuditLogEntry>>(&raw).ok())
            .unwrap_or_default()
    } else {
        Vec::new()
    };

    entries.push(AuditLogEntry {
        timestamp: Utc::now().to_rfc3339(),
        kind: kind.to_string(),
        message: message.into(),
    });

    if entries.len() > 2000 {
        entries.drain(0..entries.len() - 2000);
    }

    if let Ok(raw) = serde_json::to_string_pretty(&entries) {
        let _ = fs::write(path, raw);
    }
}

fn modified(path: &Path) -> Option<SystemTime> {
    fs::metadata(path).and_then(|meta| meta.modified()).ok()
}

pub fn spawn_config_watcher(config: Arc<RwLock<Config>>, app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let path = config_path();
        let mut last_modified = modified(&path);
        let mut ticker = interval(Duration::from_secs(2));

        loop {
            ticker.tick().await;
            let current_modified = modified(&path);
            if current_modified.is_some() && current_modified != last_modified {
                match load_or_create_config() {
                    Ok(next) => {
                        *config.write().await = next.clone();
                        let _ = app.emit("config-changed", &next);
                    }
                    Err(err) => {
                        let _ = app.emit("wlbal-error", format!("Config reload failed: {err}"));
                    }
                }
                last_modified = current_modified;
            }
        }
    });
}
