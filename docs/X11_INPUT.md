# X11 workspace input ordering (0.15.10)

A running game on another workspace could leave Steam visible but unable to
receive mouse input. Keyboard focus and Ember's rendered window order could be
correct while Xwayland still placed the hidden game's frame above Steam. This
also made the problem look like a game-crash recovery failure when no crash had
occurred.

Ember now synchronizes Xwayland's managed frame stack with its visible Space:
hidden clients are below visible clients, and visible frames follow the visual
stack. Synchronization runs on focus/layout changes and after XWM registers a
new mapped client. App-owned override-redirect menus keep their existing focus
policy. No background helper or polling service is needed.

Pointer delivery also reconciles its target before button and wheel events and
after managed focus changes. It respects active grabs and pointer locks. A
synthetic motion is sent only when the target surface changes, preserving the
wheel-only scrolling fix.

## Validation

`tests/vm/workspace-input.py` uses a native virtual seat, a managed X11 parent,
a separate live X11 game fixture and a native Wayland window. The saved 0.15.9
binary reproduces the incorrect root hit target and zero clicks reaching the
visible X11 parent. With 0.15.10, the root hit target matches the visible frame,
clicks reach it while the game remains alive, repeated workspace switches work
both ways, and a stationary switch to a native window delivers its click without
sending it to the hidden X11 app. The native stationary-switch check also passes
on the baseline; it is coverage, not independent proof of the reported Firefox
symptom.

Run this in the disposable VM prepared by `tests/vm/build.py` and booted with
`tests/vm/launch.sh`:

```sh
python3 tests/vm/workspace-input.py
python3 tests/vm/popup-focus.py
python3 tests/vm/game-pointer.py
python3 tests/vm/crash-focus.py
```

The baseline option requires a separately saved pre-fix executable installed as
`/usr/bin/emberwm-stacking-baseline` inside the guest. It is a development test
artifact, not distributed with the source. No real Steam game is crashed by
these tests. Reloading the compositor requires logging out and back in.
