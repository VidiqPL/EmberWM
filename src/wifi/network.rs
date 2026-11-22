//! NetworkManager access runs in a short-lived worker, never on the render thread.
use super::{Network, Secret, Snapshot};
use dbus::{
    arg::{PropMap, RefArg, Variant},
    blocking::{stdintf::org_freedesktop_dbus::Properties, Connection},
    Path,
};
use std::{
    collections::HashMap,
    io::Read,
    os::fd::AsRawFd,
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::{Duration, Instant},
};

/// nmcli's terse output escapes both ':' and '\\', including inside SSIDs.
pub(super) fn fields(line: &str) -> Vec<String> {
    let mut result = vec![String::new()];
    let mut escaped = false;
    for c in line.chars() {
        if escaped {
            result.last_mut().unwrap().push(c);
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if c == ':' {
            result.push(String::new());
        } else {
            result.last_mut().unwrap().push(c);
        }
    }
    if escaped {
        result.last_mut().unwrap().push('\\');
    }
    result
}

fn nmcli(args: &[&str], cancel: &AtomicBool) -> Result<String, String> {
    let mut child = Command::new("nmcli")
        .args([
            "--colors", "no", "--wait", "8", "--terse", "--escape", "yes",
        ])
        .args(args)
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|_| "NetworkManager tools unavailable: install nmcli.".to_string())?;
    let mut stdout = child.stdout.take().unwrap();
    let fd = stdout.as_raw_fd();
    // Read while the child runs so a large AP list cannot fill its pipe and deadlock.
    let flags = unsafe { libc::fcntl(fd, libc::F_GETFL) };
    if flags < 0 || unsafe { libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK) } < 0 {
        let _ = child.kill();
        let _ = child.wait();
        return Err("Cannot read network list.".into());
    }
    let deadline = Instant::now() + Duration::from_secs(12);
    let mut output = Vec::new();
    loop {
        let mut buf = [0; 4096];
        loop {
            match stdout.read(&mut buf) {
                Ok(0) => break,
                Ok(n) => {
                    if output.len() + n > 262144 {
                        let _ = child.kill();
                        let _ = child.wait();
                        return Err("Network list is too large.".into());
                    }
                    output.extend_from_slice(&buf[..n]);
                }
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err("Cannot read network list.".into());
                }
            }
        }
        if cancel.load(Ordering::Relaxed) || Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err("Network scan canceled or timed out. Try Refresh.".into());
        }
        match child.try_wait() {
            Ok(Some(status)) if status.success() => {
                // Drain any bytes written between the read and try_wait.
                let _ = stdout
                    .take(262144 - output.len() as u64)
                    .read_to_end(&mut output);
                return Ok(String::from_utf8_lossy(&output).into_owned());
            }
            Ok(Some(_)) => return Err(
                "NetworkManager unavailable or access denied. Check the service and Wi-Fi radio."
                    .into(),
            ),
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("Cannot query NetworkManager.".into());
            }
            Ok(None) => thread::sleep(Duration::from_millis(20)),
        }
    }
}

pub(super) fn scan(rescan: bool, cancel: &AtomicBool) -> Result<Snapshot, String> {
    let devices = nmcli(
        &["--fields", "DEVICE,TYPE,STATE", "device", "status"],
        cancel,
    )?;
    let radio = nmcli(&["radio", "wifi"], cancel)?.trim() == "enabled";
    let mut snapshot = Snapshot {
        radio,
        ..Default::default()
    };
    for row in devices
        .lines()
        .map(fields)
        .filter(|v| v.len() == 3 && v[1] == "wifi")
        .take(8)
    {
        let device = &row[0];
        snapshot.devices.push(device.clone());
        if !radio || row[2] == "unavailable" || row[2] == "unmanaged" {
            continue;
        }
        if rescan {
            // Request a scan separately. Waiting for nmcli list --rescan=yes can hang on
            // adapters which do not report scan completion. Cached APs remain useful.
            let _ = nmcli(&["device", "wifi", "rescan", "ifname", device], cancel);
            for _ in 0..10 {
                if cancel.load(Ordering::Relaxed) {
                    return Err("Network scan canceled.".into());
                }
                thread::sleep(Duration::from_millis(100));
            }
        }
        let aps = nmcli(
            &[
                "--fields",
                "IN-USE,SSID,BSSID,SIGNAL,SECURITY,DBUS-PATH",
                "device",
                "wifi",
                "list",
                "ifname",
                device,
                "--rescan",
                "no",
            ],
            cancel,
        )?;
        snapshot.networks.extend(parse_aps(&aps, device));
    }
    snapshot.networks.sort_by(|a, b| {
        b.active
            .cmp(&a.active)
            .then(b.signal.cmp(&a.signal))
            .then(a.ssid.cmp(&b.ssid))
    });
    // Prefer an active or stronger BSSID, while keeping adapters/security types distinct.
    let mut seen = std::collections::HashSet::new();
    snapshot
        .networks
        .retain(|n| seen.insert((n.device.clone(), n.ssid.clone(), n.security.clone())));
    snapshot.networks.truncate(128);
    Ok(snapshot)
}

