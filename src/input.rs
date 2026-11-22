use smithay::{
    backend::input::{
        AbsolutePositionEvent, Axis, AxisSource, ButtonState, Event, InputBackend, InputEvent,
        KeyState, KeyboardKeyEvent, PointerAxisEvent, PointerButtonEvent, PointerMotionEvent,
    },
    input::{
        keyboard::FilterResult,
        pointer::{AxisFrame, ButtonEvent, MotionEvent, RelativeMotionEvent},
    },
    utils::SERIAL_COUNTER,
};

use crate::state::Ember;

impl Ember {
    pub fn process_input_event<I: InputBackend>(&mut self, event: InputEvent<I>) {
        match event {
            InputEvent::Keyboard { event, .. } => {
                let serial = SERIAL_COUNTER.next_serial();
                let time = Event::time_msec(&event);

                let code = event.key_code().raw();
                let action = self.seat.get_keyboard().unwrap().input::<String, _>(
                    self,
                    event.key_code(),
                    event.state(),
                    serial,
                    time,
                    |state, mods, key| {
                        if event.state() == KeyState::Released {
                            return if state.suppressed_keys.remove(&code) {
                                FilterResult::Intercept(String::new())
                            } else {
                                FilterResult::Forward
                            };
                        }
                        let sym = key
                            .raw_syms()
                            .first()
                            .copied()
                            .unwrap_or_else(|| key.modified_sym());
                        if state.backend_name == "native"
                            && mods.ctrl
                            && mods.alt
                            && (smithay::input::keyboard::keysyms::KEY_F1
                                ..=smithay::input::keyboard::keysyms::KEY_F12)
                                .contains(&sym.raw())
                        {
                            state.vt_request = Some(
                                (sym.raw() - smithay::input::keyboard::keysyms::KEY_F1 + 1) as i32,
                            );
                            state.suppressed_keys.insert(code);
                            return FilterResult::Intercept(String::new());
                        }
                        if state.screencast.prompt_open() {
                            if matches!(
                                key.modified_sym(),
                                smithay::input::keyboard::Keysym::Return
                                    | smithay::input::keyboard::Keysym::KP_Enter
                            ) {
                                state.screencast.confirm_prompt();
                            } else if matches!(
                                sym,
                                smithay::input::keyboard::Keysym::Tab
                                    | smithay::input::keyboard::Keysym::Left
                                    | smithay::input::keyboard::Keysym::Right
                            ) {
                                state.screencast.select_prompt();
                            } else if matches!(
                                sym,
                                smithay::input::keyboard::Keysym::Up
                                    | smithay::input::keyboard::Keysym::Down
                            ) {
                                state
                                    .screencast
                                    .select_source(sym == smithay::input::keyboard::Keysym::Down);
                            } else if sym == smithay::input::keyboard::Keysym::Escape {
                                state.screencast.decide(false);
                            }
                            state.dirty = true;
                            state.suppressed_keys.insert(code);
                            return FilterResult::Intercept(String::new());
                        }
                        if state.menu.open {
                            match state.config.action(mods, sym).as_deref() {
                                Some("menu-toggle") | Some("wifi-toggle") => state.toggle_menu(),
                                Some("tray-toggle") => {
                                    if state.menu.tab == crate::menu::Tab::Tray {
                                        state.close_menu();
                                    } else {
                                        state.select_menu(crate::menu::Tab::Tray);
                                    }
                                }
                                Some("power-toggle") => state.select_menu(crate::menu::Tab::Power),
                                _ => state.menu_key(sym, key.modified_sym(), mods),
                            }
                            state.dirty = true;
                            state.suppressed_keys.insert(code);
                            return FilterResult::Intercept(String::new());
                        }
                        if state.tray.open {
                            if state.config.action(mods, sym).as_deref() == Some("tray-toggle") {
                                state.tray.close();
                            } else {
                                state.tray.key(sym);
                            }
                            state.dirty = true;
                            state.suppressed_keys.insert(code);
                            return FilterResult::Intercept(String::new());
                        }
                        if state.launcher.open {
                            if state.config.action(mods, sym).as_deref() == Some("launcher-toggle")
                            {
                                state.launcher.close();
                            } else {
                                state.launcher.key(key.modified_sym(), mods);
                            }
                            state.dirty = true;
                            state.suppressed_keys.insert(code);
                            return FilterResult::Intercept(String::new());
                        }
                        if state.wifi.open {
                            if state.config.action(mods, sym).as_deref() == Some("wifi-toggle") {
                                state.wifi.close();
                            } else {
                                state.wifi.key(key.modified_sym(), mods);
                            }
                            state.dirty = true;
                            state.suppressed_keys.insert(code);
                            return FilterResult::Intercept(String::new());
                        }
                        if sym == smithay::input::keyboard::Keysym::Escape
                            && (state.draw.is_some() || state.drag.is_some() || state.ui.help_open)
                        {
                            state.ui.close();
                            state.draw = None;
                            state.drag = None;
                            state.dirty = true;
                            state.suppressed_keys.insert(code);
                            return FilterResult::Intercept(String::new());
                        }
                        if let Some(action) = state.config.action(mods, sym) {
                            state.suppressed_keys.insert(code);
                            FilterResult::Intercept(action)
                        } else if state.ui.help_open {
                            state.suppressed_keys.insert(code);
                            FilterResult::Intercept(String::new())
                        } else {
                            FilterResult::Forward
                        }
                    },
                );
                if let Some(action) = action.filter(|s| !s.is_empty()) {
                    if let Err(error) = self.action(&action) {
                        tracing::warn!(%error, "action failed");
                    }
                }
            }
            InputEvent::PointerMotion { event, .. } => {
                self.sync_pointer_capture();
                let pointer = self.seat.get_pointer().unwrap();
                if self.capture_allowed() {
                    pointer.relative_motion(
                        self,
                        self.surface_under(pointer.current_location()),
                        &RelativeMotionEvent {
                            delta: event.delta(),
                            delta_unaccel: event.delta_unaccel(),
                            utime: event.time(),
                        },
                    );
                    pointer.frame(self);
                }
                let size = self.output_size();
                let mut pos = self.seat.get_pointer().unwrap().current_location() + event.delta();
                pos.x = pos.x.clamp(0.0, (size.w - 1).max(0) as f64);
                pos.y = pos.y.clamp(0.0, (size.h - 1).max(0) as f64);
                self.pointer_motion(pos, event.time_msec());
            }
            InputEvent::PointerMotionAbsolute { event, .. } => {
                let output = self.space.outputs().next().unwrap();

                let output_geo = self.space.output_geometry(output).unwrap();

                let pos = event.position_transformed(output_geo.size) + output_geo.loc.to_f64();

                self.pointer_motion(pos, event.time_msec());
            }
            InputEvent::PointerButton { event, .. } => {
                self.pointer_button(event.button_code(), event.state(), event.time_msec());
            }

            InputEvent::PointerAxis { event, .. } => {
                let pos = self.seat.get_pointer().unwrap().current_location();
                if self.screencast.prompt_open() {
                    let amount = event
                        .amount(Axis::Vertical)
                        .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.));
                    if amount != 0. {
                        self.screencast.select_source(amount > 0.);
                        self.dirty = true;
                    }
                    return;
                }
                if self.menu.open
                    && (self.menu.tab == crate::menu::Tab::Power
                        || self.menu.body.is_none_or(|r| !r.to_f64().contains(pos)))
                {
                    if self.menu.tab == crate::menu::Tab::Power {
                        let amount = event
                            .amount(Axis::Vertical)
                            .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.));
                        if amount != 0. {
                            self.power.scroll(amount > 0.);
                            self.dirty = true;
                        }
                    }
                    return;
                }
                if self.tray.open {
                    let amount = event
                        .amount(Axis::Vertical)
                        .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.));
                    if amount != 0. {
                        self.tray.scroll(amount > 0.);
                        self.dirty = true;
                    }
                    return;
                }
                if self.launcher.open {
                    let amount = event
                        .amount(Axis::Vertical)
                        .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0));
                    if amount != 0.0 {
                        self.launcher.scroll(amount > 0.0);
                        self.dirty = true;
                    }
                    return;
                }
                if self.wifi.open {
                    let amount = event
                        .amount(Axis::Vertical)
                        .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0));
                    if amount != 0.0 {
                        self.wifi.scroll(amount > 0.0);
                        self.dirty = true;
                    }
                    return;
                }
                if self.ui.contains(pos)
                    && !self.seat.get_pointer().unwrap().is_grabbed()
                    && self.draw.is_none()
                    && self.drag.is_none()
                {
                    let amount = event
                        .amount(Axis::Vertical)
                        .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0));
                    if amount != 0.0 {
                        self.ui.scroll(amount > 0.0);
                        self.dirty = true;
                    }
                    return;
                }
                if !self.seat.get_pointer().unwrap().is_grabbed()
                    && self.drag.is_none()
                    && self.draw.is_none()
                    && self.panels.contains(pos)
                {
                    let amount = event
                        .amount(Axis::Vertical)
                        .unwrap_or_else(|| event.amount_v120(Axis::Vertical).unwrap_or(0.0));
                    if amount != 0.0 {
                        if let Some(action) = self.panels.scroll(
                            pos,
                            amount > 0.0,
                            self.workspace,
                            self.config.layout.workspaces,
                        ) {
                            if let Err(error) = self.action(&action) {
                                tracing::warn!(%error,"panel scroll failed");
                            }
                        }
                    }
                    return;
                }
                // Reconcile pointer focus before delivering a wheel event: a new
                // popup can appear without any physical mouse movement.
                self.refresh_pointer_focus(event.time_msec());
                let source = event.source();

                let horizontal_amount = event.amount(Axis::Horizontal).unwrap_or_else(|| {
                    event.amount_v120(Axis::Horizontal).unwrap_or(0.0) * 15.0 / 120.
                });
                let vertical_amount = event.amount(Axis::Vertical).unwrap_or_else(|| {
                    event.amount_v120(Axis::Vertical).unwrap_or(0.0) * 15.0 / 120.
                });
                let horizontal_amount_discrete = event.amount_v120(Axis::Horizontal);
                let vertical_amount_discrete = event.amount_v120(Axis::Vertical);

                let factor = self
                    .seat
                    .get_pointer()
                    .unwrap()
                    .current_focus()
                    .and_then(|s| {
                        self.windows
                            .iter()
                            .find(|w| crate::wm::has_surface(&w.window, &s))
                    })
                    .map_or(1.0, |w| {
                        let base = if w.window.is_x11() {
                            self.config.xwayland.scroll_factor
                        } else {
                            1.0
                        };
                        let (app, title) = crate::wm::metadata(&w.window);
                        self.config
                            .window_rules
                            .iter()
                            .filter(|r| r.matches(Some(&app), Some(&title)))
                            .filter_map(|r| r.scroll_factor)
                            .next_back()
                            .unwrap_or(base)
                    });
                let horizontal_amount = horizontal_amount * factor;
                let vertical_amount = vertical_amount * factor;
                let mut frame = AxisFrame::new(event.time_msec()).source(source);
                if horizontal_amount != 0.0 {
                    frame = frame.value(Axis::Horizontal, horizontal_amount);
                    if let Some(discrete) = horizontal_amount_discrete {
                        frame = frame.v120(Axis::Horizontal, (discrete * factor).round() as i32);
                    }
                }
                if vertical_amount != 0.0 {
                    frame = frame.value(Axis::Vertical, vertical_amount);
                    if let Some(discrete) = vertical_amount_discrete {
                        frame = frame.v120(Axis::Vertical, (discrete * factor).round() as i32);
                    }
                }

                if source == AxisSource::Finger {
                    if event.amount(Axis::Horizontal) == Some(0.0) {
                        frame = frame.stop(Axis::Horizontal);
                    }
                    if event.amount(Axis::Vertical) == Some(0.0) {
                        frame = frame.stop(Axis::Vertical);
                    }
                }

                let pointer = self.seat.get_pointer().unwrap();
                pointer.axis(self, frame);
                pointer.frame(self);
            }
            _ => {}
        }
    }
    pub(crate) fn pointer_motion(
        &mut self,
        pos: smithay::utils::Point<f64, smithay::utils::Logical>,
        time: u32,
    ) {
        let pos = self.constrained_position(pos);
        if self.pointer_locked() {
            return;
        }
        if self.backend_name == "native" {
            self.dirty = true;
        }
        let serial = SERIAL_COUNTER.next_serial();

        let pointer = self.seat.get_pointer().unwrap();

        if let Some(draw) = &mut self.draw {
            draw.current = pos;
            self.dirty = true;
        }
        if let Some(drag) = &self.drag {
            let delta: smithay::utils::Point<i32, smithay::utils::Logical> =
                (pos - drag.origin).to_i32_round();
            let mut rect = drag.rect;
            if drag.resize {
                rect.size.w = (rect.size.w + delta.x).max(80);
                rect.size.h = (rect.size.h + delta.y).max(60);
            } else {
                rect.loc += delta;
            }
            let window = drag.window.clone();
            if let Some(m) = self.windows.iter_mut().find(|m| m.window == window) {
                m.rect = rect;
            }
            self.relayout();
        }
        let under = self.pointer_target(pos);

        pointer.motion(
            self,
            under,
            &MotionEvent {
                location: pos,
                serial,
                time,
            },
        );
        pointer.frame(self);
        self.sync_pointer_capture();
    }

    fn pointer_target(
        &self,
        pos: smithay::utils::Point<f64, smithay::utils::Logical>,
    ) -> Option<(
        smithay::reexports::wayland_server::protocol::wl_surface::WlSurface,
        smithay::utils::Point<f64, smithay::utils::Logical>,
    )> {
        if self.draw.is_some()
            || self.drag.is_some()
            || self.panels.contains(pos)
            || self.ui.contains(pos)
            || self.menu.open
            || self.wifi.open
            || self.tray.open
            || self.launcher.open
            || self.screencast.prompt_open()
        {
            None
        } else {
            self.surface_under(pos)
        }
    }

    pub(crate) fn refresh_pointer_focus(&mut self, time: u32) {
        let pointer = self.seat.get_pointer().unwrap();
        if pointer.is_grabbed() || self.pointer_locked() {
            return;
        }
        let pos = pointer.current_location();
        let under = self.pointer_target(pos);
        if pointer.current_focus_with_location() != under {
            self.pointer_motion(pos, time);
        }
    }
    pub(crate) fn pointer_button(&mut self, button: u32, button_state: ButtonState, time: u32) {
        self.refresh_pointer_focus(time);
        let pointer = self.seat.get_pointer().unwrap();
        let keyboard = self.seat.get_keyboard().unwrap();

        let serial = SERIAL_COUNTER.next_serial();

        if self.draw.is_some() {
            if button_state == ButtonState::Pressed {
                self.suppressed_buttons.insert(button);
            } else {
                self.suppressed_buttons.remove(&button);
            }
            if button == 0x110 {
                if button_state == ButtonState::Pressed {
                    let draw = self.draw.as_mut().unwrap();
                    draw.start = Some(pointer.current_location());
                    draw.current = pointer.current_location();
                } else if let Err(error) = self.complete_draw() {
                    tracing::warn!(%error, "draw terminal failed");
                }
            } else if button_state == ButtonState::Pressed {
                self.draw = None;
            }
            self.dirty = true;
            return;
        }
        // A canceled compositor gesture must not leak an unmatched button release to a client.
        if button_state == ButtonState::Released && self.suppressed_buttons.remove(&button) {
            self.drag = None;
            self.dirty = true;
            return;
        }
        if self.drag.as_ref().is_some_and(|d| d.button == button)
            && button_state == ButtonState::Released
        {
            self.drag = None;
            self.dirty = true;
            return;
        }
        if self.menu.open {
            if button_state == ButtonState::Pressed {
                self.suppressed_buttons.insert(button);
                if button == 0x110 || button == 0x111 {
                    self.menu_click(pointer.current_location(), button == 0x111);
                }
            }
            return;
        }
        if self.tray.open {
            if button_state == ButtonState::Pressed {
                self.suppressed_buttons.insert(button);
                if button == 0x110 || button == 0x111 {
                    self.tray.click(pointer.current_location(), button == 0x111);
                }
                self.dirty = true;
            }
            return;
        }
        if self.screencast.prompt_open() {
            if button_state == ButtonState::Pressed {
                self.suppressed_buttons.insert(button);
                if button == 0x110 || button == 0x111 {
                    self.screencast
                        .click(pointer.current_location(), button == 0x111);
                }
                self.dirty = true;
            }
            return;
        }
        if self.launcher.open {
            if button_state == ButtonState::Pressed {
                self.suppressed_buttons.insert(button);
                if button == 0x110 || button == 0x111 {
                    self.launcher
                        .click(pointer.current_location(), button == 0x111);
                }
                self.dirty = true;
            }
            return;
        }
        if self.wifi.open {
            if button_state == ButtonState::Pressed {
                self.suppressed_buttons.insert(button);
                if button == 0x110 || button == 0x111 {
                    self.wifi.click(pointer.current_location(), button == 0x111);
                }
                self.dirty = true;
            }
            return;
        }
        if !pointer.is_grabbed()
            && self.drag.is_none()
            && self.ui.contains(pointer.current_location())
        {
            if button_state == ButtonState::Pressed {
                self.suppressed_buttons.insert(button);
                if button == 0x110 || button == 0x111 {
                    if let Some(action) = self.ui.click(pointer.current_location(), button == 0x111)
                    {
                        match action {
                            crate::ui::Interaction::CloseHelp => self.ui.close(),
                            crate::ui::Interaction::Dismiss(id) => {
                                self.notifications.close(id, 2);
                            }
                            crate::ui::Interaction::Invoke(id, key) => {
                                self.notifications.invoke(id, &key)
                            }
                        }
                    }
                }
                self.dirty = true;
            }
            return;
        }
        if self.config.panels.enabled
            && !pointer.is_grabbed()
            && self.drag.is_none()
            && self.panels.contains(pointer.current_location())
        {
            if button_state == ButtonState::Pressed {
                self.suppressed_buttons.insert(button);
                if button == 0x110 {
                    if let Some(action) = self.panels.click(pointer.current_location()) {
                        if let Err(error) = self.action(&action) {
                            tracing::warn!(%error,"panel click failed");
                        }
                    }
                }
            }
            return;
        }
        if button_state == ButtonState::Pressed && !pointer.is_grabbed() {
            let window = self
                .space
                .element_under(pointer.current_location())
                .map(|(w, _)| w.clone());
            let already_focused = self.focused == window;
            self.focus(window.clone());
            if already_focused {
                if let Some(x11) = window.as_ref().and_then(|w| w.x11_surface()) {
                    x11.restore_input_focus_if_unset();
                }
            }
            let mods = keyboard.modifier_state();
            let primary = match self.config.keyboard.modifier.as_str() {
                "Super" => mods.logo,
                "Ctrl" => mods.ctrl,
                _ => mods.alt,
            };
            if primary && (button == 0x110 || button == 0x111) {
                self.suppressed_buttons.insert(button);
                if let Some(window) = window {
                    if let Some(m) = self.windows.iter_mut().find(|m| m.window == window) {
                        m.floating = true;
                        m.fullscreen = false;
                        self.drag = Some(crate::wm::DragState {
                            window,
                            origin: pointer.current_location(),
                            rect: m.rect,
                            resize: button == 0x111,
                            button,
                        });
                    }
                }
                self.relayout();
                return;
            }
        }

        pointer.button(
            self,
            &ButtonEvent {
                button,
                state: button_state,
                serial,
                time,
            },
        );
        pointer.frame(self);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{config::Config, CalloopData};
    use smithay::reexports::{calloop::EventLoop, wayland_server::Display};

    fn fixture() -> (EventLoop<'static, CalloopData>, Ember) {
        // Tests create an isolated Wayland socket, without opening a window or touching the host seat.
        static RUNTIME: std::sync::OnceLock<()> = std::sync::OnceLock::new();
        RUNTIME.get_or_init(|| {
            if std::env::var_os("XDG_RUNTIME_DIR").is_none() {
                use std::os::unix::fs::PermissionsExt;
                let dir =
                    std::env::temp_dir().join(format!("emberwm-tests-{}", std::process::id()));
                std::fs::create_dir_all(&dir).unwrap();
                std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).unwrap();
                std::env::set_var("XDG_RUNTIME_DIR", dir);
            }
        });
        let mut event_loop = EventLoop::try_new().unwrap();
        let config = Config {
            terminal: vec!["/bin/true".into(), "{app_id}".into()],
            ..Default::default()
        };
        let state = Ember::new(
            &mut event_loop,
            Display::new().unwrap(),
            config,
            "/tmp/ember-test-config.toml".into(),
        );
        (event_loop, state)
    }
    #[test]
    fn screenshot_gesture_selects_exact_reverse_rectangle_without_spawning() {
        let (_loop, mut state) = fixture();
        state.action("screenshot-region").unwrap();
        state.pointer_motion((600., 400.).into(), 1);
        state.pointer_button(0x110, ButtonState::Pressed, 2);
        state.pointer_motion((100., 80.).into(), 3);
        state.pointer_button(0x110, ButtonState::Released, 4);
        assert_eq!(
            state.clipboard_capture,
            Some(smithay::utils::Rectangle::new(
                (100, 80).into(),
                (500, 320).into()
            ))
        );
        assert!(
            state.draw.is_none() && state.children.is_empty() && state.pending_draws.is_empty()
        );
        state.clipboard_capture = None;
        state.action("screenshot-region").unwrap();
        state.pointer_button(0x111, ButtonState::Pressed, 5);
        state.pointer_button(0x111, ButtonState::Released, 6);
        assert!(state.draw.is_none() && state.clipboard_capture.is_none());
    }
    #[test]
    fn actual_pointer_path_draws_backwards_and_launches_once() {
        let (_event_loop, mut state) = fixture();
        state.arm_draw();
        state.pointer_motion((600., 500.).into(), 1);
        state.pointer_button(0x110, ButtonState::Pressed, 2);
        state.pointer_motion((100., 100.).into(), 3);
        state.pointer_button(0x110, ButtonState::Released, 4);
        assert!(state.draw.is_none());
        assert_eq!(state.pending_draws.len(), 1);
        let (rect, workspace, _) = state.pending_draws.values().next().unwrap();
        assert_eq!(
            *rect,
            smithay::utils::Rectangle::new((102, 102).into(), (496, 396).into())
        );
        assert_eq!(*workspace, 0);
        assert!(state.suppressed_buttons.is_empty());
        for mut child in state.children.drain(..) {
            child.wait().unwrap();
        }
    }
    #[test]
    fn panel_click_uses_workspace_action_and_swallows_button_release() {
        let (_event_loop, mut state) = fixture();
        state.config.panels.enabled = true;
        state.config.panels.hide_empty_workspaces = false;
        state.config.panels.items.push(crate::panels::Item {
            name: "test".into(),
            modules: vec!["workspaces".into()],
            ..Default::default()
        });
        let input = crate::panels::Input::from_state(&state);
        state
            .panels
            .update(&state.config.panels, &state.config_path, 0, input);
        let pos = state.panels.views[0].widgets[1].rect.loc.to_f64()
            + smithay::utils::Point::from((2., 2.));
        state.pointer_motion(pos, 1);
        state.pointer_button(0x110, ButtonState::Pressed, 2);
        assert_eq!(state.workspace, 1);
        state.pointer_button(0x110, ButtonState::Released, 3);
        assert!(state.suppressed_buttons.is_empty());
        assert!(state.children.is_empty());
    }
    #[test]
    fn notification_pointer_path_invokes_action_without_spawning_a_command() {
        let (_event_loop, mut state) = fixture();
        let id = state.notifications.notify(
            &state.config.notifications,
            crate::notifications::Request {
                app: "Test",
                replaces: 0,
                summary: "Action",
                body: "Click",
                actions: vec![("copy".into(), "Copy".into())],
                urgency: 1,
                resident: false,
                timeout: 0,
                sender: Some(":1.2".into()),
            },
        );
        let size = state.output_size();
        state.ui.update(
            &state.config,
            &state.config_path,
            0,
            false,
            size,
            &state.notifications,
        );
        let data = state.ui.status();
        let b = &data["cards"][0]["buttons"][0];
        let pos = smithay::utils::Point::from((
            b["x"].as_f64().unwrap() + 2.,
            b["y"].as_f64().unwrap() + 2.,
        ));
        state.pointer_motion(pos, 1);
        state.pointer_button(0x110, ButtonState::Pressed, 2);
        state.pointer_button(0x110, ButtonState::Released, 3);
        assert!(state.notifications.entries.is_empty());
        assert!(state.notifications.signals.iter().any(
            |s| matches!(s,crate::notifications::Signal::Action(n,k,_) if *n==id && k=="copy")
        ));
        assert!(state.suppressed_buttons.is_empty() && state.children.is_empty());
    }
    #[test]
    fn tiny_or_canceled_selection_never_spawns() {
        let (_event_loop, mut state) = fixture();
        state.arm_draw();
        state.pointer_motion((20., 20.).into(), 1);
        state.pointer_button(0x110, ButtonState::Pressed, 2);
        state.pointer_motion((22., 23.).into(), 3);
        state.pointer_button(0x110, ButtonState::Released, 4);
        assert!(state.pending_draws.is_empty());
        state.arm_draw();
        state.pointer_button(0x111, ButtonState::Pressed, 5);
        assert!(state.draw.is_none());
        state.pointer_button(0x111, ButtonState::Released, 6);
        assert!(state.children.is_empty());
        assert!(state.suppressed_buttons.is_empty());
    }
}
