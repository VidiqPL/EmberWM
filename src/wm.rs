use crate::{
    config::Config,
    layout::{self, Rect},
    Ember,
};
use smithay::{
    desktop::Window,
    input::keyboard::XkbConfig,
    reexports::wayland_protocols::xdg::shell::server::xdg_toplevel,
    utils::{IsAlive, Logical, Point, Rectangle, Size, SERIAL_COUNTER},
};
use std::{
    process::Command,
    time::{Duration, Instant},
};

/// Shared metadata/surface access for native Wayland and Xwayland windows.
pub fn surface(
    window: &Window,
) -> Option<smithay::reexports::wayland_server::protocol::wl_surface::WlSurface> {
    use smithay::wayland::seat::WaylandFocus;
    window.wl_surface().map(|s| s.into_owned())
}
pub fn has_surface(
    window: &Window,
    candidate: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
) -> bool {
    surface(window).as_ref() == Some(candidate)
}
pub fn metadata(window: &Window) -> (String, String) {
    if let Some(x) = window.x11_surface() {
        return (x.class(), x.title());
    }
    let Some(surface) = surface(window) else {
        return Default::default();
    };
    smithay::wayland::compositor::with_states(&surface, |states| {
        states
            .data_map
            .get::<smithay::wayland::shell::xdg::XdgToplevelSurfaceData>()
            .and_then(|d| d.lock().ok())
            .map(|d| {
                (
                    d.app_id.clone().unwrap_or_default(),
                    d.title.clone().unwrap_or_default(),
                )
            })
            .unwrap_or_default()
    })
}
pub fn unmanaged(window: &Window) -> bool {
    window
        .x11_surface()
        .is_some_and(|x| x.is_override_redirect())
}

/// LinusWare uses an unmanaged drawing surface plus a separate interactive X11 menu.
pub fn game_overlay(window: &Window) -> bool {
    window.x11_surface().is_some_and(|x| {
        [x.class(), x.title()].iter().any(|s| {
            s.eq_ignore_ascii_case("linusware") || s.eq_ignore_ascii_case("linusware_overlay")
        })
    })
}

pub struct ManagedWindow {
    pub foreign: Option<smithay::wayland::foreign_toplevel_list::ForeignToplevelHandle>,
    pub border_ids: [smithay::backend::renderer::element::Id; 4],
    pub window: Window,
    pub workspace: usize,
    pub floating: bool,
    pub scratchpad: Option<crate::scratchpad::Placement>,
    pub fullscreen: bool,
    pub rect: Rect,
    pub restore_rect: Option<Rect>,
    pub initialized: bool,
    pub drawn: bool,
    pub applied_rules: u64,
}
pub struct DrawState {
    pub screenshot: bool,
    pub start: Option<Point<f64, Logical>>,
    pub current: Point<f64, Logical>,
}
pub struct DragState {
    pub window: Window,
    pub origin: Point<f64, Logical>,
    pub rect: Rect,
    pub resize: bool,
    pub button: u32,
}

