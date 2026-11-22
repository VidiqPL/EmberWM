use crate::{CalloopData, Ember};
use serde_json::{json, Value};
use smithay::reexports::calloop::{generic::Generic, EventLoop, Interest, Mode, PostAction};
use std::{
    io,
    os::unix::{fs::PermissionsExt, net::UnixDatagram},
    path::PathBuf,
    time::Duration,
};

pub struct SocketGuard(pub PathBuf);
impl Drop for SocketGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.0);
    }
}

fn runtime() -> Result<PathBuf, String> {
    let path = std::env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .ok_or("XDG_RUNTIME_DIR is not set")?;
    if !path.is_dir() {
        return Err("XDG_RUNTIME_DIR is not a directory".into());
    }
    Ok(path)
}
impl Ember {
    pub fn ipc_path(&self) -> PathBuf {
        // XDG_RUNTIME_DIR was validated before the compositor was initialized.
        runtime().unwrap().join(format!(
            "emberwm-{}.sock",
            self.socket_name.to_string_lossy()
        ))
    }
    pub fn status(&self) -> Value {
        let windows: Vec<_> = self.windows.iter().map(|m| {
            let (app_id, title) = crate::wm::metadata(&m.window);
            json!({"app_id":app_id, "title":title, "workspace":m.workspace+1,
                "x11":m.window.is_x11(),"x11_id":m.window.x11_surface().map(|s|s.window_id()),"floating":m.floating, "scratchpad":m.scratchpad.is_some(), "fullscreen":m.fullscreen, "rules_applied":m.applied_rules.count_ones(), "placement_ready":m.initialized,"drawn":m.drawn,
                "visible":self.space.element_location(&m.window).is_some(),
                "focused":self.focused.as_ref()==Some(&m.window),
                "geometry":{"x":m.rect.loc.x,"y":m.rect.loc.y,"width":m.rect.size.w,"height":m.rect.size.h}})
        }).collect();
        let size = self.output_size();
        let pointer = self.seat.get_pointer().unwrap().current_location();
        let mode = self.space.outputs().next().and_then(|o| o.current_mode());
        json!({"version":env!("CARGO_PKG_VERSION"), "backend":self.backend_name, "session_active":self.session_active, "pid":std::process::id(),"keyboard_focus_app":self.seat.get_keyboard().and_then(|k|k.current_focus()).and_then(|s|self.windows.iter().find(|w|crate::wm::has_surface(&w.window,&s)).map(|w|crate::wm::metadata(&w.window).0)),
            "pointer":{"x":pointer.x,"y":pointer.y}, "pointer_grabbed":self.seat.get_pointer().unwrap().is_grabbed(), "pointer_buttons":self.seat.get_pointer().unwrap().pressed_buttons(), "wayland_display":self.socket_name.to_string_lossy(), "ipc_socket":self.ipc_path(), "workspace":self.workspace+1,"workspace_count":self.config.layout.workspaces,"xwayland":self.xwayland.status(),
            "output":{"width":size.w,"height":size.h,"refresh_hz":mode.map(|m|f64::from(m.refresh)/1000.0)}, "menu":self.menu.status(),"power":self.power.status(),"audio":self.audio_status,"tray":self.tray.status(),"volume_osd":{"visible":self.volume.view.is_some(),"busy":self.volume.busy(),"error":self.volume.error}, "panels":self.panels.status(), "notifications":self.notifications.status(), "ui":self.ui.status(), "wifi":self.wifi.status(), "windows":windows,"scratchpad":{"visible":self.scratchpad.visible,"windows":self.windows.iter().filter(|m|m.scratchpad.is_some()).count()},
            "draw_armed":self.draw.is_some(), "screenshot_armed":self.draw.as_ref().is_some_and(|d|d.screenshot), "clipboard":self.clipboard.status(), "pending_draws":self.pending_draws.len(), "frames_submitted":self.frames_submitted,
            "theme_generation":self.theme_generation, "config":self.config_path,"launcher":self.launcher.status(),"image_capture":{"pending":self.image_capture.pending(),"frames":self.image_capture.frames,"failures":self.image_capture.failures},"pointer_locked":self.pointer_locked(),"screencast":self.screencast.status()})
    }
    fn ipc_command(&mut self, command: &str) -> Result<Value, String> {
        match command {
            "status" => return Ok(self.status()),
            "panels" => return Ok(self.panels.status()),
            "tray" => return Ok(self.tray.status()),
            _ if command.starts_with("tray-menu ")
                || command.starts_with("tray-activate ")
                || command.starts_with("tray-select ") =>
            {
                let (verb, index) = command.split_once(' ').unwrap();
                let index = index
                    .parse::<usize>()
                    .map_err(|_| "Expected tray row index")?;
                if !self.tray.open {
                    self.action("tray-toggle")?;
                }
                self.tray.select(index, verb == "tray-menu");
                self.dirty = true;
                return Ok(self.tray.status());
            }
            "notifications" => return Ok(self.notifications.status()),
            _ if command.starts_with("notification-action ") => {
                let (id, key) = command[20..]
                    .split_once(' ')
                    .ok_or("notification-action ID KEY")?;
                let id = id.parse().map_err(|_| "invalid notification id")?;
                self.notifications.invoke(id, key);
                self.dirty = true;
            }
            "menu" => return Ok(self.menu.status()),
            "power" => return Ok(self.power.status()),
            "wifi" => return Ok(self.wifi.status()),
            "launcher" => return Ok(self.launcher.status()),
            "screencast" => return Ok(self.screencast.status()),
            _ if command == "share-request" || command.starts_with("share-request ") => {
                if !self.config.screencast.enabled {
                    return Err("screen sharing is disabled".into());
                }
                let output = self.space.outputs().next().ok_or("no display")?.name();
                self.launcher.close();
                self.close_menu();
                self.ui.close();
                self.draw = None;
                self.drag = None;
                crate::image_capture::refresh_toplevels(self);
                let mut choices = vec![crate::screencast::Choice {
                    label: format!("Display · {output}"),
                    value: output,
                }];
                for m in &self.windows {
                    if let Some(f) = &m.foreign {
                        choices.push(crate::screencast::Choice {
                            label: format!("Window · {} ({})", f.title(), f.app_id()),
                            value: format!("Window: {}", f.identifier()),
                        });
                    }
                }
                if let Some(json) = command.strip_prefix("share-request ") {
                    let allowed: Vec<String> =
                        serde_json::from_str(json).map_err(|e| e.to_string())?;
                    choices.retain(|c| allowed.contains(&c.value));
                }
                choices.truncate(128);
                let token = self.screencast.request(choices)?;
                self.suspend_pointer();
                self.dirty = true;
                return Ok(json!({"token":token}));
            }
            _ if command.starts_with("share-select ") => {
                self.screencast.choose_source(&command[13..])?;
                self.dirty = true;
            }
            _ if command.starts_with("share-result ") => {
                let token = command[13..].parse().map_err(|_| "invalid share token")?;
                return Ok(json!({"output":self.screencast.result(token)?}));
            }
            _ if command.starts_with("share-respond ") => {
                let (token, choice) = command[14..]
                    .split_once(' ')
                    .ok_or("share-respond TOKEN allow|deny")?;
                if self.screencast.status()["prompt_token"].as_u64() != token.parse().ok() {
                    return Err("unknown share token".into());
                }
                if !["allow", "deny"].contains(&choice) {
                    return Err("expected allow or deny".into());
                }
                self.screencast.decide(choice == "allow");
                self.dirty = true;
            }
            "wifi-refresh" => {
                if self.config.wifi.enabled && self.wifi.open {
                    self.wifi.refresh();
                }
            }
            "shortcuts" => {
                return Ok(
                    serde_json::json!({"visible":self.ui.help_open,"bindings":crate::ui::shortcut_rows(&self.config,self.backend_name=="native")}),
                )
            }
            _ if command.starts_with("notify ") => {
                let (summary, body) = command[7..].split_once('|').unwrap_or((&command[7..], ""));
                let id = self.notifications.notify(
                    &self.config.notifications,
                    crate::notifications::Request {
                        app: "EmberWM",
                        replaces: 0,
                        summary: summary.trim(),
                        body: body.trim(),
                        actions: vec![],
                        urgency: 1,
                        resident: false,
                        timeout: -1,
                        sender: None,
                    },
                );
                return Ok(serde_json::json!({"id":id}));
            }
            "rice list" | "rices" => {
                return Ok(
                    json!({"current":self.config.rice.profile,"available":crate::rice::names(&self.config_path),"directory":self.config_path.parent().unwrap_or(std::path::Path::new(".")).join("rices")}),
                )
            }
            _ if command.starts_with("rice save ") => {
                let name = command[10..].trim();
                let path = crate::rice::save(&self.config, &self.config_path, name)?;
                return Ok(json!({"saved":name,"path":path}));
            }
            "themes" => {
                return Ok(
                    json!({"current":self.config.theme.preset,"available":crate::themes::names(&self.config_path)}),
                )
            }
            "config" => return serde_json::to_value(&self.config).map_err(|e| e.to_string()),
            _ if command.starts_with("screenshot ") => {
                let path = PathBuf::from(&command[11..]);
                if !path.is_absolute() {
                    return Err("screenshot path must be absolute".into());
                }
                if path.extension().is_none_or(|e| e != "png") {
                    return Err("screenshot path must end in .png".into());
                }
                self.screenshot = Some(path);
                self.dirty = true;
            }
            _ if command.starts_with("screenshot-region ") => {
                let numbers = command[18..]
                    .split_whitespace()
                    .map(str::parse::<i32>)
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| "screenshot region requires four integers")?;
                if numbers.len() != 4 {
                    return Err("screenshot region requires X Y W H".into());
                }
                self.request_region(smithay::utils::Rectangle::new(
                    (numbers[0], numbers[1]).into(),
                    (numbers[2], numbers[3]).into(),
                ))?;
            }
            _ if command.starts_with("draw ") => {
                let args = command[5..]
                    .split_whitespace()
                    .map(|a| a.parse::<i32>())
                    .collect::<Result<Vec<_>, _>>()
                    .map_err(|_| "draw expects x y width height")?;
                if args.len() != 4 {
                    return Err("draw expects x y width height".into());
                }
                let size = self.output_size();
                let (x, y, w, h) = (args[0], args[1], args[2], args[3]);
                if x < 0
                    || y < 0
                    || w < 80
                    || h < 60
                    || x.checked_add(w).is_none_or(|n| n > size.w)
                    || y.checked_add(h).is_none_or(|n| n > size.h)
                {
                    return Err("draw rectangle must fit output and be at least 80x60".into());
                }
                self.spawn_terminal(Some(smithay::utils::Rectangle::new(
                    (x, y).into(),
                    (w, h).into(),
                )))?;
            }
            _ => self.action(command)?,
        }
        Ok(json!({"ok":true}))
    }
}

