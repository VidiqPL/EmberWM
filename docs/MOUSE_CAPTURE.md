# Gaming mouse capture

Ember 0.10 exposes Wayland relative-pointer and pointer-constraints protocols.
Native Wayland games can request a locked cursor for camera movement or confine
it to their surface/region. Relative events carry both accelerated and raw
unaccelerated deltas. A lock holds the absolute position, hides the compositor
cursor and continues sending relative motion; it does not cause idle redraws.

**Super+Escape** releases a game's capture. Opening the launcher, help, Wi-Fi,
a capture consent prompt, drawing mode, changing keyboard focus or switching
workspaces also releases capture. Super+Shift+Escape still exits Ember.
Persistent constraints can reactivate after leaving and re-entering the window. One-shot constraints never reactivate after deactivation.
Valid committed cursor-position hints are applied when a client unlocks.

Bindings are configurable:

```toml
[[bindings]]
key = "Escape"
action = "pointer-release"
```

The primary modifier follows `[keyboard].modifier`; standalone defaults use Super,
nested defaults use Alt. A copied personal config needs this binding added.
IPC: `emberwm ctl pointer-release`; `emberwm ctl status` reports `pointer_locked`.

Native virtual-seat tests cover relative/raw deltas under lock, no absolute
motion while locked, hints, confinement boundaries, persistent and one-shot
release, launcher escape and workspace switches. See
mouse-themes-native-results.json.

Use the standalone SDDM Ember session for gameplay. Nested Winit is a development
preview; its host cursor is constrained by the outer desktop/window. Real
Sober/Roblox camera feel still needs a gameplay check in the updated native session.


0.14.1 fixes X11 gaming focus: Ember now delivers the ICCCM focus handshake,
including WM_TAKE_FOCUS, in addition to Wayland keyboard focus. This matters for
Proton games and other globally active X11 windows. Super+Escape also blocks
replacement locks on the same surface until the pointer leaves it; a game can
no longer immediately undo the user's escape request. Stale cursor hints are
cleared on release. These changes require a new Ember login. Native virtual-seat
checks cover the focus protocol, keys, relative motion and emergency release;
Payday 3 gameplay itself remains unverified.
