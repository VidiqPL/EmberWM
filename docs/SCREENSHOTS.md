# Region screenshots to clipboard

EmberWM 0.7 adds **Super+Shift+S** in the standalone config (**Alt+Shift+S** in the
nested preview). Press the shortcut, then left-drag a rectangle and release to
copy the selected pixels as `image/png`. Paste with **Ctrl+V** into an app that
accepts images. Dragging in either direction works. Escape or right-click cancels
and preserves the existing clipboard. Tiny selections are ignored.

This reproduces the existing Hyprland binding from
`~/.config/hypr/hyprland.lua`: `grim -g "$(slurp)" - | wl-copy`.
Ember uses its own renderer, selection gesture and Wayland clipboard, so grim,
slurp and a persistent wl-copy helper are not needed. The selection outline and
mouse cursor are excluded from the copied image. A confirmation appears after
capture. No screenshot is automatically saved to disk.

The shortcut is a regular configurable binding:

```toml
[[bindings]]
key = "s"
shift = true
action = "screenshot-region"
```

The live shortcut sheet picks it up from the config. Existing custom configs can
add this binding manually. The release binary contains new compositor code, so
an already-running older Ember needs one logout/login to use it. The current
user session and mouse PTT were not restarted during development.

Diagnostic commands:

```sh
./target/release/emberwm ctl screenshot-region
./target/release/emberwm ctl screenshot-region 100 100 400 240
./target/release/emberwm ctl screenshot /tmp/full-screen.png
```

The last command retains the existing full-screen PNG file behavior.
`ctl status` reports `screenshot_armed` and image clipboard byte/transfer counts.

Only the compressed PNG is retained for clipboard ownership, capped at 16 MiB.
A normal application copy replaces it and releases Ember's image reference.
Temporary readback/encoding allocations are reclaimed after rendering settles.
Paste transfers use short-lived bounded workers (at most four, five-second
backpressure deadline); none run while idle. Clipboard contents persist until
replaced or the compositor exits, and are not stored as notification history.

Validation uses a private native VM with virtual input and a real Wayland
clipboard consumer. Super+Shift+S plus a reversed mouse drag produced an exact
396×258 PNG whose pixels matched the selected test window, including the edges,
with no selection border/cursor. Escape preserved the previous clipboard;
repeated copies remained pasteable, workers exited, and a client text copy
replaced the image. The host clipboard and desktop contents were not captured.
See `screenshot-results.json` and `VALIDATION.md`.

## 0.7.1 orientation correction

Native scanout and a nested window have different framebuffer row order. The
readback now normalizes each target before cropping. The old solid-colour test
missed a vertical flip; an asymmetric coordinate pattern now checks every pixel
in top and bottom regions, plus full-file screenshot orientation. The test
reproduced the 0.7 failure before the correction. Nested-window normalization
retains its existing orientation.
