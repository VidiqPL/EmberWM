# Development checks

**Not recommended for daily use.** Passing these checks does not establish a stable desktop.

## Regular checks

```sh
cargo test --locked
cargo clippy --all-targets --locked -- -D warnings
cargo fmt --all --check
cargo build --release --locked
./target/release/emberwm --check-config --config config/session.toml
./scripts/install-session.sh --print
```

The published development snapshot passed 46 Rust tests; two opt-in tests were
ignored. This includes a real Unix-socket backpressure regression: the default
4 KiB queue disconnects a stalled reader; the configured queue preserves all
49,152 response bytes. The higher 1 MiB limit grows on demand.

The login-entry renderer was checked without changing system sessions, including
a checkout path containing spaces. The installer must be rerun after moving a
checkout. `--print` is read-only.

## Optional integration tests

Python integration scripts generally need Python 3, Pillow and the appropriate
Wayland/X11/GL development tools. Some use PyGObject and python-dbus. Run nested
D-Bus tests with a private session bus where the script requires it. Read each
script before use; most expect fixture binaries under ignored `artifacts/`.

`tests/vm/` contains a development QEMU/KVM harness. It copies a fixture root and
uses virtual input/GPU devices. The harness currently assumes Gentoo host library
paths, a readable host kernel in `/boot`, `/dev/kvm`, `wayland-scanner`, development
headers and software Mesa support. Library sonames may need adaptation to your
system. `fetch-tools.py` downloads Alpine test tools into `/tmp`; it does not
install host packages. These tests are not a portable installation method.

```sh
python3 tests/vm/fetch-tools.py
python3 tests/vm/build.py
# Run in a separate terminal:
./tests/vm/launch.sh
# Against that isolated VM:
python3 tests/vm/xwayland-smoke.py
# Stop your test VM afterward:
python3 tests/vm/control.py qmp quit
```

Some historical baseline switches require separately built older binaries;
those binaries are deliberately not published. The personal microphone/PTT
script and its local integration test are not included. No release workflow,
compiled fixture or prebuilt compositor is distributed.

## X11 workspace input regression

See [X11_INPUT.md](X11_INPUT.md) for the live-game frame-order regression,
its native virtual-seat test commands, and the exact scope of validation.
