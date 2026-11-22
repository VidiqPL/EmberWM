//! X11 compatibility and selection bridging, owned by the compositor session.
use crate::{CalloopData, Ember};
use serde::{Deserialize, Serialize};
use smithay::{
    desktop::Window,
    reexports::{
        calloop::{LoopHandle, RegistrationToken},
        wayland_server::{protocol::wl_surface::WlSurface, Client},
    },
    utils::{Logical, Rectangle},
    wayland::{
        selection::{
            data_device::{
                clear_data_device_selection, current_data_device_selection_userdata,
                request_data_device_client_selection, set_data_device_selection,
            },
            SelectionTarget,
        },
        xwayland_shell::{XWaylandShellHandler, XWaylandShellState},
    },
    xwayland::{
        xwm::{Reorder, ResizeEdge, WmWindowProperty, XwmId},
        X11Surface, X11Wm, XWayland, XWaylandEvent, XwmHandler,
    },
};
use std::{os::fd::OwnedFd, process::Stdio};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub scroll_factor: f64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            scroll_factor: 1.0,
        }
    }
}
#[derive(Default)]
pub struct Runtime {
    pub wm: Option<X11Wm>,
    client: Option<Client>,
    token: Option<RegistrationToken>,
    handle: Option<LoopHandle<'static, CalloopData>>,
    display: Option<u32>,
    pub error: Option<String>,
    restart: bool,
    retry_at: Option<std::time::Instant>,
    failures: u32,
    offered: Option<Vec<String>>,
    auth: Option<std::path::PathBuf>,
    pid_file: Option<std::path::PathBuf>,
}
impl Runtime {
    pub fn auth_name(&self) -> String {
        self.auth
            .as_ref()
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
    pub fn display_name(&self) -> String {
        self.display.map(|n| format!(":{n}")).unwrap_or_default()
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"display":self.display_name(),"ready":self.wm.is_some(),"error":self.error,"auth":self.auth_name(),"pid":self.pid_file.as_ref().and_then(|p|std::fs::read_to_string(p).ok()).and_then(|s|s.trim().parse::<u32>().ok()),"lazy":true})
    }
}
pub fn start(data: &mut CalloopData, handle: LoopHandle<'static, CalloopData>) {
    if !data.state.config.xwayland.enabled {
        return;
    }
    // A clean standalone boot need not have had an X server create this yet.
    if let Err(error) = ensure_socket_directory() {
        data.state.xwayland.error = Some(error.to_string());
        return;
    }
    let requested = data.state.xwayland.display;
    let real = std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|d| d.join("Xwayland"))
            .find(|p| p.is_file() && !p.starts_with(env!("EMBER_XWAYLAND_HELPER_DIR")))
    });
    let Some(real) = real else {
        data.state.xwayland.error = Some("Xwayland is not installed".into());
        return;
    };
    let runtime = std::path::PathBuf::from(std::env::var_os("XDG_RUNTIME_DIR").unwrap());
    let auth = runtime.join(format!("emberwm-{}-x11.auth", std::process::id()));
    let pid_file = runtime.join(format!("emberwm-{}-x11.pid", std::process::id()));
    let envs = vec![
        (
            "PATH".to_string(),
            env!("EMBER_XWAYLAND_HELPER_DIR").to_string(),
        ),
        (
            "EMBER_XWAYLAND_REAL_PATH".into(),
            std::env::var("PATH").unwrap_or_default(),
        ),
        (
            "EMBER_REAL_XWAYLAND".into(),
            real.to_string_lossy().into_owned(),
        ),
        (
            "EMBER_XWAYLAND_AUTH".into(),
            auth.to_string_lossy().into_owned(),
        ),
        (
            "EMBER_XWAYLAND_PID_FILE".into(),
            pid_file.to_string_lossy().into_owned(),
        ),
    ];
    match XWayland::spawn(
        &data.display_handle,
        requested,
        envs,
        true,
        Stdio::null(),
        Stdio::inherit(),
        |_| (),
    ) {
        Ok((server, client)) => {
            if let Err(e) = write_authority(&auth, server.display_number()) {
                data.state.xwayland.error = Some(e.to_string());
                return;
            }
            data.state.xwayland.auth = Some(auth);
            data.state.xwayland.pid_file = Some(pid_file);
            data.state.xwayland.display = Some(server.display_number());
            data.state.xwayland.client = Some(client.clone());
            data.state.xwayland.handle = Some(handle.clone());
            data.state.xwayland.error = None;
            std::env::set_var("DISPLAY", data.state.xwayland.display_name());
            std::env::set_var("XAUTHORITY", data.state.xwayland.auth_name());
            let wm_handle = handle.clone();
            let token = handle.insert_source(server, move |event, _, data| match event {
                XWaylandEvent::Ready { x11_socket, .. } => {
                    match X11Wm::start_wm(wm_handle.clone(), x11_socket, client.clone()) {
                        Ok(mut wm) => {
                            if let Some(mimes) = data.state.xwayland.offered.clone() {
                                let _ = wm.new_selection(SelectionTarget::Clipboard, Some(mimes));
                            }
                            data.state.xwayland.failures = 0;
                            data.state.xwayland.wm = Some(wm);
                        }
                        Err(e) => {
                            data.state.xwayland.error = Some(e.to_string());
                            data.state.xwayland.restart = true;
                        }
                    }
                }
                XWaylandEvent::Error => {
                    data.state.xwayland.error = Some("Xwayland startup failed".into());
                    data.state.xwayland.restart = true;
                }
            });
            match token {
                Ok(token) => data.state.xwayland.token = Some(token),
                Err(e) => data.state.xwayland.error = Some(e.to_string()),
            }
        }
        Err(e) => {
            data.state.xwayland.retry_at =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
            data.state.xwayland.error = Some(e.to_string());
            tracing::error!(%e,"Xwayland could not start");
        }
    }
}
pub fn maintenance(data: &mut CalloopData, handle: LoopHandle<'static, CalloopData>) {
    if data
        .state
        .xwayland
        .retry_at
        .is_some_and(|at| std::time::Instant::now() >= at)
    {
        data.state.xwayland.retry_at = None;
        start(data, handle.clone());
        if data.state.xwayland.client.is_none() {
            data.state.xwayland.retry_at =
                Some(std::time::Instant::now() + std::time::Duration::from_secs(30));
        }
    }
    let dead = data.state.xwayland.client.as_ref().is_some_and(|c| {
        data.display_handle
            .backend_handle()
            .get_client_data(c.id())
            .is_err()
    });
    if !dead && !data.state.xwayland.restart {
        return;
    }
    data.state.xwayland.restart = false;
    // Remove a dead X11-owned offer, but preserve native clipboard owners.
    let stale = current_data_device_selection_userdata(&data.state.seat)
        .is_some_and(|d| matches!(*d, crate::clipboard::SelectionData::X11(_)));
    if stale {
        clear_data_device_selection(&data.display_handle, &data.state.seat);
    }
    data.state.xwayland.wm = None;
    data.state.xwayland.client = None;
    if let Some(token) = data.state.xwayland.token.take() {
        handle.remove(token);
    }
    let windows: Vec<_> = data
        .state
        .windows
        .iter()
        .filter(|m| m.window.is_x11())
        .map(|m| m.window.clone())
        .collect();
    for w in windows {
        data.state.remove_x11(&w);
    }
    data.state.xwayland.failures = data.state.xwayland.failures.saturating_add(1);
    let delay = 250u64
        .saturating_mul(1u64 << data.state.xwayland.failures.min(7))
        .min(30_000);
    data.state.xwayland.retry_at =
        Some(std::time::Instant::now() + std::time::Duration::from_millis(delay));
}
impl Ember {
    pub fn sync_x11_stacking(&mut self) {
        // X11 hit testing uses its own frame stack, even for windows hidden by
        // our workspace policy. Keep hidden clients below the visible Space.
        let order: Vec<_> = self
            .windows
            .iter()
            .filter(|m| self.space.element_location(&m.window).is_none())
            .filter_map(|m| m.window.x11_surface().cloned())
            .chain(
                self.space
                    .elements()
                    .filter_map(|w| w.x11_surface().cloned()),
            )
            .collect();
        if let Some(wm) = &mut self.xwayland.wm {
            // Smithay's downward pass places each next frame BELOW its predecessor.
            if let Err(error) = wm.update_stacking_order_downwards(order.iter().rev()) {
                tracing::warn!(%error, "could not synchronize X11 stacking");
            }
        }
    }

