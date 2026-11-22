//! Client-requested gaming cursor capture. No input polling or helper process.
use crate::Ember;
use smithay::{
    input::pointer::{MotionEvent, PointerHandle},
    reexports::wayland_server::protocol::wl_surface::WlSurface,
    utils::{Logical, Point, SERIAL_COUNTER},
    wayland::pointer_constraints::{
        with_pointer_constraint, PointerConstraint, PointerConstraintsHandler,
    },
};

#[derive(Default)]
pub struct Runtime {
    inhibited: Option<WlSurface>,
    hint: Option<(WlSurface, Point<f64, Logical>)>,
}
impl Ember {
    pub fn capture_allowed(&self) -> bool {
        self.session_active
            && !self.launcher.open
            && !self.wifi.open
            && !self.tray.open
            && !self.ui.help_open
            && !self.screencast.prompt_open()
            && self.draw.is_none()
            && self.drag.is_none()
    }
    pub fn pointer_locked(&self) -> bool {
        let pointer = self.seat.get_pointer().unwrap();
        pointer.current_focus().is_some_and(|surface| {
            with_pointer_constraint(&surface, &pointer, |c| {
                c.is_some_and(|c| c.is_active() && matches!(*c, PointerConstraint::Locked(_)))
            })
        })
    }
    pub fn release_pointer(&mut self) {
        self.deactivate_pointer(true);
    }
    pub fn suspend_pointer(&mut self) {
        self.deactivate_pointer(false);
    }
    fn deactivate_pointer(&mut self, inhibit: bool) {
        self.mouse.hint = None;
        let pointer = self.seat.get_pointer().unwrap();
        if let Some(surface) = pointer.current_focus() {
            if inhibit {
                self.mouse.inhibited = Some(surface.clone());
            }
            let active = with_pointer_constraint(&surface, &pointer, |c| {
                if let Some(c) = c.filter(|c| c.is_active()) {
                    c.deactivate();
                    true
                } else {
                    false
                }
            });
            if active {
                self.dirty = true;
            }
        }
    }
    pub fn sync_pointer_capture(&mut self) {
        let pointer = self.seat.get_pointer().unwrap();
        let Some(surface) = pointer.current_focus() else {
            self.mouse.inhibited = None;
            return;
        };
        if self.mouse.inhibited.as_ref().is_some_and(|s| s != &surface) {
            self.mouse.inhibited = None;
        }
        let keyboard = self.seat.get_keyboard().and_then(|k| k.current_focus());
        if !self.capture_allowed()
            || keyboard
                .as_ref()
                .is_none_or(|k| surface_root(k) != surface_root(&surface))
        {
            self.suspend_pointer();
            return;
        }
        if self.mouse.inhibited.as_ref() == Some(&surface) {
            return;
        }
        let pos = pointer.current_location();
        let Some((under, origin)) = self.surface_under(pos).filter(|(s, _)| s == &surface) else {
            return;
        };
        with_pointer_constraint(&under, &pointer, |c| {
            if let Some(c) = c.filter(|c| !c.is_active()) {
                if c.region()
                    .is_none_or(|r| r.contains((pos - origin).to_i32_floor()))
                {
                    c.activate();
                    self.dirty = true;
                }
            }
        });
    }
    /// Apply a lock/confine before forwarding absolute motion. Relative motion is sent separately.
    pub fn constrained_position(&mut self, pos: Point<f64, Logical>) -> Point<f64, Logical> {
        self.sync_pointer_capture();
        let pointer = self.seat.get_pointer().unwrap();
        let old = pointer.current_location();
        let Some(surface) = pointer.current_focus() else {
            return pos;
        };
        let Some((_, origin)) = self.surface_under(old).filter(|(s, _)| s == &surface) else {
            return pos;
        };
        // Copy constraint metadata before hit-testing: surface traversal also takes the
        // compositor surface lock and must never run inside with_pointer_constraint.
        let constraint = with_pointer_constraint(&surface, &pointer, |c| {
            c.filter(|c| c.is_active()).map(|c| {
                (
                    matches!(*c, PointerConstraint::Locked(_)),
                    c.region().cloned(),
                )
            })
        });
        let Some((locked, region)) = constraint else {
            return pos;
        };
        if locked {
            return old;
        }
        let contains = |p: Point<f64, Logical>| {
            region
                .as_ref()
                .is_none_or(|r| r.contains((p - origin).to_i32_floor()))
                && self.surface_under(p).is_some_and(|(s, _)| s == surface)
        };
        let delta = pos - old;
        let steps = delta.x.abs().max(delta.y.abs()).ceil().clamp(1., 4096.) as u32;
        let step: Point<f64, Logical> = (delta.x / steps as f64, delta.y / steps as f64).into();
        let mut at = old;
        for _ in 0..steps {
            let next = at + step;
            if contains(next) {
                at = next;
            } else if contains((next.x, at.y).into()) {
                at.x = next.x;
            } else if contains((at.x, next.y).into()) {
                at.y = next.y;
            }
        }
        at
    }
}
impl PointerConstraintsHandler for Ember {
    fn new_constraint(&mut self, _surface: &WlSurface, _pointer: &PointerHandle<Self>) {
        self.sync_pointer_capture();
    }
    fn cursor_position_hint(
        &mut self,
        surface: &WlSurface,
        pointer: &PointerHandle<Self>,
        location: Point<f64, Logical>,
    ) {
        // Hints are surface-local and only honoured for an active lock.
        if with_pointer_constraint(surface, pointer, |c| c.is_some_and(|c| c.is_active())) {
            self.mouse.hint = Some((surface.clone(), location));
        }
    }
}
// Position hints take effect after the client destroys/unlocks its lock, never while it is locked.
impl Ember {
    pub fn apply_pointer_hint(&mut self) {
        let Some((surface, hint)) = self.mouse.hint.clone() else {
            return;
        };
        let pointer = self.seat.get_pointer().unwrap();
        if pointer.current_focus().as_ref() != Some(&surface) {
            self.mouse.hint = None;
            return;
        }
        if self.pointer_locked() {
            return;
        }
        self.mouse.hint = None;
        let Some((_, origin)) = self
            .surface_under(pointer.current_location())
            .filter(|(s, _)| s == &surface)
        else {
            return;
        };
        let pos = origin + hint;
        if self.surface_under(pos).is_some_and(|(s, _)| s == surface) {
            pointer.motion(
                self,
                Some((surface, origin)),
                &MotionEvent {
                    location: pos,
                    serial: SERIAL_COUNTER.next_serial(),
                    time: self.start_time.elapsed().as_millis() as u32,
                },
            );
            pointer.frame(self);
            self.dirty = true;
        }
    }
}
smithay::delegate_pointer_constraints!(Ember);
smithay::delegate_relative_pointer!(Ember);

fn surface_root(surface: &WlSurface) -> WlSurface {
    let mut root = surface.clone();
    while let Some(parent) = smithay::wayland::compositor::get_parent(&root) {
        root = parent;
    }
    root
}
