#![allow(irrefutable_let_patterns)]
mod audio;
mod backend;
mod clipboard;
mod config;
mod display;
mod grabs;
mod handlers;
mod image_capture;
mod input;
mod ipc;
mod launcher;
mod layout;
mod memory;
mod mouse;
mod native;
mod notifications;
mod panels;
mod render;
mod rice;
mod rules;
mod scratchpad;
mod screencast;
mod session;
mod state;
mod themes;
mod tray;
mod ui;
mod volume;
mod wifi;
mod winit;
mod wm;
mod xwayland;

use smithay::reexports::{
    calloop::EventLoop,
    wayland_server::{Display, DisplayHandle},
};
pub use state::Ember;
use std::{
    path::PathBuf,
    time::{Duration, Instant},
};
pub struct CalloopData {
    state: Ember,
    display_handle: DisplayHandle,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    if args
        .first()
        .is_some_and(|a| a == "--choose-screen" || a == "--choose-source")
    {
        return ipc::choose_screen();
    }
    if args.first().is_some_and(|a| a == "ctl") {
        args.remove(0);
        return ipc::client(args);
    }
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".config")
        });
    let user_config = config_home.join("emberwm/config.toml");
    let mut path = if user_config.exists() {
        user_config
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/ember.toml")
    };
    let mut check = false;
    let mut list_modes = false;
    let mut check_audio = false;
    let mut native = false;
    let mut explicit_config = false;
    let mut terminal = false;
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--config" => {
                explicit_config = true;
                i += 1;
                path = PathBuf::from(args.get(i).ok_or("--config needs a path")?);
            }
            "--check-config" => check = true,
            "--list-modes" => list_modes = true,
            "--check-audio" => check_audio = true,
            "--terminal" => terminal = true,
            "--nested" => (),
            "--native" => native = true,
            "--print-default-config" => {
                println!("{}", toml::to_string_pretty(&config::Config::default())?);
                return Ok(());
            }
            "--xwayland-helper-path" => {
                println!("{}", env!("EMBER_XWAYLAND_HELPER_DIR"));
                return Ok(());
            }
            "--version" => {
                println!("EmberWM {}", env!("CARGO_PKG_VERSION"));
                return Ok(());
            }
            "--help" | "-h" => {
                println!("EmberWM — a minimal configurable Wayland compositor\n\nUsage: emberwm [--nested | --native] [--config PATH] [--terminal]\n       emberwm --check-config [--config PATH]\n       emberwm --print-default-config\n       emberwm --native --list-modes\n       emberwm --check-audio\n       emberwm ctl [--socket PATH] COMMAND\n\nCommands: status, config, terminal, draw [X Y W H], screenshot-region [X Y W H], close, float, fullscreen,\n          focus-next, focus-prev, workspace N, send N, scratchpad-mark, scratchpad-toggle, ratio-up, ratio-down, themes, theme NAME, theme-cycle,\n          rice list, rice NAME, rice save NAME, rice-cycle, pointer-release,\n          reload, rules-apply, panels, panels-toggle, shortcuts, shortcuts-toggle, notifications,\n          notify SUMMARY | BODY, notification-dismiss, notifications-clear,\n          notifications-toggle-dnd, wifi, wifi-toggle, wifi-refresh, launcher, launcher-toggle, screencast, screenshot /absolute/path.png, exec COMMAND, quit\n\n--native: standalone DRM/KMS session; --nested: window inside an existing desktop.\nStandalone modifier: Super; nested default: Alt. Mod+Enter: terminal; Mod+D: draw terminal; Mod+Shift+G: mark scratchpad; Mod+G: show/hide scratchpad;\nMod+Space: launcher; Mod+Shift+Space: float; Mod+T: theme; Mod+Shift+T: rice profile; Mod+R: reload; Mod+B: panels; Mod+/: shortcuts; Mod+.: Wi-Fi; Mod+Shift+S: region screenshot; F11: fullscreen; Mod+Shift+Escape: quit. Escape cancels drawing.");
                return Ok(());
            }
            arg => return Err(format!("unknown argument: {arg}").into()),
        }
        i += 1;
    }
    if native && !explicit_config && !config_home.join("emberwm/config.toml").exists() {
        path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("config/session.toml");
    }
    let path = std::fs::canonicalize(path)?;
    let config = config::Config::load(&path)?;
    render::validate_wallpaper(&config, &path)?;
    panels::validate_resources(&config.panels, &path)?;
    ui::validate_resources(&config, &path)?;
    if list_modes {
        return display::list_modes(config.display.refresh_hz);
    }
    if check_audio {
        println!("{}", serde_json::to_string_pretty(&audio::probe())?);
        return Ok(());
    }
    if check {
        println!("Configuration valid: {}", path.display());
        return Ok(());
    }
    if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
        return Err("XDG_RUNTIME_DIR is required".into());
    }
    if native {
        session::default_backends();
    }
    tracing_subscriber::fmt()
        .with_env_filter(
            std::env::var("RUST_LOG").unwrap_or_else(|_| "emberwm=info,smithay=warn".into()),
        )
        .init();
    let mut event_loop: EventLoop<CalloopData> = EventLoop::try_new()?;
    let display: Display<Ember> = Display::new()?;
    let display_handle = display.handle();
    let state = Ember::new(&mut event_loop, display, config, path);
    let mut data = CalloopData {
        state,
        display_handle,
    };
    let mut backend = if native {
        backend::Backend::Native(Box::new(native::Native::init(&mut event_loop, &mut data)?))
    } else {
        backend::Backend::Nested(Box::new(winit::Nested::init(&mut event_loop, &mut data)?))
    };
    std::env::remove_var("DISPLAY");
    std::env::remove_var("XAUTHORITY");
    xwayland::start(&mut data, event_loop.handle());
    data.state
        .tray
        .maintain(data.state.config.tray.enabled, event_loop.handle());
    let _socket_guard = ipc::listen(&mut event_loop, &data.state)?;
    data.state
        .launcher
        .set_session(data.state.socket_name.clone(), data.state.ipc_path());
    if native {
        if let Err(error) = session::export_activation_environment(&data.state) {
            tracing::warn!(%error,"could not export session activation environment");
        }
    }
    let shutdown = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    signal_hook::flag::register(signal_hook::consts::SIGTERM, shutdown.clone())?;
    signal_hook::flag::register(signal_hook::consts::SIGINT, shutdown.clone())?;
    tracing::info!(wayland_display=?data.state.socket_name, ipc_socket=?data.state.ipc_path(), backend=data.state.backend_name, "EmberWM started");
    let mut notification_service = notifications::Service::default();
    notification_service.update(
        &event_loop.handle(),
        data.state.config.notifications.enabled,
        data.state.theme_generation,
        &data.state.config.notifications,
        &mut data.state.notifications,
    );
    if terminal {
        data.state.spawn_terminal(None)?;
    }
    let mut audio_session = if native && data.state.config.audio.enabled {
        Some(audio::AudioSession::start())
    } else {
        None
    };
    if let Some(audio) = &audio_session {
        data.state.audio_status = audio.status.clone();
    }
    let mut next_frame = Instant::now();
    loop {
        xwayland::maintenance(&mut data, event_loop.handle());
        backend.process_events(&mut data)?;
        backend.import_buffers(&mut data);
        data.state.maintenance();
        data.state
            .notifications
            .advance(&data.state.config.notifications);
        notification_service.update(
            &event_loop.handle(),
            data.state.config.notifications.enabled,
            data.state.theme_generation,
            &data.state.config.notifications,
            &mut data.state.notifications,
        );
        let ui_size = data.state.output_size();
        let share_changed = data.state.screencast.update(
            &data.state.config,
            &data.state.config_path,
            data.state.theme_generation,
            ui_size,
            data.state.session_active,
        );
        if let Some(error) = data.state.launcher.take_launch_error() {
            data.state.notifications.notify(
                &data.state.config.notifications,
                notifications::Request {
                    app: "Ember launcher",
                    replaces: 0,
                    summary: "Application could not open",
                    body: &error,
                    actions: vec![],
                    urgency: 1,
                    resident: false,
                    timeout: -1,
                    sender: None,
                },
            );
        }
        let launcher_changed = data.state.launcher.update(
            &data.state.config,
            &data.state.config_path,
            data.state.theme_generation,
            ui_size,
        );
        let wifi_changed = data.state.wifi.update(
            &data.state.config,
            &data.state.config_path,
            data.state.theme_generation,
            ui_size,
        );
        data.state
            .tray
            .maintain(data.state.config.tray.enabled, event_loop.handle());
        let tray_changed = data.state.tray.update(
            &data.state.config,
            &data.state.config_path,
            data.state.theme_generation,
            ui_size,
        );
        let volume_changed = data.state.volume.update(
            &data.state.config,
            &data.state.config_path,
            data.state.theme_generation,
            ui_size,
        );
        let ui_changed = data.state.ui.update(
            &data.state.config,
            &data.state.config_path,
            data.state.theme_generation,
            native,
            ui_size,
            &data.state.notifications,
        );
        let panels_changed = if data.state.config.panels.active().next().is_some() {
            let panel_input = panels::Input::from_state(&data.state);
            data.state.panels.update(
                &data.state.config.panels,
                &data.state.config_path,
                data.state.theme_generation,
                panel_input,
            )
        } else {
            data.state.panels.disable()
        };
        if tray_changed
            || volume_changed
            || panels_changed
            || ui_changed
            || wifi_changed
            || launcher_changed
            || share_changed
        {
            data.state.dirty = true;
            let pointer = data.state.seat.get_pointer().unwrap();
            if !pointer.is_grabbed() && data.state.draw.is_none() && data.state.drag.is_none() {
                let pos = pointer.current_location();
                let time = data.state.start_time.elapsed().as_millis() as u32;
                data.state.pointer_motion(pos, time);
            }
        }
        if native && data.state.config.audio.enabled != audio_session.is_some() {
            drop(audio_session.take());
            audio_session = if data.state.config.audio.enabled {
                Some(audio::AudioSession::start())
            } else {
                None
            };
            data.state.audio_status = audio::AudioStatus::default();
        }
        if let Some(audio) = &mut audio_session {
            audio.poll();
            data.state.audio_status = audio.status.clone();
        }
        if data.state.dirty && backend.can_render(&data.state) && Instant::now() >= next_frame {
            let frame_started = Instant::now();
            backend.render(&mut data)?;
            next_frame = frame_started + display::active_interval(&data.state);
        }
        memory::reclaim_if_idle(data.state.dirty);
        data.display_handle.flush_clients()?;
        // Sleep in the event loop when idle; wake promptly for clients, input and IPC.
        let mut timeout = if data.state.dirty && backend.can_render(&data.state) {
            next_frame.saturating_duration_since(Instant::now())
        } else {
            Duration::from_secs(1)
        };
        if let Some(panel_timeout) = data.state.panels.next_wake() {
            timeout = timeout.min(panel_timeout);
        }
        if let Some(wake) = data.state.notifications.next_wake() {
            timeout = timeout.min(wake);
        }
        if let Some(wake) = notification_service.next_wake() {
            timeout = timeout.min(wake);
        }
        if let Some(wake) = data.state.screencast.next_wake() {
            timeout = timeout.min(wake);
        }
        if data.state.tray.busy()
            || data.state.volume.busy()
            || data.state.wifi.busy()
            || data.state.launcher.busy()
        {
            // Short-lived network work is polled only while pending; closed idle has no timer.
            timeout = timeout.min(Duration::from_millis(50));
        }
        if let Some(wake) = data.state.volume.wake() {
            timeout = timeout.min(wake);
        }
        event_loop.dispatch(Some(timeout), &mut data)?;
        if !data.state.running || shutdown.load(std::sync::atomic::Ordering::Relaxed) {
            break;
        }
    }
    Ok(())
}