pub fn listen(
    event_loop: &mut EventLoop<CalloopData>,
    state: &Ember,
) -> Result<SocketGuard, Box<dyn std::error::Error>> {
    let path = state.ipc_path();
    // Never unlink a socket owned by a running/unknown instance.
    let socket = UnixDatagram::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    socket.set_nonblocking(true)?;
    event_loop.handle().insert_source(
        Generic::new(socket, Interest::READ, Mode::Level),
        |_, socket, data| {
            let mut buf = [0u8; 8192];
            // Bound each dispatch so a burst of local commands cannot starve input/rendering.
            for _ in 0..32 {
                match socket.recv_from(&mut buf) {
                    Ok((n, peer)) => {
                        let reply = match std::str::from_utf8(&buf[..n]) {
                            Ok(command) if n < buf.len() => data
                                .state
                                .ipc_command(command.trim())
                                .unwrap_or_else(|error| json!({"error":error})),
                            _ => json!({"error":"invalid or oversized command"}),
                        };
                        if let Some(path) = peer.as_pathname() {
                            let _ = socket.send_to(reply.to_string().as_bytes(), path);
                        }
                    }
                    Err(e) if e.kind() == io::ErrorKind::WouldBlock => break,
                    Err(e) => return Err(e),
                }
            }
            Ok(PostAction::Continue)
        },
    )?;
    Ok(SocketGuard(path))
}

