//! Session bus activation must point helpers at this compositor, not the greeter.
use crate::Ember;

const BACKENDS: [(&str, &str); 4] = [
    ("QT_QPA_PLATFORM", "wayland;xcb"),
    ("GDK_BACKEND", "wayland,x11"),
    ("SDL_VIDEO_DRIVER", "wayland,x11"),
    ("SDL_VIDEODRIVER", "wayland,x11"),
];

pub fn default_backends() {
    for (key, value) in BACKENDS {
        if std::env::var_os(key).is_none_or(|v| v.is_empty()) {
            std::env::set_var(key, value);
        }
    }
}

pub fn export_activation_environment(state: &Ember) -> Result<(), String> {
    let bus = dbus::blocking::Connection::new_session().map_err(|e| e.to_string())?;
    let mut values = std::collections::HashMap::from([
        (
            "WAYLAND_DISPLAY".to_string(),
            state.socket_name.to_string_lossy().into_owned(),
        ),
        ("DISPLAY".into(), state.xwayland.display_name()),
        ("XAUTHORITY".into(), state.xwayland.auth_name()),
        ("XDG_CURRENT_DESKTOP".into(), "Ember".into()),
        ("XDG_SESSION_DESKTOP".into(), "Ember".into()),
        ("XDG_SESSION_TYPE".into(), "wayland".into()),
        (
            "EMBER_SOCKET".into(),
            state.ipc_path().to_string_lossy().into_owned(),
        ),
    ]);
    for (key, fallback) in BACKENDS {
        values.insert(
            key.into(),
            std::env::var(key).unwrap_or_else(|_| fallback.into()),
        );
    }
    bus.with_proxy(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        std::time::Duration::from_secs(2),
    )
    .method_call(
        "org.freedesktop.DBus",
        "UpdateActivationEnvironment",
        (values,),
    )
    .map_err(|e| e.to_string())
}
