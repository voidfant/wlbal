use crate::config::{append_log, Config, ResumeAfterOverride};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tauri::{AppHandle, Emitter};
use tokio::{sync::RwLock, time::{interval, Duration}};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "PascalCase")]
pub enum Phase {
    Work,
    Leisure,
}

impl Phase {
    pub fn opposite(self) -> Self {
        match self {
            Phase::Work => Phase::Leisure,
            Phase::Leisure => Phase::Work,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimerSnapshot {
    pub phase: Phase,
    pub remaining_secs: u64,
    pub total_secs: u64,
    pub session_count: u32,
    pub paused: bool,
    pub override_active: bool,
    pub override_remaining_secs: Option<u64>,
    pub next_phase: Phase,
    pub next_duration_secs: u64,
    pub started_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseChangedPayload {
    pub new_phase: Phase,
    pub triggered_by: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SwitchResult {
    pub ok: bool,
    pub new_phase: Phase,
    pub duration_secs: u64,
    pub warning: Option<String>,
}

#[derive(Debug, Clone)]
struct OverrideResume {
    phase: Phase,
    remaining_secs: u64,
    total_secs: u64,
    session_count: u32,
}

#[derive(Debug, Clone)]
struct TimerInner {
    phase: Phase,
    remaining_secs: u64,
    total_secs: u64,
    session_count: u32,
    paused: bool,
    started_at: DateTime<Utc>,
    last_tick_at: DateTime<Utc>,
    override_resume: Option<OverrideResume>,
}

#[derive(Clone)]
pub struct TimerHandle {
    inner: Arc<RwLock<TimerInner>>,
    config: Arc<RwLock<Config>>,
}

impl TimerHandle {
    pub fn new(config: Arc<RwLock<Config>>, initial_config: &Config) -> Self {
        let initial = duration_for_phase(initial_config, Phase::Work, 0);
        let now = Utc::now();
        Self {
            inner: Arc::new(RwLock::new(TimerInner {
                phase: Phase::Work,
                remaining_secs: initial,
                total_secs: initial,
                session_count: 0,
                paused: true,
                started_at: now,
                last_tick_at: now,
                override_resume: None,
            })),
            config,
        }
    }

    pub async fn snapshot(&self) -> TimerSnapshot {
        let cfg = self.config.read().await;
        let mut inner = self.inner.write().await;
        let mut phase_events = Vec::new();
        advance_inner(&mut inner, &cfg, Utc::now(), &mut phase_events);
        snapshot_from_inner(&inner, &cfg)
    }

    pub async fn switch_override(&self, duration_secs: u64) -> Result<SwitchResult, String> {
        if duration_secs == 0 {
            return Err("Invalid duration".into());
        }
        let capped = duration_secs.min(2 * 60 * 60);
        let mut warning = if duration_secs > capped {
            Some("Override capped at 2h".to_string())
        } else {
            None
        };

        let mut inner = self.inner.write().await;
        if inner.override_resume.is_some() {
            warning = Some(match warning {
                Some(existing) => format!("{existing}; replaced active override"),
                None => "Replaced active override".to_string(),
            });
        }

        let resume = OverrideResume {
            phase: inner.phase,
            remaining_secs: inner.remaining_secs,
            total_secs: inner.total_secs,
            session_count: inner.session_count,
        };
        inner.phase = inner.phase.opposite();
        inner.remaining_secs = capped;
        inner.total_secs = capped;
        inner.paused = false;
        inner.started_at = Utc::now();
        inner.last_tick_at = inner.started_at;
        inner.override_resume = Some(resume);

        Ok(SwitchResult {
            ok: true,
            new_phase: inner.phase,
            duration_secs: capped,
            warning,
        })
    }

    pub async fn pause(&self) {
        let mut inner = self.inner.write().await;
        inner.paused = true;
        inner.last_tick_at = Utc::now();
        append_log("admin_override", "Timer paused via CLI");
    }

    pub async fn resume(&self) {
        let mut inner = self.inner.write().await;
        inner.paused = false;
        inner.last_tick_at = Utc::now();
        append_log("admin_override", "Timer resumed via CLI");
    }

    pub async fn start_work(&self, cfg: &Config) -> TimerSnapshot {
        self.set_work_state(cfg, false).await
    }

    pub async fn stop_at_work(&self, cfg: &Config) -> TimerSnapshot {
        self.set_work_state(cfg, true).await
    }

    async fn set_work_state(&self, cfg: &Config, paused: bool) -> TimerSnapshot {
        let mut inner = self.inner.write().await;
        let duration = duration_for_phase(cfg, Phase::Work, 0);
        let now = Utc::now();
        inner.phase = Phase::Work;
        inner.remaining_secs = duration;
        inner.total_secs = duration;
        inner.session_count = 0;
        inner.paused = paused;
        inner.started_at = now;
        inner.last_tick_at = now;
        inner.override_resume = None;
        snapshot_from_inner(&inner, cfg)
    }

    pub async fn restart_current_phase(&self, cfg: &Config) -> TimerSnapshot {
        let mut inner = self.inner.write().await;
        let duration = duration_for_phase(cfg, inner.phase, inner.session_count);
        let now = Utc::now();
        inner.remaining_secs = duration;
        inner.total_secs = duration;
        inner.started_at = now;
        inner.last_tick_at = now;
        inner.override_resume = None;
        snapshot_from_inner(&inner, cfg)
    }
}

pub fn spawn_timer(timer: TimerHandle, app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        let mut ticker = interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            let mut phase_events = Vec::new();
            let snapshot = {
                let cfg = timer.config.read().await;
                let mut inner = timer.inner.write().await;
                let now = Utc::now();

                advance_inner(&mut inner, &cfg, now, &mut phase_events);

                snapshot_from_inner(&inner, &cfg)
            };

            let _ = app.emit("timer-tick", &snapshot);
            for event in phase_events {
                let _ = app.emit("phase-changed", &event);
            }
        }
    });
}

fn advance_inner(
    inner: &mut TimerInner,
    cfg: &Config,
    now: DateTime<Utc>,
    phase_events: &mut Vec<PhaseChangedPayload>,
) {
    if inner.paused {
        inner.last_tick_at = now;
        return;
    }

    let elapsed = (now - inner.last_tick_at).num_seconds().max(0) as u64;
    if elapsed == 0 {
        return;
    }

    inner.last_tick_at += chrono::Duration::seconds(elapsed as i64);
    let mut whole_seconds = elapsed;

    while whole_seconds > 0 {
        if whole_seconds < inner.remaining_secs {
            inner.remaining_secs -= whole_seconds;
            whole_seconds = 0;
        } else {
            whole_seconds = whole_seconds.saturating_sub(inner.remaining_secs);
            complete_current_phase(inner, cfg, "schedule", phase_events);
        }
    }
}

fn complete_current_phase(
    inner: &mut TimerInner,
    cfg: &Config,
    trigger: &str,
    phase_events: &mut Vec<PhaseChangedPayload>,
) {
    if let Some(resume) = inner.override_resume.take() {
        match cfg.override_config.resume_after_override {
            ResumeAfterOverride::Continue => {
                inner.phase = resume.phase;
                inner.remaining_secs = resume.remaining_secs.max(1);
                inner.total_secs = resume.total_secs.max(1);
                inner.session_count = resume.session_count;
            }
            ResumeAfterOverride::FreshCycle => {
                inner.phase = Phase::Work;
                inner.remaining_secs = duration_for_phase(cfg, Phase::Work, inner.session_count);
                inner.total_secs = inner.remaining_secs;
            }
        }
    } else {
        match inner.phase {
            Phase::Work => {
                inner.session_count = inner.session_count.saturating_add(1);
                inner.phase = Phase::Leisure;
                inner.remaining_secs = duration_for_phase(cfg, Phase::Leisure, inner.session_count);
                inner.total_secs = inner.remaining_secs;
            }
            Phase::Leisure => {
                inner.phase = Phase::Work;
                inner.remaining_secs = duration_for_phase(cfg, Phase::Work, inner.session_count);
                inner.total_secs = inner.remaining_secs;
            }
        }
    }
    inner.started_at = Utc::now();
    inner.last_tick_at = inner.started_at;
    phase_events.push(PhaseChangedPayload {
        new_phase: inner.phase,
        triggered_by: trigger.to_string(),
    });
}

fn snapshot_from_inner(inner: &TimerInner, cfg: &Config) -> TimerSnapshot {
    let next_phase = inner.phase.opposite();
    let next_duration_secs = if inner.override_resume.is_some() {
        inner
            .override_resume
            .as_ref()
            .map(|resume| resume.remaining_secs)
            .unwrap_or_else(|| duration_for_phase(cfg, next_phase, inner.session_count))
    } else if inner.phase == Phase::Work {
        let next_session = inner.session_count.saturating_add(1);
        duration_for_phase(cfg, Phase::Leisure, next_session)
    } else {
        duration_for_phase(cfg, Phase::Work, inner.session_count)
    };

    TimerSnapshot {
        phase: inner.phase,
        remaining_secs: inner.remaining_secs,
        total_secs: inner.total_secs,
        session_count: inner.session_count,
        paused: inner.paused,
        override_active: inner.override_resume.is_some(),
        override_remaining_secs: inner.override_resume.as_ref().map(|_| inner.remaining_secs),
        next_phase,
        next_duration_secs,
        started_at: inner.started_at.to_rfc3339(),
        updated_at: inner.last_tick_at.to_rfc3339(),
    }
}

fn duration_for_phase(cfg: &Config, phase: Phase, session_count: u32) -> u64 {
    let mins = match phase {
        Phase::Work => cfg.schedule.work_mins,
        Phase::Leisure => {
            let every = cfg.schedule.sessions_before_long_break.max(1);
            if session_count > 0 && session_count % every == 0 {
                cfg.schedule.long_break_mins
            } else {
                cfg.schedule.leisure_mins
            }
        }
    };
    mins.max(1) as u64 * 60
}