pub fn client(mut args: Vec<String>) -> Result<(), Box<dyn std::error::Error>> {
    let runtime = runtime()?;
    let path = if args.first().is_some_and(|a| a == "--socket") {
        if args.len() < 3 {
            return Err("ctl --socket PATH COMMAND".into());
        }
        args.remove(0);
        PathBuf::from(args.remove(0))
    } else if let Some(p) = std::env::var_os("EMBER_SOCKET") {
        PathBuf::from(p)
    } else {
        let sockets: Vec<_> = std::fs::read_dir(&runtime)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("emberwm-wayland-"))
                    && p.extension().is_some_and(|e| e == "sock")
            })
            .collect();
        match sockets.as_slice() {
            [path] => path.clone(),
            [] => return Err("no Ember instance found".into()),
            _ => return Err("multiple Ember instances: use ctl --socket PATH COMMAND".into()),
        }
    };
    if args.is_empty() {
        return Err("missing command (try status)".into());
    }
    let local = runtime.join(format!(
        "emberctl-{}-{}.sock",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_nanos()
    ));
    let socket = UnixDatagram::bind(&local)?;
    let _guard = SocketGuard(local);
    socket.set_read_timeout(Some(Duration::from_secs(3)))?;
    socket.connect(path)?;
    socket.send(args.join(" ").as_bytes())?;
    let mut buf = vec![0; 262144];
    let n = socket.recv(&mut buf)?;
    let reply: Value = serde_json::from_slice(&buf[..n])?;
    println!("{}", serde_json::to_string_pretty(&reply)?);
    if reply.get("error").is_some_and(|v| !v.is_null()) {
        return Err("command failed".into());
    }
    Ok(())
}

