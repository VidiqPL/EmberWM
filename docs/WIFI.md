# Native Wi-Fi controls

**Super+.** opens Wi-Fi controls in the standalone session. The nested default
is **Alt+.**. The popup uses the same native drawing and panel theme as the help
sheet. No separate application or permanent Wi-Fi UI daemon is launched.

Choose a network with the mouse, or Up/Down and Enter. Its details provide a
**Connect** or **Disconnect** button. For WPA Personal networks, type the password
in the masked field and press Enter or Connect. Leave the field blank to reuse
saved credentials. NetworkManager stores new connection profiles and passwords
according to its normal policy; Ember does not write its own password file.
New profiles are restricted to your login user, so they use the existing
permission to manage your own connections instead of requesting system-wide
profile access. Editing an older system-owned profile can still require extra
permission.

- **Backspace** removes one character; **Ctrl+U** clears the field.
- **Escape** returns from details, then closes the popup. Super+. toggles it off.
- Right-click, click outside, or use × to close it.
- **F5 / Refresh** requests a new Wi-Fi scan. Opening the popup shows the existing
  NetworkManager scan cache promptly. Refresh samples results after a short wait;
  scan completion depends on the adapter, so repeat Refresh if necessary.
- Scroll or Up/Down navigates long lists. Connected networks appear first.

Network work runs in a short-lived worker. Its operations cannot block the
compositor's input/render loop. The popup intercepts keyboard input while open,
so the focused application's window does not receive typed passwords. Passwords
are masked, excluded from IPC/status and logs, and never passed as process
arguments. The input storage is overwritten on back/close/submit. NetworkManager
receives passwords over the system D-Bus connection.

Closing releases the pixel buffer, font reference, network list and input.
An in-progress connect/disconnect completes in the background; closing does not
disconnect an established connection. Scan workers are canceled when closed.
No Wi-Fi polling or worker remains after a completed operation at idle.

## Configure

The shipped session config includes:

```toml
[wifi]
enabled = true
width = 540
font_size = 13.0

[[bindings]]
key = "period"
action = "wifi-toggle"
```

Use `[wifi] enabled = false` to disable the popup. Change `period` to change its
shortcut; the help sheet follows the binding after **Super+R**. If a custom
config supplies its own `bindings` array, add the binding there. Width accepts
320–1200 pixels; font size accepts 10–24. Colour and font follow `[panels]`
`background`, `foreground`, `accent`, `border_color` and `font`, even when the
panels themselves are hidden. Settings reload live.

Diagnostics: `emberwm ctl wifi`, `wifi-toggle`, `wifi-refresh`. Status includes
network and connection state, never password contents. Refresh via IPC only
operates while the popup is open.

## Requirements and limits

Uses the existing **NetworkManager** service and **nmcli** for discovery. Both
are already installed on this Gentoo machine. The logged-in user needs normal
NetworkManager permissions through the system's active-session policy. The UI
shows an error if the service is unavailable or access is denied; it does not
run sudo or request the system's administrator password.

Open, WPA/WPA2/WPA3 Personal and OWE networks can get new profiles. Enterprise
and WEP networks need an existing NetworkManager profile; this popup reuses
those profiles but does not edit their advanced authentication settings. Hidden
SSIDs, non-UTF-8 SSIDs, VPN configuration and Wi-Fi radio controls are not part of
this popup. A disabled/blocked radio or absent adapter is shown explicitly.

## Verification

See `wifi-backend-results.json` for real D-Bus
serialization against a private NetworkManager fixture, including saved
credentials, PSK replacement with DNS preservation, denied access and failed
activation. `wifi-results.json` covers the rendered nested
popup, live reload, disabling, cache reuse and repeated-open memory checks.

`tests/vm/wifi-smoke.py` exercises the real standalone KMS/libinput path using
virtual keys and mouse input. The fixture records client key events to confirm
that password entry stays in the popup. Test networks/passwords are synthetic.
Host network discovery is checked separately without connecting/disconnecting.
Connecting the physical adapter to a different network still needs a manual
session test.

After rebuilding in place, log out and select EmberWM again to load version
0.8.0. The SDDM launcher already uses the release binary at this project path.

NetworkManager references: [nmcli](https://networkmanager.dev/docs/api/latest/nmcli.html)
and [D-Bus API](https://networkmanager.dev/docs/api/latest/spec.html).
