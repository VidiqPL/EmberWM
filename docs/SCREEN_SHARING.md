# Screen sharing

Ember 0.10 supports whole-display and **individual-window video sharing** through
the ScreenCast desktop portal and PipeWire. Use an app's Share Screen action;
Ember's native picker shows the display and mapped windows, filtered to the source
types requested by the app. Up/down or the mouse wheel selects a source; Tab or
left/right selects Cancel/Share, and Enter confirms. Cancel is selected initially.
You can click a source and then Share selected. Escape cancels.

A window stream renders that window's content in isolation: covering windows,
panels, wallpaper and compositor popups cannot enter it. The source stays available
on other workspaces and stops when the window unmaps/closes. Stop sharing in the
app to close the stream. Cursor inclusion follows the app's requested mode.

The user configuration is installed for **Ember only**. KDE and Hyprland portal
choices remain separate. Log out and enter Ember again after rebuilding so its
session activation environment and new Wayland protocol are available. Restart
apps that were running outside the new Ember session.

## Installed components

- Ember implements wlr-screencopy v3 with SHM buffers, full output and region
  capture, cursor inclusion/exclusion, damage requests and bounded pending copies.
- The already-installed `xdg-desktop-portal`, `xdg-desktop-portal-wlr`, PipeWire and
  WirePlumber provide application-facing portal requests and video transport.
- Ember also implements ext-foreign-toplevel-list, ext-image-capture-source and
  ext-image-copy-capture with SHM buffers, source-size updates, timestamps,
  embedded/hidden cursors, full damage and source-stop events.
- The portal invokes `emberwm --choose-source` in dmenu mode for the native picker;
  only sources offered by the requesting app/backend are selectable.
  Sharing defaults to 30 FPS to avoid unnecessary capture work at 144 Hz.
- The session exports its Wayland display and desktop to D-Bus activation before
  launching apps, so activated helpers connect to the correct compositor.

Per-user files installed by `sh scripts/install-portals.sh`:

- `~/.local/share/xdg-desktop-portal/portals/ember.portal`
- `~/.config/xdg-desktop-portal/ember-portals.conf`
- `~/.config/xdg-desktop-portal-wlr/Ember`

Existing differing files are backed up. The script is idempotent and does not
restart current desktop services. GTK remains the fallback for unrelated portals
such as file selection; notification rendering uses Ember's native backend.

## Configure and memory

```toml
[screencast]
enabled = true
allow_direct_capture = false
```

Capture is exposed to the installed portal backend by default. Set
`allow_direct_capture = true` only if you also want native direct capture tools
such as grim to see the protocol. These direct clients bypass the portal prompt.
Ember's built-in screenshot shortcut does not need this setting.

There is no permanently allocated stream image or capture thread. Requested
frames copy directly from the GPU readback mapping into the client's SHM buffer;
this avoids an extra 7.9 MiB CPU image at 1080p. Temporary GPU targets/mappings
are released after use. Closed consent UI buffers are released immediately, and
freed allocator memory is reclaimed after a stream settles. The launcher and
consent helper exit or release their data when finished.

Portal and media services are session infrastructure and can remain resident
after an app activates them. They are not counted as zero-cost or stopped while
apps may still need them. No portal backend is eagerly launched by Ember itself.

## Verification and limits

`portal-results.json` records a real ScreenCast portal
request, both consent choices, a restricted PipeWire remote FD and 12 actual
video frames. Pixel checks cover colour and orientation. The test uses a private
bus and an isolated PipeWire/WirePlumber stack with hardware monitors disabled,
and shares a synthetic nested desktop rather than the user's screen.

The same pipeline also passed with PipeWire/WirePlumber on a separate session
bus, covering reuse of an existing media stack. See
`portal-reused-media-results.json`.

Native DRM/KMS tests exercise the capture protocol at 1920×1080/143.981 Hz,
including both cursor modes and asymmetric image checks. See
`launcher-screencast-native-results.json`
and `screencopy-results.json`.

Single-window portal results record a real
PipeWire window stream while a different synthetic window covered the source
fullscreen. Exact test pixels passed with no occluder content. Independent
protocol tests cover both cursor modes, thirty
frames, invalid SHM buffers, off-workspace capture and closing the source window.

Desktop-audio sharing and separate cursor-metadata streams are not implemented.
Embedded and hidden cursor modes work. Firefox/Discord-specific UX and the physical
SDDM session still need a user session check. Diagnostics: `emberwm ctl screencast`
and `emberwm ctl status` (image_capture); failures go to the session log.

Protocol references: [official xdpw 0.8.1 implementation](https://github.com/emersion/xdg-desktop-portal-wlr/blob/v0.8.1/src/screencast/ext_image_copy.c)
and [Wayland protocols](https://gitlab.freedesktop.org/wayland/wayland-protocols/-/tree/main/staging/ext-image-copy-capture).
