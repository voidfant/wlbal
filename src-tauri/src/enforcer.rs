use crate::{
    config::{append_log, Config},
    timer::{Phase, TimerHandle},
};
use plist::Value;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    env,
    fs,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};
use tauri::{AppHandle, Emitter};
use tokio::{sync::RwLock, time::{interval, Duration}};

const HOSTS_START: &str = "# wlbal-block-start";
const HOSTS_END: &str = "# wlbal-block-end";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppInfo {
    pub name: String,
    pub bundle_id: String,
    pub path: String,
    pub executable: String,
    pub icon_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockAttemptPayload {
    pub app_name: Option<String>,
    pub domain: Option<String>,
    pub phase: Phase,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PermissionCheck {
    pub hosts_writable: bool,
    pub hosts_message: String,
}

pub fn get_installed_apps() -> Vec<AppInfo> {
    let mut apps = Vec::new();
    for dir in app_search_dirs() {
        collect_apps(&dir, 0, &mut apps);
    }
    apps.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    apps.dedup_by(|a, b| a.bundle_id == b.bundle_id);
    apps
}

pub fn check_permissions() -> PermissionCheck {
    let writable = fs::OpenOptions::new().append(true).open("/etc/hosts").is_ok();
    PermissionCheck {
        hosts_writable: writable,
        hosts_message: if writable {
            "/etc/hosts is writable".into()
        } else {
            "Hosts updates will request administrator privileges when rules change".into()
        },
    }
}

pub fn install_cli_binary() -> Result<String, String> {
    let current = env::current_exe().map_err(|err| err.to_string())?;
    let cli = current
        .parent()
        .ok_or_else(|| "Cannot locate app executable directory".to_string())?
        .join("wlbal");

    let source = if cli.exists() { cli } else { current };

    let script = format!("cp {} /usr/local/bin/wlbal && chmod 755 /usr/local/bin/wlbal", shell_quote(&source));
    run_admin_script(&script).map_err(|err| format!("CLI install failed: {err}"))?;
    Ok("/usr/local/bin/wlbal".into())
}

pub fn spawn_enforcer(
    config: Arc<RwLock<Config>>,
    timer: TimerHandle,
    enforcement_enabled: Arc<RwLock<bool>>,
    app: AppHandle,
) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = interval(Duration::from_secs(2));
        let mut last_hosts_key = String::new();
        let mut was_armed = false;
        let mut installed_cache = Vec::new();
        let mut refresh_installed_in = 0u32;

        loop {
            ticker.tick().await;
            let snapshot = timer.snapshot().await;
            let armed = *enforcement_enabled.read().await;
            let current_config = config.read().await.clone();

            if !armed || snapshot.paused {
                let hosts_key = "suspended".to_string();
                if hosts_key != last_hosts_key {
                    if current_config.sites.hosts_blocking_enabled && hosts_has_block() {
                        if let Err(err) = write_hosts_block(&[], was_armed || snapshot.paused) {
                            let _ = app.emit("wlbal-error", format!("Hosts unblock failed: {err}"));
                        }
                    }
                    last_hosts_key = hosts_key;
                }
                was_armed = armed;
                continue;
            }
            was_armed = true;

            if refresh_installed_in == 0 {
                installed_cache = tokio::task::spawn_blocking(get_installed_apps)
                    .await
                    .unwrap_or_default();
                refresh_installed_in = 30;
            } else {
                refresh_installed_in -= 1;
            }

            prune_stale_apps(&config, &current_config, &installed_cache).await;
            enforce_apps(&current_config, snapshot.phase, &installed_cache, &app).await;

            let domains = active_domains(&current_config, snapshot.phase);
            enforce_browser_urls(&current_config, snapshot.phase, &domains, &app).await;
            let hosts_key = format!(
                "{:?}:{:?}:{}",
                snapshot.phase, domains, current_config.sites.hosts_blocking_enabled
            );
            if hosts_key != last_hosts_key {
                if !current_config.sites.hosts_blocking_enabled {
                    last_hosts_key = hosts_key;
                    continue;
                }
                if domains.is_empty() && !hosts_has_block() {
                    last_hosts_key = hosts_key;
                    continue;
                }
                match write_hosts_block(&domains, true) {
                    Ok(()) => {
                        last_hosts_key = hosts_key;
                        for domain in domains {
                            let _ = app.emit(
                                "block-attempt",
                                &BlockAttemptPayload {
                                    app_name: None,
                                    domain: Some(domain),
                                    phase: snapshot.phase,
                                },
                            );
                        }
                    }
                    Err(err) => {
                        let _ = app.emit("wlbal-error", format!("Hosts update failed: {err}"));
                    }
                }
            }
        }
    });
}

