use crate::timer::{Phase, TimerHandle};
use tauri::{
    image::Image,
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::TrayIconBuilder,
    AppHandle, Manager,
};
use tokio::time::{interval, Duration};

pub fn setup_tray(app: &AppHandle, timer: TimerHandle) -> tauri::Result<()> {
    let status = MenuItem::with_id(app, "status", "wlbal starting", false, None::<&str>)?;
    let show = MenuItem::with_id(app, "show", "Show Window", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit wlbal", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&status, &show, &separator, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main").menu(&menu).on_menu_event(|app, event| {
        match event.id().as_ref() {
            "show" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        }
    });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }

    let tray = builder.build(app)?;
    tauri::async_runtime::spawn(async move {
        let mut ticker = interval(Duration::from_secs(1));
        loop {
            ticker.tick().await;
            let state = timer.snapshot().await;
            let label = format!(
                "{} {}{}",
                phase_label(state.phase),
                format_remaining(state.remaining_secs),
                if state.paused { " paused" } else { "" }
            );
            let _ = status.set_text(&label);
            let _ = tray.set_tooltip(Some(&format!("wlbal: {label}")));
            let _ = tray.set_icon(Some(phase_icon(state.phase, state.paused)));
        }
    });

    Ok(())
}

fn phase_label(phase: Phase) -> &'static str {
    match phase {
        Phase::Work => "Work",
        Phase::Leisure => "Leisure",
    }
}

fn format_remaining(secs: u64) -> String {
    let mins = secs / 60;
    let rem = secs % 60;
    format!("{mins:02}:{rem:02}")
}

fn phase_icon(phase: Phase, paused: bool) -> Image<'static> {
    let (r, g, b) = if paused {
        (102, 102, 102)
    } else {
        match phase {
            Phase::Work => (230, 57, 70),
            Phase::Leisure => (46, 196, 182),
        }
    };

    let size = 18u32;
    let mut rgba = Vec::with_capacity((size * size * 4) as usize);
    let center = (size as f32 - 1.0) / 2.0;
    for y in 0..size {
        for x in 0..size {
            let dx = x as f32 - center;
            let dy = y as f32 - center;
            let inside = (dx * dx + dy * dy).sqrt() <= center;
            if inside {
                rgba.extend_from_slice(&[r, g, b, 255]);
            } else {
                rgba.extend_from_slice(&[0, 0, 0, 0]);
            }
        }
    }

    Image::new_owned(rgba, size, size)
}