    pub fn x11_offer(&mut self, mimes: Option<Vec<String>>) {
        self.xwayland.offered = mimes.clone();
        if let Some(wm) = &mut self.xwayland.wm {
            let _ = wm.new_selection(SelectionTarget::Clipboard, mimes);
        }
    }
    pub fn x11_send(&mut self, target: SelectionTarget, mime: String, fd: OwnedFd) {
        let fd = match self.clipboard.x11_destination(fd) {
            Ok(fd) => fd,
            Err(error) => {
                tracing::warn!(%error, "X11 clipboard destination failed");
                return;
            }
        };
        if let (Some(wm), Some(handle)) = (&mut self.xwayland.wm, &self.xwayland.handle) {
            if let Err(e) = wm.send_selection(target, mime, fd, handle.clone()) {
                tracing::warn!(%e,"X11 selection transfer failed");
            }
        }
    }
    fn map_x11(&mut self, x: X11Surface) {
        if !x.is_mapped()
            || x.wl_surface().is_none()
            || self
                .windows
                .iter()
                .any(|m| m.window.x11_surface() == Some(&x))
        {
            return;
        }
        let requested_geometry = x.geometry();
        let w = Window::new_x11_window(x.clone());
        self.manage(w.clone());
        if x.is_override_redirect() {
            let m = self.windows.iter_mut().find(|m| m.window == w).unwrap();
            m.floating = true;
            m.initialized = true;
            m.rect = x.geometry();
            self.relayout();
        } else if let Some(surface) = x.wl_surface() {
            if x.is_popup() || x.is_transient_for().is_some() || crate::wm::game_overlay(&w) {
                let m = self.windows.iter_mut().find(|m| m.window == w).unwrap();
                m.floating = true;
                m.rect = requested_geometry;
            }
            self.initialize_window(&surface);
            if x.is_fullscreen() {
                self.set_fullscreen(&w, true);
            }
        }
    }
    fn remove_x11(&mut self, w: &Window) {
        let pointer = self.seat.get_pointer().unwrap();
        // XWM clears the X11/wl_surface association before this callback. Also
        // recognize a now-orphaned target; comparing only with w would miss it.
        let removed_or_orphaned = |s: WlSurface| {
            crate::wm::has_surface(w, &s)
                || !self.windows.iter().any(|m| {
                    let mut owns = false;
                    m.window
                        .with_surfaces(|candidate, _| owns |= candidate == &s);
                    owns
                })
        };
        let owns_pointer = pointer.current_focus().is_some_and(removed_or_orphaned)
            || pointer
                .grab_start_data()
                .and_then(|g| g.focus)
                .is_some_and(|(s, _)| removed_or_orphaned(s));
        let time = self.start_time.elapsed().as_millis() as u32;
        if owns_pointer {
            self.suspend_pointer();
            // Xwayland may keep the wl_surface alive after its X11 window unmaps.
            // End that window's implicit grab and balance the delivered presses.
            for button in pointer.pressed_buttons() {
                pointer.button(
                    self,
                    &smithay::input::pointer::ButtonEvent {
                        button,
                        state: smithay::backend::input::ButtonState::Released,
                        serial: smithay::utils::SERIAL_COUNTER.next_serial(),
                        time,
                    },
                );
                self.suppressed_buttons.insert(button);
            }
            if pointer.is_grabbed() {
                pointer.unset_grab(self, smithay::utils::SERIAL_COUNTER.next_serial(), time);
            }
            pointer.motion(
                self,
                None,
                &smithay::input::pointer::MotionEvent {
                    location: pointer.current_location(),
                    serial: smithay::utils::SERIAL_COUNTER.next_serial(),
                    time,
                },
            );
            pointer.frame(self);
        }
        self.space.unmap_elem(w);
        self.windows.retain(|m| &m.window != w);
        self.scratchpad_cleanup();
        self.relayout();
        if self.focused.as_ref() == Some(w) {
            let next = self.space.elements().last().cloned();
            self.focus(next);
        }
        if let Some(x11) = self.focused.as_ref().and_then(|w| w.x11_surface()) {
            x11.restore_input_focus_if_unset();
        }
        if owns_pointer {
            self.pointer_motion(pointer.current_location(), time);
        }
    }
}
impl XWaylandShellHandler for Ember {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.xwayland_shell
    }
    fn surface_associated(&mut self, _: XwmId, _: WlSurface, window: X11Surface) {
        self.map_x11(window);
    }
}
smithay::delegate_xwayland_shell!(Ember);
impl XwmHandler for Ember {
    fn xwm_state(&mut self, _: XwmId) -> &mut X11Wm {
        self.xwayland.wm.as_mut().expect("active XWM")
    }
    fn new_window(&mut self, _: XwmId, _: X11Surface) {}
    fn new_override_redirect_window(&mut self, _: XwmId, _: X11Surface) {}
    fn map_window_request(&mut self, _: XwmId, window: X11Surface) {
        if window.set_mapped(true).is_ok() {
            self.map_x11(window);
        }
    }
    fn map_window_notify(&mut self, _: XwmId, _: X11Surface) {
        // MapNotify adds the client to XWM's internal stacking list after the
        // map request callback, so synchronize again once it is registered.
        self.sync_x11_stacking();
    }
    fn mapped_override_redirect_window(&mut self, _: XwmId, window: X11Surface) {
        self.map_x11(window);
    }
    fn unmapped_window(&mut self, _: XwmId, window: X11Surface) {
        let w = self
            .windows
            .iter()
            .find(|m| m.window.x11_surface() == Some(&window))
            .map(|m| m.window.clone());
        if let Some(w) = w {
            self.remove_x11(&w);
        }
    }
    fn destroyed_window(&mut self, xwm: XwmId, window: X11Surface) {
        self.unmapped_window(xwm, window);
    }
    fn configure_request(
        &mut self,
        _: XwmId,
        window: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        w: Option<u32>,
        h: Option<u32>,
        _: Option<Reorder>,
    ) {
        if let Some(m) = self
            .windows
            .iter_mut()
            .find(|m| m.window.x11_surface() == Some(&window))
        {
            if m.floating && !m.fullscreen {
                if let Some(x) = x {
                    m.rect.loc.x = x;
                }
                if let Some(y) = y {
                    m.rect.loc.y = y;
                }
                if let Some(w) = w {
                    m.rect.size.w = (w as i32).clamp(64, 8192);
                }
                if let Some(h) = h {
                    m.rect.size.h = (h as i32).clamp(48, 8192);
                }
            }
            let _ = window.configure(m.rect);
        } else {
            let _ = window.configure(None);
        }
        self.relayout();
    }
    fn configure_notify(
        &mut self,
        _: XwmId,
        window: X11Surface,
        geometry: Rectangle<i32, Logical>,
        _: Option<u32>,
    ) {
        if window.is_override_redirect() {
            if let Some(m) = self
                .windows
                .iter_mut()
                .find(|m| m.window.x11_surface() == Some(&window))
            {
                m.rect = geometry;
                self.relayout();
            }
        }
    }
    fn property_notify(&mut self, _: XwmId, window: X11Surface, _: WmWindowProperty) {
        if let Some(s) = window.wl_surface() {
            self.apply_window_rules(&s);
        }
        self.dirty = true;
    }
    fn fullscreen_request(&mut self, _: XwmId, window: X11Surface) {
        if let Some(w) = self
            .windows
            .iter()
            .find(|m| m.window.x11_surface() == Some(&window))
            .map(|m| m.window.clone())
        {
            self.set_fullscreen(&w, true);
        }
    }
    fn unfullscreen_request(&mut self, _: XwmId, window: X11Surface) {
        if let Some(w) = self
            .windows
            .iter()
            .find(|m| m.window.x11_surface() == Some(&window))
            .map(|m| m.window.clone())
        {
            self.set_fullscreen(&w, false);
        }
    }
    fn activation_request(&mut self, _: XwmId, surface: X11Surface) {
        if surface.is_override_redirect() {
            return;
        }
        let Some(window) = self
            .windows
            .iter()
            .find(|w| w.window.x11_surface() == Some(&surface))
            .map(|w| w.window.clone())
        else {
            return;
        };
        let Some(managed) = self.windows.iter().find(|w| w.window == window) else {
            return;
        };
        let same_app = self.focused.as_ref().is_some_and(|focused| {
            crate::wm::metadata(focused).0 == crate::wm::metadata(&window).0
        });
        if managed.workspace != self.workspace && !same_app {
            return;
        }
        if managed.workspace != self.workspace {
            self.workspace = managed.workspace;
        }
        if self
            .windows
            .iter()
            .any(|w| w.workspace == self.workspace && w.fullscreen && w.window != window)
        {
            let peers: Vec<_> = self
                .windows
                .iter()
                .filter(|w| w.workspace == self.workspace && w.fullscreen && w.window != window)
                .map(|w| w.window.clone())
                .collect();
            for peer in peers {
                self.set_fullscreen(&peer, false);
            }
        }
        self.relayout();
        self.focus(Some(window));
    }
    fn maximize_request(&mut self, xwm: XwmId, window: X11Surface) {
        self.fullscreen_request(xwm, window);
    }
    fn unmaximize_request(&mut self, xwm: XwmId, window: X11Surface) {
        self.unfullscreen_request(xwm, window);
    }
    fn resize_request(&mut self, _: XwmId, _: X11Surface, _: u32, _: ResizeEdge) {}
    fn move_request(&mut self, _: XwmId, _: X11Surface, _: u32) {}
    fn allow_selection_access(&mut self, _: XwmId, target: SelectionTarget) -> bool {
        self.session_active && matches!(target, SelectionTarget::Clipboard)
    }
    fn send_selection(&mut self, _: XwmId, _: SelectionTarget, mime: String, fd: OwnedFd) {
        let own = current_data_device_selection_userdata(&self.seat).map(|d| d.clone());
        if let Some(crate::clipboard::SelectionData::Image(png)) = own {
            if mime == "image/png" {
                self.clipboard.send(fd, png);
            }
        } else {
            let _ = request_data_device_client_selection(&self.seat, mime, fd);
        }
    }
    fn new_selection(&mut self, xwm: XwmId, target: SelectionTarget, mimes: Vec<String>) {
        if matches!(target, SelectionTarget::Clipboard) {
            self.clipboard.clear();
            self.xwayland.offered = None;
            set_data_device_selection(
                &self.display_handle,
                &self.seat,
                mimes,
                crate::clipboard::SelectionData::X11(xwm),
            );
        }
    }
    fn cleared_selection(&mut self, xwm: XwmId, target: SelectionTarget) {
        if matches!(target, SelectionTarget::Clipboard) {
            let ours = current_data_device_selection_userdata(&self.seat)
                .is_some_and(|d| matches!(*d,crate::clipboard::SelectionData::X11(id)if id==xwm));
            if ours {
                clear_data_device_selection(&self.display_handle, &self.seat);
            }
        }
    }
    fn disconnected(&mut self, _: XwmId) {
        self.xwayland.restart = true;
    }
}
// The Wayland dispatch state is Ember; XWM callbacks run in the outer event loop.
impl XWaylandShellHandler for CalloopData {
    fn xwayland_shell_state(&mut self) -> &mut XWaylandShellState {
        &mut self.state.xwayland_shell
    }
    fn surface_associated(&mut self, id: XwmId, s: WlSurface, w: X11Surface) {
        self.state.surface_associated(id, s, w);
    }
}
impl XwmHandler for CalloopData {
    fn xwm_state(&mut self, id: XwmId) -> &mut X11Wm {
        self.state.xwm_state(id)
    }
    fn new_window(&mut self, id: XwmId, w: X11Surface) {
        self.state.new_window(id, w)
    }
    fn new_override_redirect_window(&mut self, id: XwmId, w: X11Surface) {
        self.state.new_override_redirect_window(id, w)
    }
    fn map_window_request(&mut self, id: XwmId, w: X11Surface) {
        self.state.map_window_request(id, w)
    }
    fn map_window_notify(&mut self, id: XwmId, w: X11Surface) {
        self.state.map_window_notify(id, w)
    }
    fn mapped_override_redirect_window(&mut self, id: XwmId, w: X11Surface) {
        self.state.mapped_override_redirect_window(id, w)
    }
    fn unmapped_window(&mut self, id: XwmId, w: X11Surface) {
        self.state.unmapped_window(id, w)
    }
    fn destroyed_window(&mut self, id: XwmId, w: X11Surface) {
        self.state.destroyed_window(id, w)
    }
    fn configure_request(
        &mut self,
        id: XwmId,
        w: X11Surface,
        x: Option<i32>,
        y: Option<i32>,
        width: Option<u32>,
        height: Option<u32>,
        r: Option<Reorder>,
    ) {
        self.state.configure_request(id, w, x, y, width, height, r)
    }
    fn configure_notify(
        &mut self,
        id: XwmId,
        w: X11Surface,
        g: Rectangle<i32, Logical>,
        a: Option<u32>,
    ) {
        self.state.configure_notify(id, w, g, a)
    }
    fn property_notify(&mut self, id: XwmId, w: X11Surface, p: WmWindowProperty) {
        self.state.property_notify(id, w, p)
    }
    fn fullscreen_request(&mut self, id: XwmId, w: X11Surface) {
        self.state.fullscreen_request(id, w)
    }
    fn unfullscreen_request(&mut self, id: XwmId, w: X11Surface) {
        self.state.unfullscreen_request(id, w)
    }
    fn activation_request(&mut self, id: XwmId, w: X11Surface) {
        self.state.activation_request(id, w);
    }
    fn maximize_request(&mut self, id: XwmId, w: X11Surface) {
        self.state.maximize_request(id, w)
    }
    fn unmaximize_request(&mut self, id: XwmId, w: X11Surface) {
        self.state.unmaximize_request(id, w)
    }
    fn resize_request(&mut self, id: XwmId, w: X11Surface, b: u32, e: ResizeEdge) {
        self.state.resize_request(id, w, b, e)
    }
    fn move_request(&mut self, id: XwmId, w: X11Surface, b: u32) {
        self.state.move_request(id, w, b)
    }
    fn allow_selection_access(&mut self, id: XwmId, s: SelectionTarget) -> bool {
        self.state.allow_selection_access(id, s)
    }
    fn send_selection(&mut self, id: XwmId, s: SelectionTarget, m: String, fd: OwnedFd) {
        XwmHandler::send_selection(&mut self.state, id, s, m, fd)
    }
    fn new_selection(&mut self, id: XwmId, s: SelectionTarget, m: Vec<String>) {
        XwmHandler::new_selection(&mut self.state, id, s, m)
    }
    fn cleared_selection(&mut self, id: XwmId, s: SelectionTarget) {
        self.state.cleared_selection(id, s)
    }
    fn disconnected(&mut self, id: XwmId) {
        self.state.disconnected(id)
    }
}