pub async fn apply_website_rules_now(
    config: &Config,
    timer: &TimerHandle,
    enforcement_enabled: bool,
    allow_admin_prompt: bool,
) -> Result<Vec<String>, String> {
    let snapshot = timer.snapshot().await;
    if !config.sites.hosts_blocking_enabled {
        return Ok(active_domains(config, snapshot.phase));
    }

    if !enforcement_enabled || snapshot.paused {
        if hosts_has_block() {
            write_hosts_block(&[], allow_admin_prompt)?;
        }
        return Ok(Vec::new());
    }

    let domains = active_domains(config, snapshot.phase);
    write_hosts_block(&domains, allow_admin_prompt)?;
    Ok(domains)
}

async fn enforce_apps(config: &Config, phase: Phase, installed: &[AppInfo], app: &AppHandle) {
    let blocked = active_blocked_apps(config, phase, installed);
    if blocked.is_empty() {
        return;
    }

    for info in blocked {
        if is_wlbal_app(&info) {
            continue;
        }
        let pids = pids_for_executable(&info.executable);
        for pid in pids {
            unsafe {
                libc::kill(pid, libc::SIGTERM);
            }

            if config.notifications {
                show_notification(&format!("{} is blocked during {:?} time.", info.name, phase));
            }
            if config.log_blocked_attempts {
                append_log(
                    "blocked_app",
                    format!("Terminated {} ({}) during {:?}", info.name, info.bundle_id, phase),
                );
            }
            let _ = app.emit(
                "block-attempt",
                &BlockAttemptPayload {
                    app_name: Some(info.name.clone()),
                    domain: None,
                    phase,
                },
            );
        }
    }
}

async fn enforce_browser_urls(config: &Config, phase: Phase, domains: &[String], app: &AppHandle) {
    if domains.is_empty() {
        return;
    }

    let domains = domains.to_vec();
    let result = tokio::task::spawn_blocking(move || browser_block_attempts(phase, &domains)).await;
    let attempts = match result {
        Ok(Ok(attempts)) => attempts,
        Ok(Err(err)) => {
            append_log("browser_enforcer_error", err);
            return;
        }
        Err(err) => {
            append_log("browser_enforcer_error", err.to_string());
            return;
        }
    };

    for attempt in attempts {
        if config.notifications {
            show_notification(&format!(
                "{} is blocked during {:?} time.",
                attempt.domain, phase
            ));
        }
        if config.log_blocked_attempts {
            append_log(
                "blocked_site",
                format!(
                    "Redirected {} in {} during {:?}",
                    attempt.domain, attempt.browser, phase
                ),
            );
        }
        let _ = app.emit(
            "block-attempt",
            &BlockAttemptPayload {
                app_name: None,
                domain: Some(attempt.domain),
                phase,
            },
        );
    }
}

#[derive(Debug, Clone)]
struct BrowserBlockAttempt {
    browser: String,
    domain: String,
}

fn browser_block_attempts(phase: Phase, domains: &[String]) -> Result<Vec<BrowserBlockAttempt>, String> {
    let mut attempts = Vec::new();
    let browsers = [
        BrowserScript {
            app_name: "Safari",
            kind: BrowserScriptKind::Safari,
        },
        BrowserScript {
            app_name: "Google Chrome",
            kind: BrowserScriptKind::Chromium,
        },
        BrowserScript {
            app_name: "Brave Browser",
            kind: BrowserScriptKind::Chromium,
        },
        BrowserScript {
            app_name: "Microsoft Edge",
            kind: BrowserScriptKind::Chromium,
        },
        BrowserScript {
            app_name: "Arc",
            kind: BrowserScriptKind::Chromium,
        },
        BrowserScript {
            app_name: "Vivaldi",
            kind: BrowserScriptKind::Chromium,
        },
        BrowserScript {
            app_name: "Opera",
            kind: BrowserScriptKind::Chromium,
        },
        BrowserScript {
            app_name: "Chromium",
            kind: BrowserScriptKind::Chromium,
        },
        BrowserScript {
            app_name: "Firefox",
            kind: BrowserScriptKind::FirefoxUi,
        },
        BrowserScript {
            app_name: "Firefox Developer Edition",
            kind: BrowserScriptKind::FirefoxUi,
        },
        BrowserScript {
            app_name: "LibreWolf",
            kind: BrowserScriptKind::FirefoxUi,
        },
        BrowserScript {
            app_name: "Waterfox",
            kind: BrowserScriptKind::FirefoxUi,
        },
        BrowserScript {
            app_name: "Floorp",
            kind: BrowserScriptKind::FirefoxUi,
        },
        BrowserScript {
            app_name: "Zen Browser",
            kind: BrowserScriptKind::FirefoxUi,
        },
    ];

    for browser in browsers {
        let Some(url) = browser_active_url(browser)? else {
            continue;
        };
        let Some(host) = host_from_url(&url) else {
            continue;
        };
        let Some(domain) = matching_blocked_domain(&host, domains) else {
            continue;
        };
        redirect_browser(browser, phase, &domain)?;
        attempts.push(BrowserBlockAttempt {
            browser: browser.app_name.to_string(),
            domain,
        });
    }

    Ok(attempts)
}