/// A short-lived portal chooser. stdout is reserved for xdg-desktop-portal-wlr.
pub fn choose_screen() -> Result<(), Box<dyn std::error::Error>> {
    let dir = runtime()?;
    let server = if let Some(path) = std::env::var_os("EMBER_SOCKET") {
        PathBuf::from(path)
    } else {
        let sockets: Vec<_> = std::fs::read_dir(&dir)?
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .is_some_and(|n| n.to_string_lossy().starts_with("emberwm-wayland-"))
                    && p.extension().is_some_and(|n| n == "sock")
            })
            .collect();
        match sockets.as_slice() {
            [path] => path.clone(),
            _ => return Err("cannot locate the Ember sharing session".into()),
        }
    };
    let path = dir.join(format!("ember-chooser-{}.sock", std::process::id()));
    let socket = UnixDatagram::bind(&path)?;
    let _guard = SocketGuard(path);
    socket.set_read_timeout(Some(Duration::from_secs(2)))?;
    socket.connect(server)?;
    let rpc = |command: &str| -> Result<Value, Box<dyn std::error::Error>> {
        socket.send(command.as_bytes())?;
        let mut buffer = [0; 8192];
        let n = socket.recv(&mut buffer)?;
        let reply: Value = serde_json::from_slice(&buffer[..n])?;
        if let Some(error) = reply.get("error") {
            return Err(error.to_string().into());
        }
        Ok(reply)
    };
    let mut labels = Vec::<(String, String)>::new();
    if std::env::args().any(|a| a == "--choose-source") {
        use std::io::{BufRead, Read};
        for line in std::io::BufReader::new(std::io::stdin().take(65536))
            .lines()
            .take(128)
        {
            let line = line?;
            if let Some(name) = line.strip_prefix("Monitor: ") {
                if let Some(name) = name.split_whitespace().next() {
                    labels.push((name.to_string(), line));
                }
            } else if line.starts_with("Window: ") {
                if let Some((_, id)) = line.rsplit_once(" (") {
                    if let Some(id) = id.strip_suffix(')') {
                        labels.push((format!("Window: {id}"), line));
                    }
                }
            }
        }
    }
    if labels.is_empty() && std::env::args().any(|a| a == "--choose-source") {
        return Ok(());
    }
    let request = if labels.is_empty() {
        "share-request".into()
    } else {
        format!(
            "share-request {}",
            serde_json::to_string(&labels.iter().map(|(id, _)| id).collect::<Vec<_>>())?
        )
    };
    let token = rpc(&request)?["token"].as_u64().ok_or("no sharing token")?;
    let deadline = std::time::Instant::now() + Duration::from_secs(65);
    while std::time::Instant::now() < deadline {
        let reply = rpc(&format!("share-result {token}"))?;
        if let Some(output) = reply["output"].as_str() {
            if !output.is_empty() {
                if let Some((_, label)) = labels.iter().find(|(id, _)| id == output) {
                    println!("{label}");
                } else if output.starts_with("Window: ") {
                    println!("{output}");
                } else {
                    println!("Monitor: {output}");
                }
            }
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = rpc(&format!("share-respond {token} deny"));
    Ok(())
}
