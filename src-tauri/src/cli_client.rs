use serde::{Deserialize, Serialize};
use std::{env, process};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UnixStream,
};

const SOCKET_PATH: &str = "/tmp/wlbal.sock";

#[derive(Debug, Serialize)]
struct Request {
    command: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    duration_secs: Option<u64>,
}

#[derive(Debug, Deserialize)]
struct Response {
    ok: bool,
    error: Option<String>,
    new_phase: Option<String>,
    duration_secs: Option<u64>,
    warning: Option<String>,
    phase: Option<String>,
    remaining_secs: Option<u64>,
    session_count: Option<u32>,
    paused: Option<bool>,
    override_active: Option<bool>,
}

pub fn run() {
    let runtime = tokio::runtime::Runtime::new().unwrap_or_else(|err| {
        eprintln!("wlbal: failed to start runtime: {err}");
        process::exit(1);
    });
    runtime.block_on(async_main());
}

async fn async_main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let request = match args.as_slice() {
        [cmd] if cmd == "status" || cmd == "pause" || cmd == "resume" => Request {
            command: cmd.clone(),
            duration_secs: None,
        },
        [cmd, duration] if cmd == "switch" => match parse_duration(duration) {
            Ok(secs) => Request {
                command: "switch".into(),
                duration_secs: Some(secs),
            },
            Err(err) => {
                eprintln!("wlbal: {err}");
                process::exit(2);
            }
        },
        _ => {
            eprintln!("Usage: wlbal switch <duration> | wlbal status | wlbal pause | wlbal resume");
            eprintln!("Examples: wlbal switch 10m, wlbal switch 1h");
            process::exit(2);
        }
    };

    match send(request).await {
        Ok(response) => print_response(response),
        Err(err) => {
            eprintln!("wlbal: {err}");
            process::exit(1);
        }
    }
}

async fn send(request: Request) -> Result<Response, String> {
    let mut stream = UnixStream::connect(SOCKET_PATH)
        .await
        .map_err(|_| "app is not running. Start wlbal first.".to_string())?;
    let mut raw = serde_json::to_vec(&request).map_err(|err| err.to_string())?;
    raw.push(b'\n');
    stream.write_all(&raw).await.map_err(|err| err.to_string())?;
    stream.shutdown().await.map_err(|err| err.to_string())?;

    let mut response = Vec::new();
    stream.read_to_end(&mut response).await.map_err(|err| err.to_string())?;
    serde_json::from_slice::<Response>(&response).map_err(|err| err.to_string())
}

fn print_response(response: Response) {
    if !response.ok {
        eprintln!("wlbal: {}", response.error.unwrap_or_else(|| "request failed".into()));
        process::exit(1);
    }

    if let Some(phase) = response.new_phase {
        println!(
            "Switched to {phase} for {}.",
            format_duration(response.duration_secs.unwrap_or_default())
        );
        if let Some(warning) = response.warning {
            eprintln!("wlbal: warning: {warning}");
        }
        return;
    }

    if let Some(phase) = response.phase {
        println!(
            "Phase: {phase}\nRemaining: {}\nSession count: {}\nPaused: {}\nOverride: {}",
            format_duration(response.remaining_secs.unwrap_or_default()),
            response.session_count.unwrap_or_default(),
            response.paused.unwrap_or(false),
            response.override_active.unwrap_or(false)
        );
        return;
    }

    println!("ok");
}

fn parse_duration(input: &str) -> Result<u64, String> {
    let (number, unit) = input.split_at(input.len().saturating_sub(1));
    let value = number
        .parse::<u64>()
        .map_err(|_| "duration must look like 10m or 1h".to_string())?;
    match unit {
        "m" if value > 0 => Ok(value * 60),
        "h" if value > 0 => Ok(value * 60 * 60),
        _ => Err("duration must use minutes or hours, for example 10m or 1h".into()),
    }
}

fn format_duration(secs: u64) -> String {
    if secs >= 3600 && secs % 3600 == 0 {
        format!("{}h", secs / 3600)
    } else if secs >= 60 {
        format!("{}m {}s", secs / 60, secs % 60)
    } else {
        format!("{secs}s")
    }
}
