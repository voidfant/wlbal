use crate::timer::TimerHandle;
use serde::{Deserialize, Serialize};
use std::{fs, path::Path};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};

pub const SOCKET_PATH: &str = "/tmp/wlbal.sock";

#[derive(Debug, Deserialize)]
struct CliRequest {
    command: String,
    duration_secs: Option<u64>,
}

#[derive(Debug, Serialize)]
struct CliResponse<T: Serialize> {
    ok: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
    #[serde(flatten)]
    body: Option<T>,
}

#[derive(Debug, Serialize)]
struct SwitchBody {
    new_phase: crate::timer::Phase,
    duration_secs: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    warning: Option<String>,
}

#[derive(Debug, Serialize)]
struct StatusBody {
    phase: crate::timer::Phase,
    remaining_secs: u64,
    session_count: u32,
    paused: bool,
    override_active: bool,
}

pub fn spawn_cli_server(timer: TimerHandle) {
    tauri::async_runtime::spawn(async move {
        if Path::new(SOCKET_PATH).exists() {
            let _ = fs::remove_file(SOCKET_PATH);
        }

        let listener = match UnixListener::bind(SOCKET_PATH) {
            Ok(listener) => listener,
            Err(err) => {
                crate::config::append_log("cli_error", format!("Failed to bind socket: {err}"));
                return;
            }
        };

        loop {
            match listener.accept().await {
                Ok((stream, _)) => {
                    let timer = timer.clone();
                    tauri::async_runtime::spawn(async move {
                        handle_connection(stream, timer).await;
                    });
                }
                Err(err) => {
                    crate::config::append_log("cli_error", format!("Socket accept failed: {err}"));
                }
            }
        }
    });
}

async fn handle_connection(stream: UnixStream, timer: TimerHandle) {
    let mut reader = BufReader::new(stream);
    let mut input = Vec::new();
    let read_result = reader.read_until(b'\n', &mut input).await;
    let response = match read_result {
        Ok(0) => error_response("Empty request"),
        Ok(_) => match serde_json::from_slice::<CliRequest>(&input) {
            Ok(request) => handle_request(request, timer).await,
            Err(err) => error_response(&format!("Invalid JSON: {err}")),
        },
        Err(err) => error_response(&format!("Read failed: {err}")),
    };

    if let Ok(raw) = serde_json::to_vec(&response) {
        let stream = reader.get_mut();
        let _ = stream.write_all(&raw).await;
        let _ = stream.write_all(b"\n").await;
        let _ = stream.shutdown().await;
    }
}

async fn handle_request(request: CliRequest, timer: TimerHandle) -> serde_json::Value {
    match request.command.as_str() {
        "switch" => {
            let Some(duration_secs) = request.duration_secs else {
                return error_response("Invalid duration");
            };
            match timer.switch_override(duration_secs).await {
                Ok(result) => serde_json::to_value(CliResponse {
                    ok: true,
                    error: None,
                    body: Some(SwitchBody {
                        new_phase: result.new_phase,
                        duration_secs: result.duration_secs,
                        warning: result.warning,
                    }),
                })
                .unwrap_or_else(|_| error_response("Serialization failed")),
                Err(err) => error_response(&err),
            }
        }
        "status" => {
            let state = timer.snapshot().await;
            serde_json::to_value(CliResponse {
                ok: true,
                error: None,
                body: Some(StatusBody {
                    phase: state.phase,
                    remaining_secs: state.remaining_secs,
                    session_count: state.session_count,
                    paused: state.paused,
                    override_active: state.override_active,
                }),
            })
            .unwrap_or_else(|_| error_response("Serialization failed"))
        }
        "pause" => {
            timer.pause().await;
            serde_json::json!({ "ok": true })
        }
        "resume" => {
            timer.resume().await;
            serde_json::json!({ "ok": true })
        }
        _ => error_response("Unknown command"),
    }
}

fn error_response(message: &str) -> serde_json::Value {
    serde_json::json!({
        "ok": false,
        "error": message
    })
}