fn parse_aps(output: &str, device: &str) -> Vec<Network> {
    output
        .lines()
        .filter_map(|line| {
            let v = fields(line);
            if v.len() != 6
                || v[1].is_empty()
                || v[1] == "--"
                || !v[5].starts_with("/org/freedesktop/NetworkManager/AccessPoint/")
            {
                return None;
            }
            Some(Network {
                active: v[0] == "*",
                ssid: v[1].clone(),
                bssid: v[2].clone(),
                signal: v[3].parse::<u8>().ok()?.min(100),
                security: v[4].clone(),
                device: device.into(),
                path: v[5].clone(),
            })
        })
        .collect()
}

type Settings = HashMap<String, PropMap>;
const NM: &str = "org.freedesktop.NetworkManager";
const ROOT: &str = "/org/freedesktop/NetworkManager";
fn variant<T: RefArg + 'static>(v: T) -> Variant<Box<dyn RefArg>> {
    Variant(Box::new(v))
}
fn prop<'a>(s: &'a Settings, group: &str, name: &str) -> Option<&'a dyn RefArg> {
    Some(&*s.get(group)?.get(name)?.0)
}
fn manager_error(e: dbus::Error) -> String {
    // Never surface raw messages: an auth error can echo a supplied secret.
    if e.name().is_some_and(|n| {
        n.contains("Denied") || n.contains("NotAuthorized") || n.contains("Permission")
    }) {
        "Permission denied. Use an active local login with NetworkManager permissions.".into()
    } else {
        "NetworkManager could not complete the request. Check the password and try again.".into()
    }
}