#[derive(Clone, Copy)]
struct BrowserScript {
    app_name: &'static str,
    kind: BrowserScriptKind,
}

#[derive(Clone, Copy)]
enum BrowserScriptKind {
    Safari,
    Chromium,
    FirefoxUi,
}

fn browser_active_url(browser: BrowserScript) -> Result<Option<String>, String> {
    if !is_process_running(browser.app_name) {
        return Ok(None);
    }

    let script = match browser.kind {
        BrowserScriptKind::Safari => format!(
            r#"tell application "{}"
  if not running then return ""
  if (count of windows) is 0 then return ""
  return URL of current tab of front window
end tell"#,
            browser.app_name
        ),
        BrowserScriptKind::Chromium => format!(
            r#"tell application "{}"
  if not running then return ""
  if (count of windows) is 0 then return ""
  return URL of active tab of front window
end tell"#,
            browser.app_name
        ),
        BrowserScriptKind::FirefoxUi => firefox_active_url_script(browser.app_name),
    };

    let output = run_osascript(&script)?;
    let url = output.trim();
    if url.is_empty() {
        Ok(None)
    } else {
        Ok(Some(url.to_string()))
    }
}

fn redirect_browser(browser: BrowserScript, phase: Phase, domain: &str) -> Result<(), String> {
    let block_url = block_page_url(phase, domain);
    let script = match browser.kind {
        BrowserScriptKind::Safari => format!(
            r#"tell application "{}"
  if (count of windows) is 0 then return
  set URL of current tab of front window to "{}"
end tell"#,
            browser.app_name,
            applescript_string(&block_url)
        ),
        BrowserScriptKind::Chromium => format!(
            r#"tell application "{}"
  if (count of windows) is 0 then return
  set URL of active tab of front window to "{}"
end tell"#,
            browser.app_name,
            applescript_string(&block_url)
        ),
        BrowserScriptKind::FirefoxUi => firefox_redirect_script(browser.app_name, &block_url),
    };
    run_osascript(&script).map(|_| ())
}

fn firefox_active_url_script(app_name: &str) -> String {
    format!(
        r#"set oldClipboard to the clipboard
set capturedUrl to ""
try
  tell application "System Events"
    if not (exists process "{app}") then return ""
    tell process "{app}"
      if not frontmost then return ""
      keystroke "l" using command down
      delay 0.05
      keystroke "c" using command down
      delay 0.05
    end tell
  end tell
  set capturedUrl to the clipboard
end try
set the clipboard to oldClipboard
return capturedUrl"#,
        app = app_name
    )
}

fn firefox_redirect_script(app_name: &str, url: &str) -> String {
    format!(
        r#"set oldClipboard to the clipboard
try
  tell application "System Events"
    if not (exists process "{app}") then return
    tell process "{app}"
      if not frontmost then return
      set the clipboard to "{url}"
      keystroke "l" using command down
      delay 0.05
      keystroke "v" using command down
      key code 36
      delay 0.05
    end tell
  end tell
end try
set the clipboard to oldClipboard"#,
        app = app_name,
        url = applescript_string(url)
    )
}

