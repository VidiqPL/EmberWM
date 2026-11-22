# EmberWM

> **Experimental. Not recommended for daily use.**
> This is a personal Wayland compositor project, published for source inspection
> and experimentation. It is incomplete and can lose application windows or
> input connections. It is not intended for production or dependable desktop use.

This repository provides **source code only**. There are no public releases,
prebuilt binaries or supported distribution packages. Version numbers in Cargo
and development notes identify revisions; they are not stability guarantees.

EmberWM is written in Rust using a vendored, modified Smithay 0.7. It has a
standalone DRM/KMS session and a nested mode for development inside another
Wayland desktop.

## Installation for experimentation

See **[INSTALL.md](INSTALL.md)** for dependencies, building from source, a nested
preview, adding the login-screen entry, optional portals and removal steps.
Test the nested mode or a disposable VM first. Keep a working desktop available;
Ember currently has no session locker and supports only one active display.

```sh
git clone https://github.com/VidiqPL/EmberWM.git
cd EmberWM
# Install the native development libraries listed in INSTALL.md first.
cargo build --release --locked
./target/release/emberwm --check-config --config config/ember.toml
./scripts/run-nested.sh --config "$PWD/config/ember.toml"
```

## Implemented features

- Keyboard-driven tiling/floating windows, ten workspaces, window rules and F11 fullscreen.
- Draw a rectangle to launch a terminal; mark and hide/show scratchpad windows.
- Configurable colours, rice profiles, panels and native PNG/JPEG wallpapers.
- On-demand application launcher, shortcuts, Wi-Fi controls and notification UI.
- PipeWire audio startup, media keys, volume display and StatusNotifier tray menus.
- Region screenshots copied to the clipboard and experimental portal screen/window sharing.
- Relative mouse capture and on-demand Xwayland with text/image clipboard bridging.

These are implemented features, not a promise of complete compatibility.
Multi-monitor layouts, fractional scaling, layer-shell desktop integration,
session locking, IME/primary-selection support and some popup/input protocols
remain incomplete or unsupported. Screen sharing and gaming are experimental.

The low-RAM goal is a design target. **There is no guaranteed whole-system idle
RAM figure**; application use, graphics drivers and session services dominate
measurements. Hidden UI resources are released where practical, and Xwayland
starts when an X11 client first needs it.

## Configuration and controls

Standalone defaults: [`config/session.toml`](config/session.toml).
Nested defaults: [`config/ember.toml`](config/ember.toml).
A per-user `$XDG_CONFIG_HOME/emberwm/config.toml` (usually
`~/.config/emberwm/config.toml`) overrides defaults; `--config PATH` is explicit.
Wallpaper paths are relative to the config file, so adjust them when copying it.

The standalone modifier is **Super**; the shipped nested config uses **Alt**.

| Shortcut | Action |
| --- | --- |
| Mod+Enter | Terminal |
| Mod+D, release, then drag | Draw a terminal window |
| Mod+Space | Application launcher |
| Mod+J / Mod+K | Focus next / previous |
| Mod+1…9 / Mod+0 | Workspace 1…10 |
| Mod+Shift+1…9 / Mod+Shift+0 | Move to workspace |
| Mod+Shift+Space | Toggle floating |
| Mod+left/right drag | Move / resize |
| F11 | Fullscreen |
| Mod+Shift+G / Mod+G | Mark scratchpad / hide or show it |
| Mod+Shift+S | Select screenshot region |
| Mod+/ | Shortcut sheet |
| Mod+. / Mod+, | Wi-Fi / tray menu |
| Mod+T / Mod+Shift+T | Theme / rice profile |
| Mod+R | Reload config |
| Mod+Escape | Release mouse capture |
| Mod+Shift+Escape | Exit Ember |

Bindings and optional features can be changed or disabled in TOML. Feature guides
are in [`docs/`](docs/). The optional push-to-talk supervisor can run a separately
supplied script; no personal mouse/PTT script is included.

## Development checks

```sh
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --all --check
./target/release/emberwm --check-config --config config/session.toml
```

The current development snapshot passed 46 unit tests (two opt-in tests ignored)
and targeted native virtual-seat checks. That does **not** establish suitability
for daily use or universal application/hardware support. Optional integration
tests need additional tools; see [testing notes](docs/TESTING.md).

No release/publishing workflow is provided. Bugs and incomplete behavior are
expected; source-level experimentation is the purpose of this repository.

## Credits and licensing

MIT licensed; see [`LICENSE`](LICENSE) and [`NOTICE`](NOTICE). Smithay Smallvil
scaffolding and modified Smithay sources retain their upstream notices. The font
subset has its own license in [`assets/fonts/`](assets/fonts/). Wallpaper sources
and attribution are listed in [`assets/WALLPAPER-SOURCES.md`](assets/WALLPAPER-SOURCES.md).
