# Rice profiles

A rice profile restores a complete Ember appearance: wallpaper and colours,
window borders, gaps/master ratio, panel layouts and visibility, fonts and native
popup sizes and compact notification appearance. **Super+Shift+T** cycles profiles live in standalone Ember;
**Alt+Shift+T** in the nested preview. **Super+T** still cycles theme palettes.

| Profile | Appearance |
| --- | --- |
| `red-dusk` | Abbey painting with native burgundy palette, tiny workspace/clock panel |
| `midnight` | Blue palette, three panels for workspaces, clock/date and status |
| `minimal` | Solid monochrome background, small gaps, no panels or wallpaper |

```sh
emberwm ctl rice list
emberwm ctl rice midnight
emberwm ctl rice minimal
emberwm ctl rice default
```

`rice default` restores the appearance specified by the active main config.
Profile switches update existing window placement and native UI live. They keep
window identity, workspace assignments, keyboard bindings/layout, terminal
commands, display settings, audio, Wi-Fi state and notification delivery settings.
Theme presets can adjust colours on top of a loaded rice.

## Save your current setup

```sh
emberwm ctl rice save my-look
emberwm ctl rice my-look
```

The snapshot includes current theme edits and panel visibility/layout, including
changes made after loading a profile. It is saved as `rices/my-look.toml` beside
the active main config; the command returns the exact path. If using the shipped
standalone defaults, that directory is `config/rices/` inside the Ember project.
With `~/.config/emberwm/config.toml`, it is `~/.config/emberwm/rices/`.

Saving creates a private file atomically and refuses to overwrite an existing
profile, symlink or built-in name. Use a new name or edit the existing TOML file.
It does not change the main config or select the saved profile. Snapshots use
absolute asset paths; the image and font files themselves are not copied.

Names use 1–48 letters, digits, underscores or hyphens. `red-dusk`, `midnight` and
`minimal` are built-in names. Command words such as `default`, `list` and `save`
are reserved. Custom profiles appear in the cycle automatically; malformed or
unavailable profiles are skipped by cycling and report an error on explicit load.

## Start with a profile

Add to the active main config:

```toml
[rice]
profile = "my-look"
```

Mod+R reloads the config and selected profile. This selector also applies at the
next login. It takes precedence over the main config's appearance fields and
`theme.preset`; those still form your original look when `rice.profile = ""`.
Live selections last for the current compositor session. The main config is
never rewritten automatically.

Personal configs copied before 0.11 need the shortcut added if desired:

```toml
[[bindings]]
key = "t"
shift = true
action = "rice-cycle"
```

Bindings may also use an action such as `rice minimal` for a specific profile.
The current selection and available names are shown by `emberwm ctl rice list`.
The current selection identifies the last full profile loaded; later palette or
panel edits are captured when saving.

## Custom profile files

Save the current setup first for a complete editable example, or create a file
like this under `rices/` beside the active config:

```toml
version = 1

[theme]
background = "#190e14"
active_border = "#ef5675"
inactive_border = "#512333"
selection = "#ff8199"
border_width = 2
wallpaper = "wallpaper.jpg"
wallpaper_mode = "fill"

[layout]
gap = 10
master_ratio = 0.58

[panels]
enabled = true
font = "builtin"
font_size = 12.0
height = 26
background = "#25131b"
foreground = "#f5dce4"
accent = "#ef5675"
border_color = "#512333"

[[panels.items]]
name = "main"
anchor = "bottom-center"
modules = ["workspaces", "clock"]
reserve_space = true

[launcher]
width = 640
font_size = 15.0
rows = 8
```

Omitted appearance fields use fixed defaults, making switches independent of the
previous profile. Relative PNG/JPG/JPEG wallpaper and font paths resolve from the
profile's directory. Global panel colours also style the native popups; per-panel
colour overrides still work. Existing applications keep their own UI themes and
terminal settings.

Supported sections: `theme` (appearance, no preset selector), `panels` (all panel
settings/items), `layout` (gap/master_ratio), `shortcuts`/`wifi` (width/font_size),
`launcher` (width/font_size/rows), and `notifications` (width/margin/gap).
Bindings, workspace count, app commands and service settings cannot be supplied
as top-level profile fields. Panel click actions remain part of panel items.

## Memory and validation

Profiles are read only when selected or saved. No watcher, resident profile
catalogue, thumbnails or inactive wallpapers/fonts are kept. Only the active
wallpaper texture and enabled panels use rendering resources. Parsed panel fonts
are also released after two idle seconds, while their rendered pixels remain. Minimal removes
panel buffers/font and panel polling while retaining the on-demand features.

Files are limited to 64 KiB. Format/version, settings and assets are checked before
committing a switch; an invalid profile leaves the current setup intact.

Integration tests cover complete replacement, saves and
no-overwrite behavior, assets, startup/reload, validation errors and repeated
switches. Native tests cover the actual shortcut,
existing-window relayout and thirty switches with no anonymous memory growth.
