//! An on-demand floating window group. Hidden members are unmapped, not closed.
use crate::{layout::Rect, Ember};
use smithay::desktop::Window;

#[derive(Clone, Copy)]
pub struct Placement {
    pub floating: bool,
    pub rect: Rect,
}

#[derive(Default)]
pub struct Runtime {
    pub visible: bool,
    pub return_focus: Option<Window>,
    pub last_focused: Option<Window>,
}

impl Ember {
    fn scratchpad_has_pointer(&self) -> bool {
        if self.drag.as_ref().is_some_and(|drag| {
            self.windows
                .iter()
                .any(|m| m.window == drag.window && m.scratchpad.is_some())
        }) {
            return true;
        }
        let pointer = self.seat.get_pointer().unwrap();
        let surface = pointer.current_focus().or_else(|| {
            pointer
                .grab_start_data()
                .and_then(|start| start.focus.map(|(surface, _)| surface))
        });
        let Some(surface) = surface else { return false };
        let mut found = false;
        for m in self.windows.iter().filter(|m| m.scratchpad.is_some()) {
            m.window.with_surfaces(|s, _| found |= s == &surface);
        }
        found
    }

    fn normal_focus(&self) -> Option<Window> {
        self.space
            .elements()
            .rev()
            .find(|window| {
                self.windows
                    .iter()
                    .any(|m| &m.window == *window && m.scratchpad.is_none())
            })
            .cloned()
    }

    fn normal_visible(&self, window: &Window) -> bool {
        self.space.element_location(window).is_some()
            && self
                .windows
                .iter()
                .any(|m| &m.window == window && m.scratchpad.is_none())
    }

    pub fn raise_scratchpad(&mut self) {
        for m in &self.windows {
            if m.scratchpad.is_some() && self.space.element_location(&m.window).is_some() {
                self.space.raise_element(&m.window, false);
            }
        }
        if let Some(window) = self.focused.as_ref().filter(|window| {
            self.windows
                .iter()
                .any(|m| &m.window == *window && m.scratchpad.is_some())
        }) {
            self.space.raise_element(window, false);
        }
        for m in &self.windows {
            if (crate::wm::unmanaged(&m.window) || crate::wm::game_overlay(&m.window))
                && self.space.element_location(&m.window).is_some()
            {
                self.space.raise_element(&m.window, false);
            }
        }
    }

    pub fn scratchpad_mark(&mut self) {
        let Some(window) = self.focused.clone() else {
            return;
        };
        let Some(index) = self.windows.iter().position(|m| m.window == window) else {
            return;
        };
        if self.windows[index].scratchpad.is_some() {
            self.scratchpad_release(&window, self.workspace);
            let next = if self.space.element_location(&window).is_some() {
                Some(window)
            } else {
                self.space.elements().last().cloned()
            };
            self.focus(next);
            return;
        }
        let shown_rect = self.windows[index].rect;
        // Fullscreen and the scratchpad have separate stacking/focus policies.
        if self.windows[index].fullscreen {
            self.set_fullscreen(&window, false);
        }
        let m = &mut self.windows[index];
        m.scratchpad = Some(Placement {
            floating: m.floating,
            rect: m.rect,
        });
        m.rect = shown_rect;
        m.floating = true;
        self.scratchpad.return_focus = self
            .scratchpad
            .return_focus
            .take()
            .filter(|w| self.normal_visible(w))
            .or_else(|| self.normal_focus());
        self.scratchpad.visible = true;
        self.draw = None;
        self.drag = None;
        self.relayout();
        self.focus(Some(window));
    }

    pub fn scratchpad_release(&mut self, window: &Window, workspace: usize) {
        if !self
            .windows
            .iter()
            .any(|m| &m.window == window && m.scratchpad.is_some())
        {
            return;
        }
        self.set_fullscreen(window, false);
        let m = self
            .windows
            .iter_mut()
            .find(|m| &m.window == window)
            .unwrap();
        let saved = m.scratchpad.take().unwrap();
        m.workspace = workspace;
        m.floating = saved.floating;
        m.rect = saved.rect;
        self.scratchpad_cleanup();
        self.relayout();
    }

    pub fn scratchpad_toggle(&mut self) {
        if !self.windows.iter().any(|m| m.scratchpad.is_some()) {
            return;
        }
        let release_pointer = self.scratchpad.visible && self.scratchpad_has_pointer();
        self.draw = None;
        self.drag = None;
        if self.scratchpad.visible {
            let pointer = self.seat.get_pointer().unwrap();
            let position = pointer.current_location();
            let time = smithay::utils::Clock::<smithay::utils::Monotonic>::new()
                .now()
                .as_millis();
            if release_pointer {
                self.suspend_pointer();
                if pointer.is_grabbed() {
                    pointer.unset_grab(self, smithay::utils::SERIAL_COUNTER.next_serial(), time);
                    for m in self.windows.iter().filter(|m| m.scratchpad.is_some()) {
                        crate::grabs::resize_grab::cancel(&m.window);
                    }
                }
                pointer.motion(
                    self,
                    None,
                    &smithay::input::pointer::MotionEvent {
                        location: position,
                        serial: smithay::utils::SERIAL_COUNTER.next_serial(),
                        time,
                    },
                );
                pointer.frame(self);
            }
            let next = self
                .focused
                .clone()
                .filter(|w| self.normal_visible(w))
                .or_else(|| {
                    self.scratchpad
                        .return_focus
                        .take()
                        .filter(|w| self.normal_visible(w))
                })
                .or_else(|| self.normal_focus());
            self.scratchpad.visible = false;
            self.relayout();
            self.focus(next);
            if release_pointer {
                self.pointer_motion(position, time);
            }
        } else {
            self.scratchpad.return_focus = self.focused.clone().filter(|w| self.normal_visible(w));
            self.scratchpad.visible = true;
            let next = self
                .scratchpad
                .last_focused
                .clone()
                .filter(|window| {
                    self.windows
                        .iter()
                        .any(|m| &m.window == window && m.scratchpad.is_some())
                })
                .or_else(|| {
                    self.windows
                        .iter()
                        .rev()
                        .find(|m| m.scratchpad.is_some())
                        .map(|m| m.window.clone())
                });
            self.relayout();
            self.focus(next);
        }
    }

    pub fn scratchpad_cleanup(&mut self) {
        if !self.windows.iter().any(|m| m.scratchpad.is_some()) {
            self.scratchpad = Runtime::default();
        } else {
            self.scratchpad.return_focus = self.scratchpad.return_focus.take().filter(|w| {
                self.windows
                    .iter()
                    .any(|m| &m.window == w && m.scratchpad.is_none())
            });
            self.scratchpad.last_focused = self.scratchpad.last_focused.take().filter(|w| {
                self.windows
                    .iter()
                    .any(|m| &m.window == w && m.scratchpad.is_some())
            });
        }
    }
}