pub(super) fn connect(
    network: &Network,
    password: Secret,
    cancel: &AtomicBool,
) -> Result<(), String> {
    let request_deadline = Instant::now() + Duration::from_secs(30);
    let bus = Connection::new_system().map_err(manager_error)?;
    let manager = bus.with_proxy(NM, ROOT, Duration::from_secs(3));
    let (device,): (Path<'static>,) = manager
        .method_call(NM, "GetDeviceByIpIface", (&network.device,))
        .map_err(manager_error)?;
    let ap = Path::new(network.path.clone()).map_err(|_| "Invalid access point.".to_string())?;
    let settings_proxy = bus.with_proxy(NM, format!("{ROOT}/Settings"), Duration::from_secs(3));
    let (profiles,): (Vec<Path<'static>>,) = settings_proxy
        .method_call(
            "org.freedesktop.NetworkManager.Settings",
            "ListConnections",
            (),
        )
        .map_err(manager_error)?;
    let mut saved = None;
    for path in profiles.into_iter().take(256) {
        if cancel.load(Ordering::Relaxed) || Instant::now() >= request_deadline {
            return Err("Connection canceled.".into());
        }
        let proxy = bus.with_proxy(NM, path.clone(), Duration::from_secs(3));
        let Ok((settings,)): Result<(Settings,), _> = proxy.method_call(
            "org.freedesktop.NetworkManager.Settings.Connection",
            "GetSettings",
            (),
        ) else {
            continue;
        };
        let matches_ssid = prop(&settings, "802-11-wireless", "ssid")
            .and_then(|v| v.as_iter())
            .map(|it| {
                it.filter_map(|v| v.as_u64().map(|n| n as u8))
                    .collect::<Vec<_>>()
            })
            .is_some_and(|v| v == network.ssid.as_bytes());
        let matches_device = prop(&settings, "connection", "interface-name")
            .and_then(|v| v.as_str())
            .is_none_or(|v| v.is_empty() || v == network.device);
        let key_mgmt = prop(&settings, "802-11-wireless-security", "key-mgmt")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let security_matches = if network.personal() {
            matches!(key_mgmt, "wpa-psk" | "sae")
        } else if network.enterprise() {
            key_mgmt.contains("eap") || key_mgmt == "ieee8021x"
        } else if network.security.contains("WEP") {
            key_mgmt == "none"
        } else if network.security.contains("OWE") {
            key_mgmt == "owe"
        } else {
            key_mgmt.is_empty()
        };
        if matches_ssid && matches_device && security_matches {
            saved = Some((path, settings));
            break;
        }
    }
    let active: Path<'static> = if let Some((path, mut settings)) = saved {
        if !password.is_empty() {
            if !network.personal() {
                return Err("This network needs an existing advanced connection profile.".into());
            }
            let security = settings
                .entry("802-11-wireless-security".into())
                .or_default();
            security.insert("psk".into(), variant(password.as_str().to_string()));
            security.insert("psk-flags".into(), variant(0u32));
            // Other settings (IP, DNS, routes, etc.) are preserved when replacing the PSK.
            let _: () = bus
                .with_proxy(NM, path.clone(), Duration::from_secs(3))
                .method_call(
                    "org.freedesktop.NetworkManager.Settings.Connection",
                    "Update",
                    (settings,),
                )
                .map_err(manager_error)?;
        }
        let (active,): (Path<'static>,) = manager
            .method_call(NM, "ActivateConnection", (path, device, ap))
            .map_err(manager_error)?;
        active
    } else {
        if network.enterprise() || network.security.contains("WEP") {
            return Err("Configure this enterprise/WEP network in NetworkManager first; saved profiles can be reused here.".into());
        }
        if network.personal() && password.is_empty() {
            return Err("Enter this network's Wi-Fi password, then Connect.".into());
        }
        let settings = new_settings(network, &password, &user_permission()?);
        let (_, active): (Path<'static>, Path<'static>) = manager
            .method_call(NM, "AddAndActivateConnection", (settings, device, ap))
            .map_err(manager_error)?;
        active
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    let active = bus.with_proxy(NM, active, Duration::from_secs(3));
    while Instant::now() < deadline && !cancel.load(Ordering::Relaxed) {
        let state: u32 = active
            .get("org.freedesktop.NetworkManager.Connection.Active", "State")
            .map_err(manager_error)?;
        match state {
            2 => return Ok(()),
            3 | 4 => {
                return Err(
                    "Connection failed. Check the password, signal and network settings.".into(),
                )
            }
            _ => thread::sleep(Duration::from_millis(200)),
        }
    }
    Err("Connection is still pending or timed out. Refresh to check its state.".into())
}

fn user_permission() -> Result<String, String> {
    let mut buffer = vec![0u8; 16384];
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result = std::ptr::null_mut();
    // getpwuid_r uses caller-owned storage, so concurrent workers never share passwd data.
    let code = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            entry.as_mut_ptr(),
            buffer.as_mut_ptr().cast(),
            buffer.len(),
            &mut result,
        )
    };
    if code != 0 || result.is_null() {
        return Err("Cannot determine the logged-in user for a private Wi-Fi profile.".into());
    }
    let entry = unsafe { entry.assume_init() };
    if entry.pw_name.is_null() {
        return Err("Missing local username.".into());
    }
    let name = unsafe { std::ffi::CStr::from_ptr(entry.pw_name) }
        .to_str()
        .map_err(|_| "Invalid local username.".to_string())?;
    if name.is_empty() || name.contains(':') {
        return Err("Invalid local username.".into());
    }
    Ok(format!("user:{name}:"))
}

fn new_settings(network: &Network, password: &Secret, permission: &str) -> Settings {
    let mut settings = Settings::new();
    settings.insert(
        "connection".into(),
        HashMap::from([
            ("id".into(), variant(network.ssid.clone())),
            ("type".into(), variant("802-11-wireless".to_string())),
            ("autoconnect".into(), variant(true)),
            // The host permits modifying one's own profiles without administrator auth.
            ("permissions".into(), variant(vec![permission.to_string()])),
        ]),
    );
    settings.insert(
        "802-11-wireless".into(),
        HashMap::from([
            ("ssid".into(), variant(network.ssid.as_bytes().to_vec())),
            ("mode".into(), variant("infrastructure".to_string())),
        ]),
    );
    if network.personal() || network.security.contains("OWE") {
        let key = if network.security.contains("OWE") {
            "owe"
        } else if network.security.contains("WPA3") && !network.security.contains("WPA2") {
            "sae"
        } else {
            "wpa-psk"
        };
        let mut security = HashMap::from([("key-mgmt".into(), variant(key.to_string()))]);
        if network.personal() {
            security.insert("psk".into(), variant(password.as_str().to_string()));
        }
        settings.insert("802-11-wireless-security".into(), security);
    }
    for group in ["ipv4", "ipv6"] {
        settings.insert(
            group.into(),
            HashMap::from([("method".into(), variant("auto".to_string()))]),
        );
    }
    settings
}

pub(super) fn disconnect(network: &Network) -> Result<(), String> {
    let bus = Connection::new_system().map_err(manager_error)?;
    let manager = bus.with_proxy(NM, ROOT, Duration::from_secs(3));
    let (device,): (Path<'static>,) = manager
        .method_call(NM, "GetDeviceByIpIface", (&network.device,))
        .map_err(manager_error)?;
    bus.with_proxy(NM, device, Duration::from_secs(3))
        .method_call("org.freedesktop.NetworkManager.Device", "Disconnect", ())
        .map_err(manager_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "requires tests/wifi-backend.py private NetworkManager fixture"]
    fn private_network_manager_contract() {
        assert_eq!(
            std::env::var("EMBER_WIFI_TEST_FIXTURE").as_deref(),
            Ok("private-bus")
        );
        assert_eq!(
            std::env::var("DBUS_SYSTEM_BUS_ADDRESS").unwrap(),
            std::env::var("DBUS_SESSION_BUS_ADDRESS").unwrap()
        );
        let cancel = AtomicBool::new(false);
        let snapshot = scan(false, &cancel).unwrap();
        assert!(snapshot.radio);
        assert_eq!(snapshot.networks.len(), 2);
        assert_eq!(snapshot.networks[0].ssid, "Ember Test");
        let n = snapshot.networks[0].clone();
        let secret = || {
            let mut s = Secret::default();
            for c in "test-only-secret".chars() {
                s.push(c);
            }
            s
        };
        connect(&n, secret(), &cancel).unwrap(); // new profile
        connect(&n, Secret::default(), &cancel).unwrap(); // saved credentials
        connect(&n, secret(), &cancel).unwrap(); // update PSK, preserve settings
        let mut bad = Secret::default();
        for c in "wrong-password".chars() {
            bad.push(c);
        }
        let error = connect(&n, bad, &cancel).unwrap_err();
        assert!(error.contains("Connection failed"));
        assert!(!error.contains("wrong-password"));
        disconnect(&n).unwrap();
        let denied = Network {
            ssid: "Denied".into(),
            ..n
        };
        assert!(connect(&denied, secret(), &cancel)
            .unwrap_err()
            .contains("Permission denied"));
    }
    #[test]
    fn escaped_ssids_bssids_and_malformed_rows() {
        assert_eq!(
            fields(r"*:Cafe\: blue\\red:AA\:BB:80:WPA2:/ap"),
            ["*", "Cafe: blue\\red", "AA:BB", "80", "WPA2", "/ap"]
        );
        let aps = parse_aps("*:Cafe\\: blue:AA\\:BB:80:WPA2:/org/freedesktop/NetworkManager/AccessPoint/1\nmalformed\n::AA:40::/org/freedesktop/NetworkManager/AccessPoint/2", "wlan0");
        assert_eq!(aps.len(), 1);
        assert_eq!(aps[0].ssid, "Cafe: blue");
        assert!(aps[0].active);
    }
    #[test]
    fn new_profiles_set_real_ssid_bytes_and_correct_security() {
        let mut n = Network {
            ssid: "Cafe: 🐈".into(),
            security: "WPA3".into(),
            ..Default::default()
        };
        let mut secret = Secret::default();
        secret.push('s');
        let s = new_settings(&n, &secret, "user:test-user:");
        assert_eq!(
            prop(&s, "802-11-wireless-security", "key-mgmt")
                .unwrap()
                .as_str(),
            Some("sae")
        );
        assert_eq!(
            prop(&s, "802-11-wireless-security", "psk")
                .unwrap()
                .as_str(),
            Some("s")
        );
        n.security = "WPA2 WPA3".into();
        assert_eq!(
            prop(
                &new_settings(&n, &secret, "user:test-user:"),
                "802-11-wireless-security",
                "key-mgmt"
            )
            .unwrap()
            .as_str(),
            Some("wpa-psk")
        );
        n.security = "--".into();
        assert!(
            !new_settings(&n, &secret, "user:test-user:").contains_key("802-11-wireless-security")
        );
        n.security = "OWE".into();
        let s = new_settings(&n, &secret, "user:test-user:");
        assert!(!s["802-11-wireless-security"].contains_key("psk"));
    }
}
