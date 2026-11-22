# Notification reliability in 0.9

Fixed a missed-wakeup race: libdbus's final nonblocking I/O call could read another
message after the compositor drained its queue. The socket then had no readable
bytes, so the internal message could remain stranded. The service now preserves
and immediately schedules that pending message. Dispatch remains bounded so a
burst cannot monopolize input/rendering.

The notification name is acquired before terminal/audio startup, and the service
retries after an initial name conflict or disconnect without requiring config
reload. A client can no longer replace another client's notification by guessing
its numeric ID. The outgoing signal allocation is released after dispatch.

Ember also implements the Notification portal backend on its existing event-loop
D-Bus connection. Sandboxed apps can send and withdraw portal notifications,
replace them by app/id and invoke their actions. Default actions activate the app
through the standard `org.freedesktop.Application` interface with the original
action target. GTK notification rendering and a second notification daemon are
not needed for that path.

The native session exports its Wayland/desktop environment to D-Bus activation.
Earlier session logs showed activated GTK helpers failing with `cannot open
display`; this corrects the missing environment for newly activated helpers.

Existing queue limits, Do Not Disturb, critical notices, timeouts and action
buttons remain configurable. A required listener remains active while
notifications are enabled; expired/dismissed cards release their pixel buffers
and font references. There is no hidden notification window for every app.

Diagnostics: `emberwm ctl notifications` reports ownership/error state and
queued items. `notification-action ID KEY` invokes an exposed action; normal
clicks on cards/buttons use the same path.

See `notifications-recovery-results.json`
for conflict recovery, queued-message regression coverage, independent clients,
portal replacement/withdrawal and app activation. The original notification,
DND, expiry, bounds and generated-help integration suite is also retained.
