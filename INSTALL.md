# Building and installing for experimentation

> **Not recommended for daily use.** EmberWM is an experimental personal
> compositor. Use a nested session or disposable VM for initial testing. Keep
> another working desktop installed. There are no public releases or prebuilt
> packages; build the source yourself.

The development environment is Gentoo Linux with Mesa graphics and libseat.
Other distributions and graphics/seat setups are not guaranteed to work.
Use a recent Rust/Cargo toolchain compatible with `Cargo.lock`, a C compiler,
`pkg-config`, Git, Python 3 and the native development headers below.

## 1. Dependencies

Core libraries: Wayland client/server/EGL, libinput, libudev, libseat,
libxkbcommon, libdrm, Mesa GBM/EGL/GLES and D-Bus. A working DRM/KMS graphics
stack and a configured libseat backend (elogind or seatd) are needed for native
sessions. Do not replace an existing working login/seat setup merely to try Ember.

Example Gentoo dependency installation, with USE flags adapted to your system:

```sh
sudo emerge --ask dev-lang/rust dev-util/pkgconf dev-vcs/git dev-lang/python \
  dev-libs/wayland dev-libs/libinput virtual/libudev sys-auth/seatd \
  x11-libs/libxkbcommon x11-libs/libdrm media-libs/mesa sys-apps/dbus \
  x11-terms/kitty x11-base/xwayland
```

Mesa must provide EGL, GLES and GBM support. The default terminal is Kitty;
configure another terminal if preferred. Xwayland is required for X11 apps and
is started lazily. Verify development-library discovery with:

```sh
pkg-config --exists wayland-client wayland-server wayland-egl libinput \
  libudev libseat xkbcommon libdrm gbm egl dbus-1
```

Optional desktop features use these runtime packages:

```sh
sudo emerge --ask media-video/pipewire media-video/wireplumber \
  net-misc/networkmanager sys-apps/xdg-desktop-portal \
  gui-libs/xdg-desktop-portal-wlr sys-apps/xdg-desktop-portal-gtk
```

Audio controls need `wpctl`; Wi-Fi controls need NetworkManager and appropriate
user authorization. Screen sharing needs PipeWire and portal backends. These
services have their own setup requirements. Optional features can be disabled
in the TOML config. Package references: [seatd/libseat](https://packages.gentoo.org/packages/sys-auth/seatd),
[libinput](https://packages.gentoo.org/packages/dev-libs/libinput).

## 2. Clone and build as your regular user

```sh
git clone https://github.com/VidiqPL/EmberWM.git
cd EmberWM
cargo build --release --locked
cargo test --locked
./target/release/emberwm --check-config --config config/ember.toml
./target/release/emberwm --check-config --config config/session.toml
```

Do not use `sudo cargo` or run Ember as root. On a memory-constrained build host,
use `cargo build --release --locked -j 2`. Keep the entire checkout: scripts,
config, assets, the release binary and the compiled lazy-Xwayland helper are
currently used in place. Copying only the executable is not an installation.
The checkout path cannot contain control characters, `=` or `%`.

## 3. Try a nested window first

From an existing Wayland desktop:

```sh
./scripts/run-nested.sh --config "$PWD/config/ember.toml"
```

The nested config uses **Alt** rather than Super. Alt+Enter opens a terminal;
Alt+Shift+Escape exits the nested compositor. Your host desktop may intercept
some shortcuts. A nested preview is a development test, not a native gaming
or refresh-rate benchmark.

## 4. Optional login-screen session entry

After building, review the generated descriptor and then install it:

```sh
./scripts/install-session.sh --print
sudo ./scripts/install-session.sh
```

Python 3 renders absolute paths for the current checkout; `desktop-file-validate`
is used when available. The installer writes only
`/usr/share/wayland-sessions/emberwm.desktop` and backs up an existing different
entry. It does not restart the display manager or log you out. The entry points
to this checkout, so reinstall it after moving the directory.

Log out of your working desktop and choose **EmberWM** in the login screen's
session selector, then sign in normally. Super+Enter opens the default terminal;
Super+Shift+Escape exits. The session wrapper starts its own D-Bus session and
may start audio services according to config. There is no session locker.

An experimental native session can also be started from an active local text
console login with `./scripts/start-session.sh`; device access still requires
a working seat setup. The wrapper refuses native startup inside a running
X11/Wayland desktop. Use nested mode there.

## 5. Optional portal configuration

Run this **as your normal user**, not with sudo:

```sh
./scripts/install-portals.sh
```

It installs Ember-specific portal preference/chooser files beneath
`~/.config` and `~/.local/share`, backing up changed existing files. Other
desktops keep their own portal preferences. Re-run it after moving the checkout.
Screen sharing remains experimental.

## Configuration

Use `--config /absolute/path/config.toml` or create
`~/.config/emberwm/config.toml`. Copying a shipped config changes the base
directory of its relative wallpaper/font paths; adjust those paths accordingly.
For a simple low-memory starting point:

```sh
mkdir -p ~/.config/emberwm
cp config/low-memory.toml ~/.config/emberwm/config.toml
```

This example uses a solid background and disables panels. Check changes with
`./target/release/emberwm --check-config --config ~/.config/emberwm/config.toml`.
The standalone config requests 144 Hz when available; set `[display].refresh_hz`
to `0` to prefer the monitor's preferred mode. No refresh rate or RAM target is
guaranteed on another machine.

## Recovery and removal

Session logs are in `~/.local/state/emberwm/session.log`. Keep an alternative
working session available in the login screen. From a local text console, the
control client can inspect or stop your own instance:

```sh
./target/release/emberwm ctl status
./target/release/emberwm ctl quit
```

With multiple Ember instances, specify `ctl --socket /path/to/ember.sock`.
Removing the login option does not delete the source or other desktops:

```sh
sudo rm /usr/share/wayland-sessions/emberwm.desktop
```

If you installed the optional portal files, remove only Ember's files (or restore
installer backups if they replaced your own configuration):

```sh
rm -f ~/.config/xdg-desktop-portal/ember-portals.conf
rm -f ~/.config/xdg-desktop-portal-wlr/Ember
rm -f ~/.local/share/xdg-desktop-portal/portals/ember.portal
```

Do not delete a checkout while an installed session entry still points at it.
