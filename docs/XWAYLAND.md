# X11 compatibility and clipboard (0.14)

The reported Steam error came from the absence of an X server. Ember was also
stripping DISPLAY from commands, and the screenshot paste client was trying
to reach X11 rather than the native Wayland clipboard. A Wayland socket alone
cannot satisfy those clients.

Ember now allocates its own X11 sockets and publishes DISPLAY and XAUTHORITY
before starting applications. Terminals, configured exec commands, launcher
children and D-Bus-activated applications receive the session environment.
A native login creates /tmp/.X11-unix if no earlier X server created it.

The compositor owns an authenticated, rootless Xwayland instance. A tiny C
starter sleeps on the listen sockets until the first X11 connection, then
executes the installed /usr/bin/Xwayland (resolved from the original PATH).
It restores PATH before execution so xkbcomp and normal server helpers work.
Only the starter directory is searched when launching the starter; a missing
build helper cannot silently fall back to an unauthenticated server. TCP is
disabled, and a private 0600 Xauthority file contains a random session cookie.
The helper and its build output are required beside this in-tree release;
`emberwm --xwayland-helper-path` prints the required directory when packaging.

X11 windows enter Ember's existing window/layout model: rules match their
WM_CLASS and title, and tiling/floating, workspaces, fullscreen, scratchpad,
focus and capture use shared window/surface access. X11 menus and transient
windows float. Core compatibility is covered by real Xlib clients; complete
X11 window-manager protocol parity is not claimed.

Clipboard MIME offers are synchronized between Wayland and X11. Text and PNG
work in both directions, including INCR transfers for large images. Ember's
native screenshots remain lazily transferred PNG buffers. Regular-file and
memfd Wayland paste destinations use a temporary pipe relay because epoll
cannot watch files. Those workers are capped at four, use a 16 KiB buffer and
128 KiB stack, expire after five seconds without completing, and release their
resources on completion. Normal pipe destinations need no extra relay worker.
There is no resident clipboard manager or saved clipboard history.

An X server exit removes stale X11 windows/offers and starts a new lazy server
on the same display slot. Native windows and native clipboard sources survive.
Retries are delayed, with backoff capped at 30 seconds after repeated startup
failures. Existing X11 processes lose their server connection when it dies and
may need to be reopened; restarting a server cannot restore their process state.
The full X server can remain resident after first use; its RAM is additional
to the tiny starter's pre-use memory.

```toml
[xwayland]
enabled = true
```

This defaults to true. Change it before login; toggling server ownership is a
session-level setting, not a live theme change. Disabling it removes X11
compatibility, including X11-only clipboard clients. Requires installed
Xwayland and xkbcomp (already present on this Gentoo system).

Log out and back into the Ember session once to load this binary and export
its environment. Reopen Steam and the affected terminal/Codex process there;
already-running processes cannot inherit a new DISPLAY retroactively. Avoid
hardcoding DISPLAY=:0, which could belong to the greeter or another session.
`emberwm ctl status` reports the allocated display, readiness and startup error.
Ready=false before the first X11 connection is expected and saves RAM.

Reproduction: `cargo test --locked`, release build, build the test-only
`tests/x11-clipboard-probe` crate with its target directory set to
`artifacts/x11-probe-target`, and compile `tests/x11-window.c` and
`tests/x11-selection.c` with -lX11 -lXext into artifacts/. `tests/vm/build.py` builds
the Wayland fixture. Run `dbus-run-session -- python3 tests/xwayland-smoke.py`
for isolated nested coverage and `tests/vm/xwayland-smoke.py` against the
isolated VM fixture for native DRM and input coverage. The arboard probe is
not linked into Ember. Neither test touches the host clipboard.

Recorded nested results ·
Native results · [Smithay patch](../vendor/README.md).

A full Steam client login/game and the actual Codex UI paste were not run in
the user's active desktop. Tests prove the previously absent X11 services and
clipboard backend work, but cannot guarantee no application-specific failure
ever occurs.
