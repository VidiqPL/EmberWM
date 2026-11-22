# Code and allocation audit: 0.16.1

Scope: renderer/wallpaper ownership, UI and font lifetimes, launcher/tray/network
workers, clipboard transfers, power sampling, panels, input reconciliation and
capture buffer lifetimes. This is a focused review, not an exhaustive security
audit of every protocol or a whole-system memory tuning pass.

## Changes

- Power samples compare the values actually displayed before invalidating cached
  pixels. CPU/GPU readings retain full precision in IPC; integer temperature and
  wattage labels only redraw when their displayed values change. A background
  read no longer paints a transient busy state and a replacement panel each cycle.
- Closing a power tab while its worker is pending requests heap reclamation again
  when the result is consumed, after those temporary allocations can be dropped.
- Unrelated panel and notification updates reconcile pointer focus without sending
  redundant motion. The comparison includes both the target surface and its
  global origin, so moving the same surface under a stationary pointer still
  delivers corrected local coordinates. A read-only Smithay accessor exposes the
  existing focus/origin tuple.
- Panel workspace occupancy uses a fixed 10-workspace bit mask instead of a fresh
  heap vector on every input/frame update. Closed tray menus drop their row-vector
  allocation as well as the already released row strings and pixel buffer.

## Existing memory behavior retained

Wallpaper pixels are discarded after GPU upload. Closed launcher/Wi-Fi/menu/help
UIs release large buffers and fonts. The font cache is weak and panels evict their
idle font geometry. Clipboard screenshot storage keeps one compressed PNG, shared
by bounded transfer workers. Capture readbacks are scoped to active requests.
No permanent monitoring helper or new idle timer was introduced.

The live hardware session sampled during this review was still 0.15.10 and used
approximately 41 MiB proportional set size (PSS), with approximately 23 MiB
anonymous PSS. Those readings are not measurements of 0.16.1, total desktop RAM,
or dedicated GPU VRAM. The changes primarily reduce allocation peaks and redraw
work; a substantial reduction in closed-desktop idle PSS is not established.

## Validation

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --all --check
python3 tests/vm/ui-pointer-refresh.py
python3 tests/vm/power-refresh.py
python3 tests/vm/control-center.py
```

The first two VM tests accept `--baseline` when a separately saved 0.16.0 executable
is installed as `/usr/bin/emberwm-audit-baseline` in the disposable guest. The power
test bind-mounts synthetic CPU files inside that guest; it never changes host CPU
settings. Process PSS numbers from the software-GPU VM include that renderer and
are not directly comparable with the hardware desktop. No old executable or
prebuilt release is published.

## Recorded VM comparison

The saved 0.16.0 executable produced four redundant frames during seven seconds
of unchanged synthetic power readings; 0.16.1 produced zero. Both repainted a
changed preference and stopped periodic redraws with the menu closed. In this
single matched pair, anonymous PSS before opening the menu was identical
(26,116 KiB). With the Power tab open it was 35,864 KiB before versus 29,880 KiB
after. These software-GPU figures illustrate reduced allocation churn; they are
not a general hardware idle-memory saving guarantee.

The pointer regression reproduced one unnecessary motion on notification show
and dismissal with the old build. The new build delivered zero for both, while
still updating local coordinates after panel removal moved the focused window.
A notification appearing directly under the pointer still received its click
without leaking input to the underlying app.

49 Rust tests passed (two opt-in tests ignored), as did Clippy, formatting,
configuration checks, native control-center lifecycle/auth-denial tests and game
capture checks. The existing font-eviction and screenshot/clipboard checks also
remain part of the functional verification.

The screenshot regression also exposed an old test expectation that used floating
point division for the pattern midpoint. The C fixture uses integer division;
the expected row now follows that rule so odd-height outputs are checked correctly.
