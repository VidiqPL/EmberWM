# Smithay 0.7.0 patch

Smithay 0.7.0 is copied from its crates.io release, retaining its MIT license.
One production source change is in `X11Wm::send_selection`: non-text MIME
atoms are resolved with `intern_atom(only_if_exists=true)` rather than reading
`TARGETS` from the selection proxy window. The original implementation queries
a property that was never populated there (the source targets arrived on
`_WL_SELECTION` and were consumed). Consequently image/png requests fail with
UnableToDetermineAtom even though the MIME was correctly advertised to Wayland.
This also permits repeated pastes without consuming metadata. Unknown atoms
still fail. Wayland data offers limit requests to the advertised MIME list;
the actual X11 owner still decides whether to serve the selection.

No registry files are modified. Keep this pin until the equivalent upstream fix
is available and validated. `tests/xwayland-smoke.py` exercises both directions
through real arboard and Wayland clients, including INCR image transfers.

0.14.1 also exposes `X11Surface::set_input_focus(bool)` by extracting the
existing ICCCM focus logic from its KeyboardTarget implementation. Ember uses
WlSurface keyboard targets, so it now explicitly performs XSetInputFocus and
WM_TAKE_FOCUS before Wayland keyboard focus delivery. This preserves native
clipboard/pointer integration while supporting globally active X11 clients.

0.15 adds an XwmHandler activation_request hook and dispatches
_NET_ACTIVE_WINDOW client messages to it. Ember applies its workspace/focus
policy before raising a requested existing X11 window.

0.15.3 adds X11Wm::set_active_window, called by Ember's managed focus policy,
and stops publishing raw FocusIn/FocusOut event targets as _NET_ACTIVE_WINDOW.
Those targets can be root/frame windows or unmanaged popup windows.

0.15.4 makes X11Surface::set_input_focus ignore override-redirect surfaces.
The compositor also leaves their activation state untouched and no longer
reasserts input focus for redundant activation of an already-focused parent.
This follows the override-redirect focus rule in
https://github.com/Supreeeme/xwayland-satellite/pull/494 .
The compositor's 0.15.3 popup-focus override has been removed.

0.16.1 adds `PointerHandle::current_focus_with_location`, a read-only accessor
for the current target and its surface origin. Ember uses both to avoid sending
synthetic motion for unrelated UI updates while retaining correct coordinates
when a focused surface moves under a stationary pointer.
