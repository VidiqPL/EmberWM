# Desktop controls in 0.15

Volume up/down and mute show a native bar, thumb and percentage for one second.
A bounded queue preserves repeated key presses; one temporary worker applies the
wpctl command and reads the resulting volume. The expiry restarts after the
latest result. No volume worker or overlay buffer remains after completion and
expiry. The bar is a display indicator; it does not intercept game input.

```toml
[audio]
enabled = true
osd_enabled = true
osd_duration_ms = 1000

[tray]
enabled = true
width = 460
font_size = 13.0

[[bindings]]
key = "comma"
action = "tray-toggle"
```

Super+, opens the app list. Left-click activates an app; right-click loads its
menu. Enter activates the selected row; Right opens its menu, Up/Down or the
wheel navigates, Left/Backspace returns to the app list, and Escape closes the
popup. The popup uses app names. It supports StatusNotifierItem and dbusmenu,
including enabled/visible flags, separators, toggles, and nested submenus. Apps
without a dbusmenu can receive ContextMenu to display their own native menu.
Legacy XEmbed-only icons require a separate bridge and are not hosted here.
The Gentoo installation includes libayatana-appindicator3 for modern tray apps.

A small D-Bus watcher must remain registered so apps can announce their tray
entries. It shares Ember's event loop through a watched descriptor. There is no
resident tray worker, hidden UI window or timer querying app properties. Metadata
and menus are queried only while the UI is being used; font and pixel buffers
are released when it closes. Disabling tray support releases the watcher name
and removes its event source. Startup conflicts/disconnections are retried with
a delay. App titles, registry size, menu rows and visible rows are bounded.

Diagnostics: `emberwm ctl tray`, and `emberwm ctl status`'s tray and volume_osd
fields. `tray-menu N`, `tray-activate N` and `tray-select N` address zero-based
rows for bindings/automation. The generated help sheet lists Super+,.

The X11 backend now forwards _NET_ACTIVE_WINDOW requests instead of discarding
them. A visible target is raised/focused; a target on another workspace can be
summoned by the currently focused app of the same class. Other apps cannot use
this request to switch the user's workspace. Another window's fullscreen state
is cleared as necessary to make the requested target visible. This addresses a
missing window-manager operation relevant to Steam's existing child windows;
the actual Friends > Add a Friend flow still needs a check in the new session.

Pointer focus is reconciled before wheel delivery only when the target changes, so windows/popups that
appear under a stationary cursor do not leave wheel events targeting an older
surface. A tunable X11 multiplier scales both smooth and v120 wheel values:

```toml
[xwayland]
enabled = true
scroll_factor = 1.0
```

Standalone and nested defaults both select 1.0. The earlier standalone 2.0
workaround was withdrawn after the user reported worse scrolling. The factor is limited to X11 windows and
can be reloaded. Native Wayland apps retain their existing wheel scale. This is
a scroll-distance adjustment, not evidence of a Steam rendering/frame-rate fix.

Log out and back into Ember to load the rebuilt compositor. Current apps and
the desktop were left running during development.