fn ensure_socket_directory() -> std::io::Result<()> {
    use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
    let path = std::path::Path::new("/tmp/.X11-unix");
    match std::fs::DirBuilder::new().mode(0o1777).create(path) {
        Ok(()) => std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o1777)),
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
            if std::fs::symlink_metadata(path)?.file_type().is_dir() {
                Ok(())
            } else {
                Err(std::io::Error::other("/tmp/.X11-unix is not a directory"))
            }
        }
        Err(e) => Err(e),
    }
}

fn write_authority(path: &std::path::Path, display: u32) -> std::io::Result<()> {
    use std::{
        io::{Read, Write},
        os::unix::fs::OpenOptionsExt,
    };
    let mut cookie = [0u8; 16];
    std::fs::File::open("/dev/urandom")?.read_exact(&mut cookie)?;
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&65535u16.to_be_bytes());
    for field in [
        b"".as_slice(),
        display.to_string().as_bytes(),
        b"MIT-MAGIC-COOKIE-1",
        &cookie,
    ] {
        bytes.extend_from_slice(&(field.len() as u16).to_be_bytes());
        bytes.extend_from_slice(field);
    }
    let tmp = path.with_extension("auth.new");
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(&tmp)?;
    file.write_all(&bytes)?;
    drop(file);
    std::fs::rename(tmp, path)
}
impl Drop for Runtime {
    fn drop(&mut self) {
        if let Some(p) = &self.auth {
            let _ = std::fs::remove_file(p);
        }
        if let Some(p) = &self.pid_file {
            let _ = std::fs::remove_file(p);
        }
    }
}
