# Scratchpad (0.13)

**Super+Shift+G** marks the focused window. It becomes a floating window above
normal app windows, including fullscreen apps. **Super+G** hides the marked group;
press it again to show the same windows on your current workspace.

This follows the `magic` special-workspace idea in your Hyprland config. Marking a
window initially shows it. As of 0.15.5, marking preserves its current size and
position; tiled windows are no longer shrunk to a centered default rectangle. You can mark several windows and toggle them together.
The focused marked window stays above the other marked windows. Normal windows
can receive keyboard focus underneath the visible group without covering it.

Press **Super+Shift+G** again while a marked window is focused to remove it from
the group. It joins your current normal workspace and regains its previous tiled
or floating state; a previously floating window regains its saved geometry.
**Super+Shift+1…0** also removes the focused marked window from the group when
sending it to a normal workspace. Marking a fullscreen window first exits
fullscreen. Later fullscreen requests remain isolated from normal-workspace
fullscreen state. If you unmark above an existing fullscreen app, normal fullscreen
visibility rules apply and focus returns to a visible window. Both floating moves and resizing still work.

Hidden windows are unmapped and receive no keyboard/pointer focus. Hiding releases
game pointer locks and cancels client move/resize grabs. Showing restores focus to
the last focused marked window. Hiding restores the last suitable normal window
on the current workspace, or leaves keyboard focus empty on an empty workspace.
Closing the last member empties the group and drops saved window references.
Scratchpad-only windows do not leave occupied-workspace markers in the panel.

The apps continue running while hidden, keeping their own memory and state. The
feature itself adds a small saved placement per marked window and group focus
state; it launches no helper, timer, service or duplicate app. Config/theme/rice
reloads preserve the group during the session. Membership is not persisted across
logout. The existing Super+/ shortcut sheet lists both bindings automatically.

Shipped standalone and nested configs include:

```toml
[[bindings]]
key = "g"
shift = true
action = "scratchpad-mark"

[[bindings]]
key = "g"
action = "scratchpad-toggle"
```

Nested preview uses Alt instead of Super by default. Change/remove these bindings
in your config like any other action. Existing personal configs need these two
entries added; the main config in this session is the shipped `config/session.toml`.

IPC equivalents:

```sh
emberwm ctl scratchpad-mark
emberwm ctl scratchpad-toggle
emberwm ctl status
```

Status exposes the group's visibility/count and a `scratchpad` flag per window.
The `visible` flag reflects actual mapping; hidden members remain in the window
list for introspection without appearing in the desktop scene.

Native preview · Hidden ·
Functional results · Native input results.

Tests use real synthetic Wayland clients on a private nested compositor and on a
native VM's virtual keyboard/mouse. They cover pixels above normal and fullscreen
windows, hidden-window input isolation, cross-workspace toggles, restored geometry,
multiple members, rules/reload, generated help, application closure and bounded
RAM across 100 toggles. Physical application-specific behaviour is not established
by the VM test alone.

The new binary and shipped bindings load after logging out and back into Ember.
