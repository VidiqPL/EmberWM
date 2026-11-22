# App launcher

**Super+Space** opens the launcher in the native session. Type a name or prefix
(`firefox`, `fir`, `fi`), then press Enter. Up/Down, Tab and the mouse wheel select
results; clicking a result launches it. Escape, right-click or a click outside
closes the launcher. Backspace edits the query; Ctrl+U clears it.

**Super+Shift+Space** now toggles floating mode. The live shortcut sheet includes
both bindings. The nested preview uses Alt instead of Super by default.

Matching prefers exact names and prefixes, then words, substrings and subsequences.
Within the same match strength, more-used apps appear first. Counts are learned
from successful launches through this launcher. With an empty query, frequent apps
come first. Counts and last-used timestamps are stored locally in
`~/.local/state/emberwm/launcher-usage.json` (or under `$XDG_STATE_HOME`). Remove
that file to reset ranking. No background usage monitor is started.

The launcher reads normal XDG desktop entries, including exported Flatpak apps.
User entries take precedence; Hidden/NoDisplay, TryExec and desktop visibility
settings are respected. GIO launches the selected desktop entry, preserving its
Exec field codes, argument quoting, working directory and launch semantics. `gio`
is already installed on this machine. New apps receive Ember's Wayland display,
desktop and control socket environment.

```toml
[launcher]
enabled = true
width = 640
font_size = 14.0
rows = 8

[[bindings]]
key = "space"
action = "launcher-toggle"

[[bindings]]
key = "space"
shift = true
action = "float"
```

Colours and font follow `[panels]`, matching help and Wi-Fi controls. Set
`enabled = false` to disable the launcher, or change its binding. Reload with
Super+R. A custom bindings array needs the new bindings added explicitly.

Desktop entries and usage data load when opened. Closing releases the app index,
ranking data, query, font reference and pixel buffer. A canceled scan stops early.
There is no resident launcher process, directory watcher or hidden launcher window.
Short-lived workers handle indexing and launch/history writes off the render loop.

Diagnostics: `emberwm ctl launcher` and `emberwm ctl launcher-toggle`.
Native keyboard, wheel, real desktop-entry launching and usage ranking are tested
in `launcher-screencast-native-results.json`.
