# EmberWM control center (0.16.0)

Super+`.` opens one native menu with an EmberWM header and Wi-Fi, Tray and Power
tabs. Super+`,` opens its Tray tab directly. Existing `wifi-toggle` actions remain
an alias for the shared menu, so older configurations continue to work.

Use Ctrl+Tab / Ctrl+Shift+Tab or Alt+1/2/3 to switch visible tabs. Escape backs out
of Wi-Fi network details or a power confirmation, then closes the menu. Clicking
outside closes it. Application input and game capture are suspended while the
menu is open; Wi-Fi passwords are cleared when switching away from that tab.

Wi-Fi retains scanning, saved credentials, password entry and disconnect controls.
Tray retains activation, right-click menus, disabled entries and nested submenus.

## Configuration

```toml
[menu]
enabled = true
title = "EmberWM"
width = 640
height = 620
tabs = ["wifi", "tray", "power"]
```

Remove or reorder names in `tabs` to customize the menu. An empty list or
`enabled = false` disables it. `[wifi].enabled` and `[tray].enabled` also hide
those tabs. Colors and the font follow `[panels]`; menu title and dimensions are
included in rice profiles. Menu dimensions control the shared body; older Wi-Fi
and tray width settings no longer determine its size. Reload config to apply.

The suggested default binds are:

```toml
[[bindings]]
key = "period"
action = "menu-toggle"

[[bindings]]
key = "comma"
action = "tray-toggle"
```

`power-toggle` is also available for a custom shortcut. IPC commands `menu` and
`power` report the current state without changing it.

## Power controls

The three CPU preferences are Performance (`performance`), Balanced
(`balance_performance`) and Power saver (`power`). The fixed-purpose helper selects
a compatible governor and energy preference on every CPU policy. It preflights
all policies before writing and attempts rollback on partial failure. It never
writes frequency limits, boost controls, voltages or GPU configuration. A CPU
without the required EPP interfaces reports that the preference is unavailable.

Install the optional helper once for profile switching:

```sh
sudo sh scripts/install-power-helper.sh
```

This installs `/usr/local/libexec/emberwm-power` root-owned, plus a polkit action.
Preferences apply to the current boot. It does not start or enable TuneD, TLP, power-profiles-daemon or cpupower. Profile
changes use system authentication. If no agent is present, Ember tries the
installed KDE polkit agent for that operation and reaps its own agent afterward.
Other desktops need a working authentication agent of their own. The menu closes
before authorization so its input grab cannot block the password dialog. A
notification reports the result. Choices apply to the current boot and are not
saved as a startup policy.

The Power tab also shows CPU temperature and AMD GPU mode, temperature and
reported power where the kernel exposes them. GPU clocks remain under the
existing driver policy. These readings do not establish workload performance.

Logout, Suspend, Restart and Shut down each require an in-menu confirmation.
Suspend/restart/shutdown use the existing `org.freedesktop.login1` service
(elogind on Gentoo/OpenRC), with interactive authorization and normal inhibitor
handling. Unsupported actions or denied authentication report an error.

## Resource use and testing

Only the selected tab has an active UI. Closing drops pixel buffers, fonts and
Wi-Fi password storage; power sensors refresh every three seconds only while
Power is open. No power refresh timer or worker remains after a closed read
finishes. An already authorized mutation is allowed to finish and report its
result. The existing event-driven tray watcher remains available for app
registrations.

```sh
python3 tests/power-helper.py
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
# In the isolated QEMU fixture described in TESTING.md:
python3 tests/vm/control-center.py
# On a private session bus in an existing Wayland desktop:
dbus-run-session -- python3 tests/tray-volume-smoke.py
```

The helper test uses synthetic sysfs and injects a second-policy write failure.
Native menu tests use fake Wi-Fi and never confirm a real shutdown or suspend.
Real hardware profile authorization and suspend/resume need separate checks.

Protocol references: [polkit pkexec](https://www.freedesktop.org/software/polkit/docs/latest/pkexec.1.html),
[login1](https://www.freedesktop.org/software/systemd/man/latest/org.freedesktop.login1.html),
[AMD CPU preferences](https://docs.kernel.org/admin-guide/pm/amd-pstate.html).

To remove the optional helper, delete `/usr/local/libexec/emberwm-power` and
`/usr/share/polkit-1/actions/org.emberwm.power.policy` as administrator. No power
service needs disabling.
