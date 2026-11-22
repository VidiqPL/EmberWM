# Theme presets

For complete saved panel layouts, wallpaper, fonts and spacing, use
[rice profiles](RICE_PROFILES.md) (**Super+Shift+T**). Theme presets can adjust
colours on top of a loaded profile.

**Super+T** cycles presets live in standalone Ember; **Alt+T** in the nested
preview. Built-ins: `red`, `midnight`, `forest`, `mono`, `light`. The original red
wallpaper desktop stays the startup default. Built-in colour presets preserve
the current wallpaper, framing and wallpaper palette. Use the `minimal` rice or `wallpaper = ""` to remove the image for lower
memory use. Native panels, help, Wi-Fi, launcher, notifications and share
consent inherit the panel palette.

```sh
emberwm ctl themes
emberwm ctl theme midnight
emberwm ctl theme custom-red
emberwm ctl theme default
```

Live choices last for the current session. `theme default` restores appearance
from the active config; Mod+R reloads that config and its preset. No config file is
overwritten. A personal copied config needs the binding:

```toml
[[bindings]]
key = "t"
action = "theme-cycle"
```

To select a startup preset:

```toml
[theme]
preset = "midnight"
# Explicit fields in this main config override the preset at startup/reload.
# Remove existing colour/wallpaper fields if you want those values from the preset.
```

Custom presets live under `themes/` beside the active main config, or can be an
absolute TOML path. Example: [custom-red.toml](../config/themes/custom-red.toml).
A named custom preset appears in `ctl themes` and the cycle automatically.
Built-in names are reserved. Asset paths in custom presets resolve relative to
that preset file, including native PNG/JPG/JPEG wallpaper and panel fonts.

```toml
[theme]
background = "#101526"
active_border = "#7aa2f7"
inactive_border = "#303952"
selection = "#bb9af7"
border_width = 2
wallpaper = "wallpaper.jpg"
wallpaper_mode = "fill"

[panels]
font = "builtin"
font_size = 12.0
background = "#181f33"
foreground = "#c0caf5"
accent = "#7aa2f7"
border_color = "#303952"

[layout]
gap = 10
```

Presets accept desktop theme fields, panel appearance/geometry/items and layout
gap; they cannot alter terminal commands, keybindings, workspaces, audio or network
settings. Optional `panels.items` replaces the panel list. Per-panel colour
settings still override shared panel colours. Existing apps keep their own themes;
terminal colours remain controlled by terminal arguments/configuration.

Files are read on selection/reload, limited to 64 KiB and validated before a live
switch commits. No theme watcher, daemon, thumbnail index or preloaded collection
of wallpapers is retained. Only the active wallpaper texture is kept; unchanged
wallpaper sources avoid another decode/upload.

Custom-preset tests verify relative PNG paths, explicit
main-config overrides, resource validation, reload and atomic failures.
Native tests exercised the actual shortcut and
fifty palette switches: anonymous memory grew by 4 KiB in that run.
