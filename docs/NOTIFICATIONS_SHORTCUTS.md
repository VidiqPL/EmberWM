# Notifications and the live shortcut sheet

EmberWM 0.5 includes a native desktop notification server and a shortcut reference
window. Both render inside the compositor and share font geometry with panels.
No additional notification daemon, browser UI or async worker runtime is started.

## Keyboard controls

- **Super+/** opens/closes the shortcut sheet in the standalone session.
- **Escape**, its top-right close button, or clicking outside closes the sheet.
- Scroll to change pages when a small sheet cannot fit every binding.
- **Super+N** dismisses the latest notification.
- Left-click a notification action button to invoke it. Clicking its body invokes
  the default action if supplied, otherwise dismisses it. Right-click dismisses.
- Click the notification's top-right close button to dismiss it.

Nested previews use **Alt** by default. These shortcuts are regular configurable
bindings, so changes to their keys are reflected in the sheet. Compositor
shortcuts remain usable while it is open; unmatched key presses are consumed so
they cannot type into the underlying application.

The shortcut list is generated from **all active bindings**, not a separate
manually maintained list. After editing the config, press **Super+R**; an open
sheet rebuilds immediately. Reload failures preserve the working sheet and config.
An optional `description` on any binding supplies your own label:

```toml
[[bindings]]
key = "t"
action = "exec foot"
description = "Open my terminal"
```

The sheet also lists built-in mouse gestures and, in a native session, VT keys.
Changing the configured primary modifier updates their labels too.

## Configuration

The shipped configs include these sections. Edit the existing sections rather
than duplicating them; `ctl status` reports the active file path.

```toml
[shortcuts]
enabled = true
width = 860
font_size = 12.0

[notifications]
enabled = true
width = 340
margin = 12
gap = 8
max_visible = 4
max_queued = 32
timeout_ms = 5000
do_not_disturb = false
critical_bypass_dnd = true
```

Set either section's `enabled` to `false` to disable that feature. Reloading
notification disable also clears retained notices and releases the D-Bus service
name. Re-enabling claims it again if available. The notifications appear at the
top-right below reserved panel space. Font and colours follow `[panels]` styling,
including a custom font path even if panels themselves are disabled.

The default queue is bounded to 32 notices and at most four visible cards. Oldest
notices are removed when the queue is full. Bodies are limited to 1,024 characters,
summaries to 160, and actions to four. No notification history is stored on disk.
Only visible cards have pixel buffers. Idle overlays have no font or buffers of
their own; the panel may still hold its shared font. Critical notices with a
server-default timeout persist until dismissed; explicit positive client timeouts
are honored, and timeout zero never expires. DND suppresses new normal notices;
critical notices bypass it by default.

Notification body text and labeled actions are supported. Images, themed icons,
markup, sound hints, activation tokens and persistent notification history are
not implemented. The server advertises only its supported optional capabilities.
Action keys are delivered back to the sending application; they are not executed
as shell commands by Ember.

## Standard application integration

The server owns `org.freedesktop.Notifications` on the session bus, at
`/org/freedesktop/Notifications`. It implements `Notify`, `CloseNotification`,
`GetCapabilities`, `GetServerInformation`, introspection, `NotificationClosed`
and `ActionInvoked`. Replacement IDs and client expiry hints are handled.

SDDM's Ember wrapper starts a private session D-Bus. Applications launched by
Ember inherit it, so standard notification clients can send notifications there.
An existing server is respected: Ember never replaces it or queues to steal its
name later. In a nested Plasma preview, the existing desktop notification server
may therefore handle app notifications; `ctl notify` still previews native cards.

```sh
./target/release/emberwm ctl notify 'Hello | This is an Ember notification'
./target/release/emberwm ctl notifications
./target/release/emberwm ctl shortcuts
./target/release/emberwm ctl notifications-clear
./target/release/emberwm ctl notifications-toggle-dnd
```

The final two actions can also be assigned to a binding. DND toggling is temporary;
reloading restores the configured value.

## Memory and tests

The server uses a socket event source on the compositor's existing loop. Static
sheets/cards reuse their render buffers, with notification expiry as the only new
idle deadline. Fonts are held through a weak cache and shared with panels. Pixel
surfaces are drawn into their final buffers without a temporary full-size copy.

The memory review also replaced the wallpaper's persistent CPU pixel copy with a
renderer texture. See `MEMORY-0.5.md` for measured savings and limitations.

```sh
cargo test --locked -j4
cargo clippy --locked --all-targets -j4 -- -D warnings
dbus-run-session -- python3 tests/notifications-smoke.py
python3 tests/vm/notifications-smoke.py
```

The notification suite requires Python GObject/Gio and an existing graphical
session for nesting. The VM suite requires the generated isolated fixture.
Testing uses private buses and virtual input; it does not replace Plasma's
notification server or inject input into the host desktop.

Notifications with a generated shortcut sheet (development screenshot omitted)