impl Ember {
    pub fn output_size(&self) -> Size<i32, Logical> {
        self.space
            .outputs()
            .next()
            .and_then(|o| self.space.output_geometry(o))
            .map(|r| r.size)
            .unwrap_or((1280, 800).into())
    }
    pub fn work_area(&self) -> Rect {
        crate::panels::work_area(&self.config.panels, self.output_size())
    }
    pub fn manage(&mut self, window: Window) {
        let work_area = self.work_area();
        let area = work_area.size;
        let w = (area.w * 3 / 5).max(160);
        let h = (area.h * 3 / 5).max(100);
        let offset = (self.windows.len() as i32 % 5) * 24;
        self.windows.push(ManagedWindow {
            foreign: None,
            border_ids: std::array::from_fn(|_| smithay::backend::renderer::element::Id::new()),
            window: window.clone(),
            workspace: self.workspace,
            floating: self.config.layout.default_floating,
            scratchpad: None,
            fullscreen: false,
            rect: Rectangle::new(
                (
                    (area.w - w) / 2 + offset + work_area.loc.x,
                    (area.h - h) / 2 + offset + work_area.loc.y,
                )
                    .into(),
                (w, h).into(),
            ),
            restore_rect: None,
            initialized: false,
            drawn: false,
            applied_rules: 0,
        });
        self.relayout();
    }
    /// Complete initial placement after the client has supplied its metadata.
    pub fn initialize_window(
        &mut self,
        surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    ) {
        let Some(index) = self
            .windows
            .iter()
            .position(|m| has_surface(&m.window, surface) && !m.initialized)
        else {
            return;
        };
        self.windows[index].initialized = true;
        self.apply_draw_identity(surface);
        self.apply_window_rules(surface);
        let window = self.windows[index].window.clone();
        let workspace = self.windows[index].workspace;
        if self.windows[index].fullscreen {
            self.set_fullscreen(&window, true);
        } else if workspace == self.workspace && !game_overlay(&window) {
            let peers: Vec<_> = self
                .windows
                .iter()
                .filter(|m| {
                    m.workspace == workspace
                        && m.scratchpad.is_none()
                        && m.fullscreen
                        && m.window != window
                })
                .map(|m| m.window.clone())
                .collect();
            for peer in peers {
                self.set_fullscreen(&peer, false);
            }
        }
        self.relayout();
        if workspace == self.workspace {
            self.focus(Some(window));
        }
    }
    /// A single fullscreen policy for keys, placement rules and client requests.
    pub fn set_fullscreen(&mut self, window: &Window, enabled: bool) {
        let Some(index) = self.windows.iter().position(|m| &m.window == window) else {
            return;
        };
        let workspace = self.windows[index].workspace;
        let initialized = self.windows[index].initialized;
        let scratchpad = self.windows[index].scratchpad.is_some();
        if enabled && initialized {
            for m in &mut self.windows {
                if m.scratchpad.is_some() == scratchpad
                    && (scratchpad || m.workspace == workspace)
                    && &m.window != window
                    && m.fullscreen
                {
                    m.fullscreen = false;
                    if let Some(rect) = m.restore_rect.take() {
                        m.rect = rect;
                    }
                }
            }
        }
        let m = &mut self.windows[index];
        if enabled && !m.fullscreen {
            m.restore_rect = Some(m.rect);
        }
        if !enabled && m.fullscreen {
            if let Some(rect) = m.restore_rect.take() {
                m.rect = rect;
            }
        }
        m.fullscreen = enabled;
        self.relayout();
        if enabled && initialized && self.space.element_location(window).is_some() {
            self.focus(Some(window.clone()));
        }
    }
    pub fn apply_window_rules(
        &mut self,
        surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    ) {
        if !self.config.window_rules.iter().any(|r| r.enabled) {
            return;
        }
        let Some(index) = self
            .windows
            .iter()
            .position(|m| m.initialized && !m.drawn && has_surface(&m.window, surface))
        else {
            return;
        };
        let (app_id, title) = metadata(&self.windows[index].window);
        let bits = crate::rules::matching(&self.config.window_rules, Some(&app_id), Some(&title));
        if bits & !self.windows[index].applied_rules == 0 {
            return;
        }
        self.windows[index].applied_rules |= bits;
        let p = crate::rules::Placement::from_rules(&self.config.window_rules, bits);
        let area = self.work_area();
        let border = self.config.theme.border_width;
        let m = &mut self.windows[index];
        let window = m.window.clone();
        let was_focused = self.focused.as_ref() == Some(&window);
        if let Some(n) = p.workspace {
            m.workspace = n - 1;
        }
        if let Some(floating) = p.floating {
            m.floating = floating || m.scratchpad.is_some();
        }
        let geometry = p.width.is_some()
            || p.height.is_some()
            || p.x.is_some()
            || p.y.is_some()
            || p.center == Some(true);
        if geometry {
            let mut rect = m.restore_rect.unwrap_or(m.rect);
            rect.size.w = p
                .width
                .unwrap_or(rect.size.w)
                .clamp(64, (area.size.w - 2 * border).max(64));
            rect.size.h = p
                .height
                .unwrap_or(rect.size.h)
                .clamp(64, (area.size.h - 2 * border).max(64));
            if p.center == Some(true) {
                rect.loc = area.loc
                    + Point::from((
                        (area.size.w - rect.size.w) / 2,
                        (area.size.h - rect.size.h) / 2,
                    ));
            }
            if let Some(x) = p.x {
                rect.loc.x = x;
            }
            if let Some(y) = p.y {
                rect.loc.y = y;
            }
            rect.loc.x = rect.loc.x.clamp(
                area.loc.x + border,
                (area.loc.x + area.size.w - rect.size.w - border).max(area.loc.x + border),
            );
            rect.loc.y = rect.loc.y.clamp(
                area.loc.y + border,
                (area.loc.y + area.size.h - rect.size.h - border).max(area.loc.y + border),
            );
            if m.fullscreen {
                m.restore_rect = Some(rect);
            } else {
                m.rect = rect;
            }
        }
        let workspace = m.workspace;
        if p.follow == Some(true) {
            self.workspace = workspace;
        }
        if let Some(enabled) = p.fullscreen {
            self.set_fullscreen(&window, enabled);
        } else if self.windows[index].fullscreen {
            self.set_fullscreen(&window, true);
        }
        self.relayout();
        if workspace == self.workspace && (was_focused || p.follow == Some(true)) {
            self.focus(Some(window));
        } else if was_focused && workspace != self.workspace {
            let next = self.space.elements().last().cloned();
            self.focus(next);
        }
    }
    pub fn reapply_window_rules(&mut self) {
        let surfaces: Vec<_> = self
            .windows
            .iter_mut()
            .filter(|m| m.initialized)
            .filter_map(|m| {
                m.applied_rules = 0;
                surface(&m.window)
            })
            .collect();
        for surface in surfaces {
            self.apply_window_rules(&surface);
        }
    }
    pub fn focus(&mut self, window: Option<Window>) {
        let window = window.filter(|w| self.space.element_location(w).is_some());
        // Override-redirect overlays/menus must not replace the game's keyboard target.
        let window = if window.as_ref().is_some_and(unmanaged) {
            self.focused
                .clone()
                .filter(|w| self.space.element_location(w).is_some() && !unmanaged(w))
                .or_else(|| self.space.elements().rev().find(|w| !unmanaged(w)).cloned())
        } else {
            window
        };
        if self.focused != window {
            self.suspend_pointer();
        }
        if self.focused != window {
            if let Some(old) = self.focused.as_ref().and_then(|w| w.x11_surface()) {
                old.set_input_focus(false);
            }
        }
        if self.focused != window {
            if let Some(x11) = window.as_ref().and_then(|w| w.x11_surface()) {
                x11.set_input_focus(true);
            }
        }
        if let Some(wm) = &self.xwayland.wm {
            if let Err(error) = wm.set_active_window(window.as_ref().and_then(|w| w.x11_surface()))
            {
                tracing::warn!(%error,"could not publish active X11 window");
            }
        }
        self.focused = window.clone();
        for m in &self.windows {
            if unmanaged(&m.window) {
                continue;
            }
            m.window.set_activated(window.as_ref() == Some(&m.window));
            if let Some(t) = m.window.toplevel() {
                if m.initialized {
                    t.send_pending_configure();
                }
            }
        }
        if let Some(w) = &window {
            self.space.raise_element(w, false);
            if self
                .windows
                .iter()
                .any(|m| &m.window == w && m.scratchpad.is_some())
            {
                self.scratchpad.last_focused = Some(w.clone());
            }
        }
        self.raise_scratchpad();
        let surface = window.as_ref().and_then(surface);
        if let Some(keyboard) = self.seat.get_keyboard() {
            keyboard.set_focus(self, surface, SERIAL_COUNTER.next_serial());
        }
        self.sync_x11_stacking();
        self.refresh_pointer_focus(self.start_time.elapsed().as_millis() as u32);
        self.dirty = true;
    }
    pub fn relayout(&mut self) {
        let size = self.output_size();
        let fullscreen = self
            .windows
            .iter()
            .find(|m| {
                m.initialized
                    && m.scratchpad.is_none()
                    && m.workspace == self.workspace
                    && m.fullscreen
            })
            .map(|m| m.window.clone());
        let count = self
            .windows
            .iter()
            .filter(|m| {
                m.initialized
                    && m.scratchpad.is_none()
                    && m.workspace == self.workspace
                    && !m.floating
                    && !m.fullscreen
            })
            .count();
        let work_area = self.work_area();
        let mut tiles = layout::tiled(
            work_area.size,
            count,
            self.config.layout.gap,
            self.config.theme.border_width,
            self.config.layout.master_ratio,
        )
        .into_iter()
        .map(|mut rect| {
            rect.loc += work_area.loc;
            rect
        });
        for m in &mut self.windows {
            let visible = if m.scratchpad.is_some() {
                self.scratchpad.visible
            } else {
                m.workspace == self.workspace
                    && (unmanaged(&m.window)
                        || game_overlay(&m.window)
                        || fullscreen.as_ref().is_none_or(|f| f == &m.window))
            };
            if m.fullscreen {
                m.rect = Rectangle::from_size(size);
            } else if m.initialized
                && m.scratchpad.is_none()
                && m.workspace == self.workspace
                && !m.floating
            {
                m.rect = tiles.next().unwrap();
            } else if m.floating {
                m.rect.loc.x = m.rect.loc.x.clamp(0, (size.w - 64).max(0));
                m.rect.loc.y = m.rect.loc.y.clamp(0, (size.h - 48).max(0));
            }
            if let Some(t) = m.window.toplevel() {
                t.with_pending_state(|s| {
                    s.size = Some(m.rect.size);
                    if m.fullscreen {
                        s.states.set(xdg_toplevel::State::Fullscreen);
                    } else {
                        s.states.unset(xdg_toplevel::State::Fullscreen);
                    }
                    for edge in [
                        xdg_toplevel::State::TiledTop,
                        xdg_toplevel::State::TiledBottom,
                        xdg_toplevel::State::TiledLeft,
                        xdg_toplevel::State::TiledRight,
                    ] {
                        if !m.floating && !m.fullscreen {
                            s.states.set(edge);
                        } else {
                            s.states.unset(edge);
                        }
                    }
                });
                if m.initialized {
                    t.send_pending_configure();
                }
            }
            if let Some(x) = m
                .window
                .x11_surface()
                .filter(|x| m.initialized && !x.is_override_redirect())
            {
                if x.geometry() != m.rect {
                    let _ = x.configure(m.rect);
                }
                if x.is_fullscreen() != m.fullscreen {
                    let _ = x.set_fullscreen(m.fullscreen);
                }
            }
            if visible {
                self.space.map_element(m.window.clone(), m.rect.loc, false);
            } else {
                self.space.unmap_elem(&m.window);
            }
        }
        // Floating windows stay above tiled windows.
        for m in &self.windows {
            if m.scratchpad.is_none()
                && m.workspace == self.workspace
                && m.floating
                && (fullscreen.is_none() || m.fullscreen)
            {
                self.space.raise_element(&m.window, false);
            }
        }
        self.raise_scratchpad();
        self.sync_x11_stacking();
        self.dirty = true;
    }
    pub fn maintenance(&mut self) {
        self.sync_pointer_capture();
        self.apply_pointer_hint();
        let before = self.windows.len();
        self.windows.retain(|m| m.window.alive());
        if before != self.windows.len() {
            self.scratchpad_cleanup();
            self.relayout();
            if self.focused.as_ref().is_some_and(|w| !w.alive()) {
                let next = self.space.elements().last().cloned();
                self.focus(next);
            }
        }
        self.children
            .retain_mut(|c| matches!(c.try_wait(), Ok(None)));
        self.pending_draws
            .retain(|_, (_, _, t)| t.elapsed() < Duration::from_secs(15));
        crate::image_capture::refresh_toplevels(self);
        crate::image_capture::refresh(self);
        self.space.refresh();
        self.popups.cleanup();
    }
    pub fn apply_draw_identity(
        &mut self,
        surface: &smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
    ) {
        if self.pending_draws.is_empty() {
            return;
        }
        use smithay::wayland::{compositor::with_states, shell::xdg::XdgToplevelSurfaceData};
        let id = with_states(surface, |s| {
            s.data_map
                .get::<XdgToplevelSurfaceData>()
                .and_then(|d| d.lock().ok())
                .and_then(|d| d.app_id.clone())
        });
        if let Some((rect, workspace, _)) = id.and_then(|id| self.pending_draws.remove(&id)) {
            if let Some(m) = self
                .windows
                .iter_mut()
                .find(|m| has_surface(&m.window, surface))
            {
                m.floating = true;
                m.drawn = true;
                m.fullscreen = false;
                m.restore_rect = None;
                m.rect = rect;
                m.workspace = workspace;
                let window = m.window.clone();
                if workspace == self.workspace {
                    let peers: Vec<_> = self
                        .windows
                        .iter()
                        .filter(|m| m.workspace == workspace && m.fullscreen)
                        .map(|m| m.window.clone())
                        .collect();
                    for peer in peers {
                        self.set_fullscreen(&peer, false);
                    }
                }
                self.relayout();
                if workspace == self.workspace {
                    self.focus(Some(window));
                }
            }
        }
    }
    pub fn spawn_terminal(&mut self, rect: Option<Rect>) -> Result<(), String> {
        if rect.is_some() && !self.config.terminal.iter().any(|s| s.contains("{app_id}")) {
            return Err("draw terminals need {app_id} in terminal arguments (see config)".into());
        }
        let id = format!("ember-terminal-{}", u32::from(SERIAL_COUNTER.next_serial()));
        let args: Vec<_> = self
            .config
            .terminal
            .iter()
            .map(|a| {
                a.replace("{app_id}", &id)
                    .replace("{background}", &self.config.theme.background)
                    .replace("{foreground}", &self.config.panels.foreground)
                    .replace("{accent}", &self.config.panels.accent)
            })
            .collect();
        let child = Command::new(&args[0])
            .args(&args[1..])
            .env("WAYLAND_DISPLAY", &self.socket_name)
            .env("XDG_CURRENT_DESKTOP", "Ember")
            .env("EMBER_SOCKET", self.ipc_path())
            .env("DISPLAY", self.xwayland.display_name())
            .spawn()
            .map_err(|e| e.to_string())?;
        self.children.push(child);
        if let Some(rect) = rect {
            self.pending_draws
                .insert(id, (rect, self.workspace, Instant::now()));
        }
        Ok(())
    }
    pub fn arm_draw(&mut self) {
        self.drag = None;
        self.draw = Some(DrawState {
            screenshot: false,
            start: None,
            current: self.seat.get_pointer().unwrap().current_location(),
        });
        self.dirty = true;
    }
    pub fn complete_draw(&mut self) -> Result<(), String> {
        if let Some(draw) = self.draw.take() {
            self.dirty = true;
            if let Some(start) = draw.start {
                let mut rect = layout::selection(start, draw.current, self.output_size());
                if draw.screenshot {
                    if rect.size.w >= 2 && rect.size.h >= 2 {
                        self.request_region(rect)?;
                    }
                    return Ok(());
                }
                let b = self.config.theme.border_width;
                if rect.size.w < 80 + 2 * b || rect.size.h < 60 + 2 * b {
                    return Ok(());
                }
                rect.loc += (b, b).into();
                rect.size.w -= 2 * b;
                rect.size.h -= 2 * b;
                self.spawn_terminal(Some(rect))?;
            }
        }
        Ok(())
    }
    pub fn action(&mut self, action: &str) -> Result<(), String> {
        match action {
            "scratchpad-mark" => self.scratchpad_mark(),
            "scratchpad-toggle" => self.scratchpad_toggle(),
            "rice-cycle" => self.cycle_rice()?,
            _ if action.starts_with("rice ") => self.select_rice(action[5..].trim())?,
            "theme-cycle" => {
                let names = crate::themes::names(&self.config_path);
                let i = names
                    .iter()
                    .position(|n| n == &self.config.theme.preset)
                    .map_or(0, |i| (i + 1) % names.len());
                self.select_theme(&names[i])?;
            }
            _ if action.starts_with("theme ") => self.select_theme(action[6..].trim())?,
            "pointer-release" => self.release_pointer(),
            "terminal" => self.spawn_terminal(None)?,
            "panels-toggle" => {
                self.config.panels.enabled = !self.config.panels.enabled;
                self.relayout();
            }
            "volume-up" | "volume-down" | "mute" | "mic-mute" => {
                self.volume.request(action);
            }
            "draw" => self.arm_draw(),
            "screenshot-region" => {
                self.launcher.close();
                self.tray.close();
                self.ui.close();
                self.wifi.close();
                self.arm_draw();
                self.draw.as_mut().unwrap().screenshot = true;
            }
            "quit" => {
                self.running = false;
                self.loop_signal.stop();
            }
            "close" => {
                if let Some(w) = self.focused.as_ref() {
                    if let Some(t) = w.toplevel() {
                        t.send_close();
                    }
                    if let Some(x) = w.x11_surface() {
                        let _ = x.close();
                    }
                }
            }
            "reload" => self.reload()?,
            "rules-apply" => self.reapply_window_rules(),
            "shortcuts-toggle" => {
                if self.config.shortcuts.enabled {
                    self.tray.close();
                    self.launcher.close();
                    self.wifi.close();
                    self.ui.toggle();
                }
            }
            "tray-toggle" => {
                if !self.config.tray.enabled {
                    return Ok(());
                }
                self.launcher.close();
                self.wifi.close();
                self.ui.close();
                self.draw = None;
                self.drag = None;
                self.tray.toggle();
            }
            "wifi-toggle" => {
                if self.config.wifi.enabled {
                    self.launcher.close();
                    self.tray.close();
                    self.ui.close();
                    self.draw = None;
                    self.drag = None;
                    self.wifi.toggle();
                }
            }
            "launcher-toggle" => {
                if self.config.launcher.enabled {
                    self.tray.close();
                    self.ui.close();
                    self.wifi.close();
                    self.draw = None;
                    self.drag = None;
                    self.launcher.toggle();
                }
            }
            "notification-dismiss" => {
                if let Some(id) = self.notifications.entries.last().map(|e| e.id) {
                    self.notifications.close(id, 2);
                }
            }
            "notifications-clear" => self.notifications.clear(2),
            "notifications-toggle-dnd" => {
                self.config.notifications.do_not_disturb =
                    !self.config.notifications.do_not_disturb;
            }
            "fullscreen" => {
                if let Some(window) = self.focused.clone() {
                    if let Some(m) = self.windows.iter().find(|m| m.window == window) {
                        let enabled = !m.fullscreen;
                        self.set_fullscreen(&window, enabled);
                    }
                }
            }
            "float" => {
                if let Some(m) = self
                    .windows
                    .iter_mut()
                    .find(|m| Some(&m.window) == self.focused.as_ref())
                {
                    if let Some(rect) = m.restore_rect.take() {
                        m.rect = rect;
                    }
                    m.floating = m.scratchpad.is_some() || !m.floating;
                    m.fullscreen = false;
                }
                self.relayout();
            }
            "ratio-down" | "ratio-up" => {
                self.config.layout.master_ratio = (self.config.layout.master_ratio
                    + if action == "ratio-up" { 0.05 } else { -0.05 })
                .clamp(0.2, 0.8);
                self.relayout();
            }
            "focus-next" | "focus-prev" => {
                let visible: Vec<_> = self.space.elements().cloned().collect();
                if !visible.is_empty() {
                    // Use stable management order, not stacking order (raising must not break cycling).
                    let visible: Vec<_> = self
                        .windows
                        .iter()
                        .filter(|m| visible.contains(&m.window) && !unmanaged(&m.window))
                        .map(|m| m.window.clone())
                        .collect();
                    let i = visible
                        .iter()
                        .position(|w| Some(w) == self.focused.as_ref())
                        .unwrap_or(0);
                    let next = if action == "focus-next" {
                        (i + 1) % visible.len()
                    } else {
                        (i + visible.len() - 1) % visible.len()
                    };
                    self.focus(Some(visible[next].clone()));
                }
            }
            _ if action.starts_with("workspace ") || action.starts_with("send ") => {
                let (verb, n) = action.split_once(' ').unwrap();
                let n = n
                    .parse::<usize>()
                    .map_err(|_| "workspace must be a number")?;
                if n == 0 || n > self.config.layout.workspaces {
                    return Err("workspace out of range".into());
                }
                if verb == "send" {
                    if let Some(window) = self.focused.clone() {
                        self.scratchpad_release(&window, n - 1);
                        if let Some(m) = self.windows.iter_mut().find(|m| m.window == window) {
                            m.workspace = n - 1;
                        }
                    }
                } else {
                    self.workspace = n - 1;
                }
                if verb == "send" {
                    if let Some(window) = self.focused.clone() {
                        if self
                            .windows
                            .iter()
                            .any(|m| m.window == window && m.fullscreen)
                        {
                            self.set_fullscreen(&window, true);
                        }
                    }
                }
                self.draw = None;
                self.drag = None;
                self.relayout();
                let next = self.space.elements().last().cloned();
                self.focus(next);
            }
            _ if action.starts_with("exec ") => {
                let child = Command::new("/bin/sh")
                    .arg("-c")
                    .arg(&action[5..])
                    .env("WAYLAND_DISPLAY", &self.socket_name)
                    .env("EMBER_SOCKET", self.ipc_path())
                    .env("XDG_CURRENT_DESKTOP", "Ember")
                    .env("DISPLAY", self.xwayland.display_name())
                    .spawn()
                    .map_err(|e| e.to_string())?;
                self.children.push(child);
            }
            _ => return Err(format!("unknown action: {action}")),
        }
        self.sync_pointer_capture();
        self.dirty = true;
        Ok(())
    }
    pub fn reload(&mut self) -> Result<(), String> {
        let config = Config::load(&self.config_path)?;
        // Validate wallpaper before committing the new config.
        crate::render::validate_wallpaper(&config, &self.config_path)?;
        crate::panels::validate_resources(&config.panels, &self.config_path)?;
        crate::ui::validate_resources(&config, &self.config_path)?;
        let keyboard = self.seat.get_keyboard().unwrap();
        keyboard
            .set_xkb_config(
                self,
                XkbConfig {
                    layout: &config.keyboard.xkb_layout,
                    ..Default::default()
                },
            )
            .map_err(|e| format!("keyboard configuration: {e}"))?;
        keyboard.change_repeat_info(config.keyboard.repeat_rate, config.keyboard.repeat_delay);
        self.workspace = self.workspace.min(config.layout.workspaces - 1);
        for m in &mut self.windows {
            m.workspace = m.workspace.min(config.layout.workspaces - 1);
        }
        let rules_changed = self.config.window_rules != config.window_rules;
        self.config = config;
        if rules_changed {
            self.reapply_window_rules();
        }
        self.theme_generation += 1;
        self.relayout();
        let next = self
            .focused
            .clone()
            .filter(|w| self.space.element_location(w).is_some())
            .or_else(|| self.space.elements().last().cloned());
        self.focus(next);
        tracing::info!("configuration reloaded");
        Ok(())
    }
}
