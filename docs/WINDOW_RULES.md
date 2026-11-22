# Window rules, F11 and ten workspaces

EmberWM 0.6 adds event-driven window placement rules, handles apps' native
Wayland fullscreen requests, and provides ten workspaces in the shipped configs.
The standalone modifier is Super; the nested preview defaults to Alt.

## Workspace and fullscreen keys

| Key | Action |
| --- | --- |
| Super+1…9 | Select workspace 1…9 |
| Super+0 | Select workspace 10 |
| Super+Shift+1…9 | Move focused window to workspace 1…9 |
| Super+Shift+0 | Move focused window to workspace 10 |
| F11 | Toggle the focused window's fullscreen state |
| Super+F | Also toggle fullscreen |
| Super+/ | Show the live shortcut sheet |
| Super+R | Reload config |

F11 is a regular binding with `modifier = false`, so it works without Super.
Entering fullscreen removes Ember's borders and reserved panel space; exiting
restores the floating rectangle or recalculates the tiled layout. Repeated app
fullscreen requests preserve the original floating rectangle.

The default F11 binding consumes the key. To let an app handle F11 itself,
remove this binding block from the config:

```toml
[[bindings]]
key = "F11"
action = "fullscreen"
modifier = false
```

App-originated `xdg_toplevel.set_fullscreen` and `unset_fullscreen` requests are
now handled too. A client on another workspace receives the correct configure
state without switching the current workspace. Each workspace has one active
fullscreen window; making another fullscreen restores the previous window there.

The default workspace count is 10, configurable from 1 to 10 in `[layout]`.
Internally and in `ctl`, workspace 10 is called `10`; the keyboard uses the `0`
key for it. Panels and the shortcut sheet derive their contents from the config.

## Configure rules

Edit `config/session.toml`, or your overriding `~/.config/emberwm/config.toml`.
`ctl status` reports the active path. Each `[[window_rules]]` adds a rule.
The shipped config contains an **inactive** Sober example; set its
`enabled = true` to send Sober to workspace 10 and follow it there.
No enabled app-specific placement rule is imposed by default.

```toml
[[window_rules]]
name = "Roblox workspace"
app_id = "org.vinegarhq.Sober"
workspace = 10
follow = true
# fullscreen = true  # optional: start fullscreen as well

[[window_rules]]
name = "Floating settings"
app_id = "org.example.*"
title = "*Settings*"
width = 720
height = 480
center = true

[[window_rules]]
name = "Music window"
app_id = "my-player"
workspace = 3
floating = true
width = 600
height = 400
x = 80
y = 100
```

Use `ctl status` to inspect actual window app IDs/titles. Sober's installed
Flatpak ID was verified as `org.vinegarhq.Sober`; an app can expose a different
window identity, so the live status remains the reference for matching.

| Field | Meaning |
| --- | --- |
| `name` | Optional label. |
| `enabled` | Defaults to true; false disables the rule. |
| `app_id`, `title` | Match one or both; both supplied predicates must match. |
| `workspace` | Destination workspace number, 1–10, within the configured count. |
| `follow` | True switches to that workspace and focuses the window. Defaults to staying on the current workspace. |
| `floating` | True floats the window; false tiles it. |
| `fullscreen` | Initial fullscreen/windowed state. |
| `width`, `height` | Content size in pixels, 64–8192. Implies floating mode. |
| `x`, `y` | Content position in output pixels, 0–16384. Implies floating mode. |
| `center` | True centers within the work area and implies floating mode. |
| `scroll_factor` | Wheel scale for matching windows, 0.1–10. Overrides the X11 base scale; last matching value wins. |

Match strings use case-sensitive glob matching: `*` matches any run and `?` one
Unicode character. Other characters are literal. A rule needs at least one
matcher. Up to 64 rules are supported; matcher strings are limited to 256 bytes.
Rules are combined in config order, with the last provided value for each
property taking priority. A later position overrides centering and vice versa.
Rule dimensions/positions are clamped to the usable output area; client minimum
sizes may impose further limits. Use `center = true` or `x`/`y` within one rule.
Geometry combined with `floating = false` in that same rule is rejected.

## Timing and reload behavior

Placement is resolved at the first toplevel commit, after the usual initial
app ID/title requests and before the initial window configure. A background
window can initialize while hidden, preserving the foreground fullscreen app.
Later app-ID/title changes can trigger a previously unmatched rule.

Each matching rule is applied once per window/configuration. Subsequent title
updates do not undo manual window moves or fullscreen toggles. When a new rule
first matches, the currently matching rules are combined in order. Changing the
rule list and pressing Super+R re-evaluates existing windows. Unrelated colour,
panel or shortcut reloads preserve their manual placement.

To explicitly reapply unchanged rules:

```sh
./target/release/emberwm ctl rules-apply
```

Removing/disabling a rule stops future application; it leaves the existing
window placement in place. Drawn terminals retain their explicit rectangle,
workspace and floating/windowed state and are excluded from window rules.

There is no rule polling or regex runtime. Matching borrows existing app
metadata and uses a fixed 64-bit applied-rule set per window. `ctl status`
reports `workspace_count`, each window's `placement_ready`, `rules_applied`
count, and whether it was `drawn`.

## Validation scope

Tests cover real Wayland clients, initial and late identity matching, rule
precedence, manual overrides, changed/invalid reloads, background configure,
fullscreen entry/exit and drawn-terminal precedence. Native virtual keyboard
events verify all ten workspace keys, Super+Shift+0, compositor F11 and an
app handling F11 then making its own fullscreen requests. The test client uses
the Sober app ID; actual Roblox/Sober gameplay was not launched or measured.

See `rules-results.json`, `rules-native-results.json` and `VALIDATION.md`.

## Per-app scroll speed (0.15.8)

Scroll factors are evaluated against the window receiving each axis event, so
config reload immediately updates them without reapplying placement rules.
Matching remains case-sensitive; use the actual app ID from `ctl status`.
The standalone config includes:

```toml
[[window_rules]]
name = "Steam scroll speed"
app_id = "steam"
scroll_factor = 1.5
```

This changes Steam's wheel distance while other apps use their normal base scale.
Animation remains the application's responsibility; Steam's own smooth-scrolling
option is enabled separately. Reloading an older Ember version will reject this
new field; first log into 0.15.8.
