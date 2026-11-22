# Native panels

EmberWM 0.4 includes compositor-native panels, inspired by the small workspace and
clock strip in the reference clip. No Waybar, browser engine or panel daemon is
required. The shipped configs enable a red, 22-pixel top-left workspace/clock
panel and include a disabled top-right system panel.

Edit `config/session.toml` for the standalone session, or your overriding
`~/.config/emberwm/config.toml` if you created one. `emberwm ctl status` reports the
active config path. Reload with **Super+R** (Alt+R in nested mode).

## Remove or toggle panels

Set `enabled = false` inside the existing `[panels]` section to remove everything.
This drops panel font references, pixel buffers, polling timers and any pending
volume query. A visible notification or shortcut sheet can still hold the shared
font until that overlay closes. Panels reserve no window space while disabled. Alternatively remove all
`[[panels.items]]` entries and put `items = []` under `[panels]`.

Disable one panel with `enabled = false` in its `[[panels.items]]` entry, or delete
that entry. **Super+B** temporarily toggles all panels; reloading restores the
saved setting. Configs from older Ember versions without a `[panels]` section
continue to run without panels.

## Add and style panels

Each `[[panels.items]]` creates another panel. Names must be unique; up to 16 are
supported. Module order is the order on screen. These examples can be added to
the end of your existing config:

```toml
[[panels.items]]
name = "date"
anchor = "top-center"
modules = ["date"]
date_format = "%a %d %b"
reserve_space = false

[[panels.items]]
name = "status-bottom"
anchor = "bottom-right"
modules = ["text:EMBER", "layout", "memory", "volume"]
height = 26
font_size = 12.0
margin = 10
background = "#24131b"
foreground = "#ead8dd"
accent = "#cf4763"
reserve_space = true

[[panels.items]]
name = "window-title"
anchor = "bottom-left"
modules = ["title"]
max_title_chars = 40
width = 360
on_click = "terminal"
```

Anchors: `top-left`, `top-center`, `top-right`, `bottom-left`, `bottom-center`,
`bottom-right`. `offset_x` and `offset_y` move a panel from its anchor, using
screen coordinates (positive means right/down). Place multiple panels on one
edge with different anchors or offsets; overlapping panels are not rearranged
automatically. Later entries draw on top of earlier entries.

`width = 0` sizes to content; `width = -1` stretches across the output minus
margins. A positive number fixes the width and clips overflowing content.
`reserve_space = true` keeps tiled windows below/above that panel's edge strip.
`false` overlays it. Floating windows can pass beneath panels. Fullscreen windows
use the whole output; panels hide by default.

Global settings live in the existing `[panels]` section, **before** its first
`[[panels.items]]` entry:

```toml
[panels]
enabled = true
font = "builtin"
font_size = 11.0
height = 22
margin = 8
padding = 8
spacing = 7
background = "#211317"
foreground = "#ead8dd"
accent = "#cf4763"
border_color = "#4c2835"
border_width = 0
opacity = 0.94
hide_empty_workspaces = true
hide_on_fullscreen = true
poll_interval_secs = 5
```

Panel entries can override `height`, `font_size`, `margin`, `background`,
`foreground` and `accent`. Colours use `#RRGGBB`. Opacity affects the background;
text remains opaque. Font sizes are 8–32 pixels, heights up to 128 pixels. The
font must fit within the panel height with at least four pixels to spare.

The bundled Ember Panel Mono (a small DejaVu Sans Mono subset) avoids an external
font lookup. It covers Latin (including Polish), Greek, Cyrillic, punctuation,
arrows and terminal shapes; unsupported glyphs use the font’s missing-glyph box.
`scripts/subset-panel-font.py` reproduces it using fonttools 4.60.1. To use another
TrueType/OpenType font, set `font` to its path; relative paths resolve beside the
config. No shell expansion is performed; use an absolute path instead of `~`.
Unusable fonts reject a reload and preserve the working configuration.

## Modules and controls

| Module | Display / interaction |
| --- | --- |
| `workspaces` | Active and occupied workspaces; click to switch, scroll to cycle. Set `hide_empty_workspaces = false` to show all. |
| `clock` | Local time, using per-panel `clock_format`, default `%H:%M`. |
| `date` | Local date, using per-panel `date_format`, default `%a %d %b`. |
| `title` | Focused window title, clipped to `max_title_chars` (default 32). |
| `layout` | Focused window's `TILE` / `FLOAT` state. |
| `memory` | Whole-system RAM used (`MemTotal − MemAvailable`), in MiB. |
| `cpu` | Whole-system CPU usage since the previous sample. Initial value is `--`. |
| `uptime` | System uptime in hours/minutes. |
| `volume` | Default PipeWire output volume; click to mute, scroll to adjust. Requires `wpctl`. |
| `text:your text` | Literal text; no commands are polled. |

Set `clock_format = "%H:%M:%S"` for seconds, or `%I:%M %p` for 12-hour time.
Formats use `strftime`. A panel's optional `on_click` is any existing Ember action
(for example `terminal`, `panels-toggle`, or `exec your-program`). Workspace and
volume controls take priority over that fallback action. No keyboard focus is
stolen simply by moving over a panel.

## Idle behavior and diagnostics

Unchanged text reuses rendered buffers. The default clock wakes once per minute;
seconds formats wake once per second. System counters poll only when their
modules are enabled, at `poll_interval_secs` (1–600). Only the volume module
briefly launches `wpctl get-volume`; queries have a two-second deadline and are
reaped on disable or shutdown. No commands run for plain text/title/workspaces.

`emberwm ctl panels` shows active panel rectangles, module text/actions, pixel
buffer bytes, font/polling state and the cumulative rasterization count. See
`panels-results.json` and `panels-native-results.json` for validation and measured
memory costs. The nested preview's RAM module includes the entire host desktop;
it is not a measurement of Ember alone.

Three optional native panels (development screenshot omitted)