fn block_page_url(phase: Phase, domain: &str) -> String {
    let (accent, title, message) = match phase {
        Phase::Work => (
            "#E63946",
            "Get back to work",
            "This site is blocked during Work time.",
        ),
        Phase::Leisure => (
            "#2EC4B6",
            "Back off and enjoy your leisure",
            "This site is blocked during Leisure time.",
        ),
    };

    let html = format!(
        r#"<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>wlbal blocked</title>
<style>
html,body{{height:100%;margin:0;background:#0E0E0E;color:#F0F0F0;font-family:Inter,-apple-system,BlinkMacSystemFont,"Segoe UI",sans-serif;}}
body{{display:grid;place-items:center;}}
main{{width:min(760px,calc(100vw - 48px));border-top:6px solid {accent};padding:42px 0;}}
.mark{{width:18px;height:18px;background:{accent};margin-bottom:28px;}}
h1{{margin:0;font-family:"JetBrains Mono","IBM Plex Mono",ui-monospace,Menlo,monospace;font-size:clamp(42px,8vw,92px);line-height:.94;text-transform:uppercase;letter-spacing:0;}}
p{{margin:24px 0 0;color:#9A9A9A;font-size:18px;line-height:1.5;}}
.domain{{margin-top:30px;border:1px solid #2A2A2A;background:#1A1A1A;padding:14px 16px;color:#F0F0F0;font-family:"JetBrains Mono","IBM Plex Mono",ui-monospace,Menlo,monospace;font-size:14px;display:inline-block;}}
</style>
</head>
<body>
<main>
<div class="mark"></div>
<h1>{title}</h1>
<p>{message}</p>
<div class="domain">{domain}</div>
</main>
</body>
</html>"#,
        accent = accent,
        title = html_escape(title),
        message = html_escape(message),
        domain = html_escape(domain),
    );

    format!("data:text/html;charset=utf-8,{}", percent_encode(&html))
}

fn run_osascript(script: &str) -> Result<String, String> {
    let output = Command::new("osascript")
        .args(["-e", script])
        .output()
        .map_err(|err| err.to_string())?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(stderr.trim().to_string())
    }
}

fn is_process_running(app_name: &str) -> bool {
    Command::new("pgrep")
        .args(["-x", app_name])
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn host_from_url(url: &str) -> Option<String> {
    let mut value = url.trim().to_lowercase();
    if value.starts_with("about:")
        || value.starts_with("chrome:")
        || value.starts_with("edge:")
        || value.starts_with("data:text/html")
    {
        return None;
    }
    if let Some((_, rest)) = value.split_once("://") {
        value = rest.to_string();
    }
    if let Some((host, _)) = value.split_once('/') {
        value = host.to_string();
    }
    if let Some((host, _)) = value.split_once(':') {
        value = host.to_string();
    }
    let host = value.trim().trim_start_matches("www.").trim_matches('.');
    if host.is_empty() || host.contains(' ') {
        None
    } else {
        Some(host.to_string())
    }
}

fn html_escape(input: &str) -> String {
    input
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#39;")
}

fn applescript_string(input: &str) -> String {
    input.replace('\\', "\\\\").replace('"', "\\\"")
}

fn percent_encode(input: &str) -> String {
    let mut encoded = String::new();
    for byte in input.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'-'
            | b'_'
            | b'.'
            | b'~' => encoded.push(byte as char),
            _ => encoded.push_str(&format!("%{byte:02X}")),
        }
    }
    encoded
}

fn matching_blocked_domain(host: &str, domains: &[String]) -> Option<String> {
    let normalized_host = host.trim_start_matches("www.");
    domains.iter().find_map(|domain| {
        let normalized_domain = domain.trim_start_matches("www.");
        if normalized_host == normalized_domain
            || normalized_host.ends_with(&format!(".{normalized_domain}"))
        {
            Some(normalized_domain.to_string())
        } else {
            None
        }
    })
}

fn active_blocked_apps(config: &Config, phase: Phase, installed: &[AppInfo]) -> Vec<AppInfo> {
    let by_id: HashMap<&str, &AppInfo> = installed
        .iter()
        .filter(|app| !is_wlbal_app(app))
        .map(|app| (app.bundle_id.as_str(), app))
        .collect();

    let (blocked_ids, allowed_only) = match phase {
        Phase::Work => (&config.apps.work_blocked, &config.apps.work_allowed_only),
        Phase::Leisure => (&config.apps.leisure_blocked, &config.apps.leisure_allowed_only),
    };

    let mut blocked: Vec<AppInfo> = if !allowed_only.is_empty() {
        let allowed: HashSet<&str> = allowed_only.iter().map(String::as_str).collect();
        installed
            .iter()
            .filter(|app| !is_wlbal_app(app))
            .filter(|app| !allowed.contains(app.bundle_id.as_str()))
            .cloned()
            .collect()
    } else {
        blocked_ids
            .iter()
            .filter_map(|id| by_id.get(id.as_str()).copied().cloned())
            .collect()
    };

    if matches!(phase, Phase::Work)
        && config.telegram.enabled
        && config.telegram.block_official_clients_during_work
    {
        for app in installed {
            if is_telegram_client(app)
                && !blocked.iter().any(|blocked_app| blocked_app.bundle_id == app.bundle_id)
            {
                blocked.push(app.clone());
            }
        }
    }

    blocked
}

fn is_wlbal_app(app: &AppInfo) -> bool {
    let bundle = app.bundle_id.to_ascii_lowercase();
    let name = app.name.to_ascii_lowercase();
    let executable = app.executable.to_ascii_lowercase();
    let path = app.path.to_ascii_lowercase();

    bundle == "com.wlbal.app"
        || bundle == "com.wlbal"
        || bundle == "com.workleisurebalance.wlbal"
        || name == "wlbal"
        || name == "wlbal-desktop"
        || executable == "wlbal"
        || executable == "wlbal-desktop"
        || path.ends_with("/wlbal.app")
        || path.contains("/work-leisure-balance/")
}

fn is_telegram_client(app: &AppInfo) -> bool {
    let bundle = app.bundle_id.to_ascii_lowercase();
    let name = app.name.to_ascii_lowercase();
    let executable = app.executable.to_ascii_lowercase();
    bundle.contains("telegram") || name.contains("telegram") || executable.contains("telegram")
}

async fn prune_stale_apps(config_lock: &Arc<RwLock<Config>>, config: &Config, installed: &[AppInfo]) {
    let valid: HashSet<&str> = installed.iter().map(|app| app.bundle_id.as_str()).collect();
    let mut next = config.clone();
    next.apps.work_blocked.retain(|id| valid.contains(id.as_str()));
    next.apps.leisure_blocked.retain(|id| valid.contains(id.as_str()));
    next.apps.work_allowed_only.retain(|id| valid.contains(id.as_str()));
    next.apps.leisure_allowed_only.retain(|id| valid.contains(id.as_str()));

    if &next != config {
        *config_lock.write().await = next.clone();
        let _ = crate::config::save_config_to_disk(&next);
    }
}

fn pids_for_executable(executable: &str) -> Vec<i32> {
    Command::new("pgrep")
        .args(["-x", executable])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .filter_map(|line| line.trim().parse::<i32>().ok())
                .collect()
        })
        .unwrap_or_default()
}

fn active_domains(config: &Config, phase: Phase) -> Vec<String> {
    let source = match phase {
        Phase::Work => &config.sites.work_blocked,
        Phase::Leisure => &config.sites.leisure_blocked,
    };

    let mut domains = Vec::new();
    for raw in source {
        if let Some(base) = normalize_domain(raw) {
            domains.push(base.clone());
            domains.push(format!("www.{base}"));
        }
    }
    if matches!(phase, Phase::Work)
        && config.telegram.enabled
        && config.telegram.block_official_clients_during_work
    {
        domains.push("web.telegram.org".into());
    }
    domains.sort();
    domains.dedup();
    domains
}

pub fn normalize_domain(raw: &str) -> Option<String> {
    let mut value = raw.trim().to_lowercase();
    for prefix in ["https://", "http://"] {
        if let Some(stripped) = value.strip_prefix(prefix) {
            value = stripped.to_string();
        }
    }
    value = value.trim_start_matches("www.").to_string();
    if let Some((host, _)) = value.split_once('/') {
        value = host.to_string();
    }
    value = value.trim_matches('.').to_string();
    if value.is_empty() || value.contains(' ') {
        None
    } else {
        Some(value)
    }
}

fn write_hosts_block(domains: &[String], allow_admin_prompt: bool) -> Result<(), String> {
    let existing = fs::read_to_string("/etc/hosts").map_err(|err| err.to_string())?;
    let cleaned = strip_hosts_block(&existing);
    let mut next = cleaned.trim_end().to_string();

    if !domains.is_empty() {
        next.push_str("\n\n");
        next.push_str(HOSTS_START);
        next.push('\n');
        for domain in domains {
            next.push_str("127.0.0.1 ");
            next.push_str(domain);
            next.push('\n');
            next.push_str("::1 ");
            next.push_str(domain);
            next.push('\n');
        }
        next.push_str(HOSTS_END);
        next.push('\n');
    } else {
        next.push('\n');
    }

    let temp_path = env::temp_dir().join("wlbal-hosts");
    fs::write(&temp_path, next).map_err(|err| err.to_string())?;
    let script = format!(
        "cp {} /etc/hosts && dscacheutil -flushcache && killall -HUP mDNSResponder",
        shell_quote(&temp_path)
    );

    let result = if fs::OpenOptions::new().write(true).open("/etc/hosts").is_ok() {
        Command::new("sh")
            .args(["-c", &script])
            .status()
            .map_err(|err| err.to_string())
            .and_then(|status| if status.success() { Ok(()) } else { Err(format!("shell exited with {status}")) })
    } else if allow_admin_prompt {
        run_admin_script(&script)
    } else {
        Err("administrator approval required to update /etc/hosts".into())
    };

    result?;
    verify_hosts_state(domains)
}

fn strip_hosts_block(input: &str) -> String {
    let mut output = Vec::new();
    let mut skipping = false;

    for line in input.lines() {
        if line.trim() == HOSTS_START {
            skipping = true;
            continue;
        }
        if line.trim() == HOSTS_END {
            skipping = false;
            continue;
        }
        if !skipping {
            output.push(line);
        }
    }
    output.join("\n")
}

fn hosts_has_block() -> bool {
    fs::read_to_string("/etc/hosts")
        .map(|hosts| hosts.lines().any(|line| line.trim() == HOSTS_START))
        .unwrap_or(false)
}

fn verify_hosts_state(domains: &[String]) -> Result<(), String> {
    let hosts = fs::read_to_string("/etc/hosts").map_err(|err| err.to_string())?;
    if domains.is_empty() {
        if hosts.lines().any(|line| line.trim() == HOSTS_START) {
            return Err("wlbal block marker is still present in /etc/hosts".into());
        }
        return Ok(());
    }

    for domain in domains {
        let ipv4 = format!("127.0.0.1 {domain}");
        let ipv6 = format!("::1 {domain}");
        if !hosts.lines().any(|line| line.trim() == ipv4)
            || !hosts.lines().any(|line| line.trim() == ipv6)
        {
            return Err(format!("{domain} was not written to /etc/hosts"));
        }
    }
    Ok(())
}

fn run_admin_script(script: &str) -> Result<(), String> {
    let apple_script = format!(
        "do shell script \"{}\" with administrator privileges",
        script.replace('\\', "\\\\").replace('"', "\\\"")
    );
    Command::new("osascript")
        .args(["-e", &apple_script])
        .status()
        .map_err(|err| err.to_string())
        .and_then(|status| if status.success() { Ok(()) } else { Err(format!("osascript exited with {status}")) })
}

fn show_notification(message: &str) {
    let script = format!(
        "display notification \"{}\" with title \"wlbal\"",
        message.replace('\\', "\\\\").replace('"', "\\\"")
    );
    let _ = Command::new("osascript").args(["-e", &script]).status();
}

fn app_search_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/Applications")];
    if let Ok(home) = env::var("HOME") {
        dirs.push(Path::new(&home).join("Applications"));
    }
    dirs
}

fn collect_apps(dir: &Path, depth: usize, apps: &mut Vec<AppInfo>) {
    if depth > 4 {
        return;
    }

    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().is_some_and(|ext| ext == "app") {
            if let Some(info) = read_app_info(&path) {
                apps.push(info);
            }
        } else if path.is_dir() {
            collect_apps(&path, depth + 1, apps);
        }
    }
}

fn read_app_info(path: &Path) -> Option<AppInfo> {
    let plist_path = path.join("Contents").join("Info.plist");
    let value = Value::from_file(&plist_path).ok()?;
    let dict = value.as_dictionary()?;
    let bundle_id = dict.get("CFBundleIdentifier")?.as_string()?.to_string();
    let executable = dict.get("CFBundleExecutable")?.as_string()?.to_string();
    let name = dict
        .get("CFBundleDisplayName")
        .and_then(Value::as_string)
        .or_else(|| dict.get("CFBundleName").and_then(Value::as_string))
        .map(str::to_string)
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("Application")
                .to_string()
        });

    let icon_path = dict
        .get("CFBundleIconFile")
        .and_then(Value::as_string)
        .map(|name| {
            let mut file = name.to_string();
            if !file.ends_with(".icns") {
                file.push_str(".icns");
            }
            path.join("Contents").join("Resources").join(file).to_string_lossy().to_string()
        });

    Some(AppInfo {
        name,
        bundle_id,
        path: path.to_string_lossy().to_string(),
        executable,
        icon_path,
    })
}

fn shell_quote(path: &Path) -> String {
    format!("'{}'", path.to_string_lossy().replace('\'', "'\\''"))
}
