//! Freedesktop notifications, serviced on the compositor event loop without worker threads.
use crate::CalloopData;
use dbus::{
    arg::ArgType,
    channel::{BusType, Channel},
    Message, MessageType,
};
use serde::{Deserialize, Serialize};
use smithay::reexports::calloop::{
    generic::Generic, Interest, LoopHandle, Mode, PostAction, RegistrationToken,
};
use std::{
    cell::Cell,
    ffi::CString,
    os::fd::{FromRawFd, OwnedFd},
    rc::Rc,
    time::{Duration, Instant},
};
const NAME: &str = "org.freedesktop.Notifications";
const PATH: &str = "/org/freedesktop/Notifications";
const PORTAL_NAME: &str = "org.freedesktop.impl.portal.desktop.ember";
const PORTAL_PATH: &str = "/org/freedesktop/portal/desktop";
const PORTAL_INTERFACE: &str = "org.freedesktop.impl.portal.Notification";
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub width: i32,
    pub margin: i32,
    pub compact: bool,
    pub respect_panel_space: bool,
    pub gap: i32,
    pub max_visible: usize,
    pub max_queued: usize,
    pub timeout_ms: u64,
    pub do_not_disturb: bool,
    pub critical_bypass_dnd: bool,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            width: 340,
            margin: 12,
            compact: false,
            respect_panel_space: true,
            gap: 8,
            max_visible: 4,
            max_queued: 32,
            timeout_ms: 5000,
            do_not_disturb: false,
            critical_bypass_dnd: true,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(180..=800).contains(&self.width)
            || !(0..=200).contains(&self.margin)
            || !(0..=80).contains(&self.gap)
            || !(1..=8).contains(&self.max_visible)
            || !(1..=64).contains(&self.max_queued)
            || self.max_queued < self.max_visible
            || !(100..=600_000).contains(&self.timeout_ms)
        {
            return Err("notification dimensions, queue size or timeout out of range".into());
        }
        Ok(())
    }
}
pub struct Notice {
    pub id: u32,
    pub app: String,
    pub summary: String,
    pub body: String,
    pub actions: Vec<(String, String)>,
    pub urgency: u8,
    pub resident: bool,
    pub deadline: Option<Instant>,
    pub sender: Option<String>,
    portal: Option<PortalNotice>,
}
struct PortalNotice {
    app: String,
    id: String,
    targets: std::collections::HashMap<String, dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>>,
    default_action: Option<String>,
}
pub struct Request<'a> {
    pub app: &'a str,
    pub replaces: u32,
    pub summary: &'a str,
    pub body: &'a str,
    pub actions: Vec<(String, String)>,
    pub urgency: u8,
    pub resident: bool,
    pub timeout: i32,
    pub sender: Option<String>,
}
pub enum Signal {
    Closed(u32, u32, Option<String>),
    Action(u32, String, Option<String>),
    PortalAction(
        String,
        String,
        String,
        Vec<dbus::arg::Variant<Box<dyn dbus::arg::RefArg>>>,
    ),
}
#[derive(Default)]
pub struct Runtime {
    pub entries: Vec<Notice>,
    pub revision: u64,
    next_id: u32,
    pub signals: Vec<Signal>,
    pub server_running: bool,
    pub server_error: Option<String>,
}
fn bounded(text: &str, max: usize) -> String {
    text.chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(max)
        .collect()
}
impl Runtime {
    pub fn notify(&mut self, settings: &Settings, request: Request<'_>) -> u32 {
        let id = if request.replaces != 0
            && self
                .entries
                .iter()
                .any(|e| e.id == request.replaces && e.sender == request.sender)
        {
            request.replaces
        } else {
            loop {
                self.next_id = self.next_id.wrapping_add(1).max(1);
                if !self.entries.iter().any(|e| e.id == self.next_id) {
                    break self.next_id;
                }
            }
        };
        if !settings.enabled
            || (settings.do_not_disturb && !(request.urgency == 2 && settings.critical_bypass_dnd))
        {
            if !self.close(id, 4) {
                self.emit(Signal::Closed(id, 4, request.sender));
            }
            return id;
        }
        let duration = match request.timeout {
            0 => None,
            n if n > 0 => Some(Duration::from_millis(n as u64)),
            _ if request.urgency == 2 => None,
            _ => Some(Duration::from_millis(settings.timeout_ms)),
        };
        let notice = Notice {
            id,
            app: bounded(request.app, 64),
            summary: bounded(request.summary, 160),
            body: bounded(request.body, 1024),
            actions: request
                .actions
                .into_iter()
                .take(4)
                .map(|(k, v)| (bounded(&k, 128), bounded(&v, 48)))
                .collect(),
            urgency: request.urgency.min(2),
            resident: request.resident,
            deadline: duration.map(|d| Instant::now() + d),
            sender: request.sender,
            portal: None,
        };
        if let Some(old) = self.entries.iter_mut().find(|e| e.id == id) {
            *old = notice;
        } else {
            while self.entries.len() >= settings.max_queued {
                self.close(self.entries[0].id, 4);
            }
            self.entries.push(notice);
        }
        self.revision = self.revision.wrapping_add(1);
        id
    }
    fn emit(&mut self, signal: Signal) {
        if self.signals.len() < 256 {
            self.signals.push(signal);
        }
    }
    pub fn close(&mut self, id: u32, reason: u32) -> bool {
        if let Some(i) = self.entries.iter().position(|e| e.id == id) {
            let n = self.entries.remove(i);
            if n.portal.is_none() {
                self.emit(Signal::Closed(id, reason, n.sender));
            }
            self.revision = self.revision.wrapping_add(1);
            if self.entries.is_empty() {
                self.entries = Vec::new();
            }
            true
        } else {
            false
        }
    }
    pub fn invoke(&mut self, id: u32, key: &str) {
        if let Some(n) = self.entries.iter().find(|e| e.id == id) {
            if !n.actions.iter().any(|(k, _)| k == key) {
                return;
            }
            let resident = n.resident;
            let sender = n.sender.clone();
            let portal = n.portal.as_ref().map(|p| {
                (
                    p.app.clone(),
                    p.id.clone(),
                    if key == "default" {
                        p.default_action.clone().unwrap_or_default()
                    } else {
                        key.to_string()
                    },
                    p.targets
                        .get(key)
                        .map(|v| dbus::arg::Variant(v.0.box_clone()))
                        .into_iter()
                        .collect(),
                )
            });
            if let Some((app, portal_id, action, target)) = portal {
                self.emit(Signal::PortalAction(app, portal_id, action, target));
            } else {
                self.emit(Signal::Action(id, key.into(), sender));
            }
            if !resident {
                self.close(id, 2);
            }
        }
    }
    pub fn clear(&mut self, reason: u32) {
        while let Some(id) = self.entries.first().map(|e| e.id) {
            self.close(id, reason);
        }
    }
    pub fn advance(&mut self, settings: &Settings) {
        let now = Instant::now();
        loop {
            let expired = self
                .entries
                .iter()
                .find(|e| !settings.enabled || e.deadline.is_some_and(|d| d <= now))
                .map(|e| e.id);
            if let Some(id) = expired {
                self.close(id, if settings.enabled { 1 } else { 4 });
            } else {
                break;
            }
        }
        while self.entries.len() > settings.max_queued {
            self.close(self.entries[0].id, 4);
        }
    }
    pub fn next_wake(&self) -> Option<Duration> {
        self.entries
            .iter()
            .filter_map(|e| e.deadline)
            .min()
            .map(|d| d.saturating_duration_since(Instant::now()))
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"server_running":self.server_running,"server_error":self.server_error,"queued":self.entries.len(),"items":self.entries.iter().map(|e|serde_json::json!({"id":e.id,"app":e.app,"summary":e.summary,"body":e.body,"urgency":e.urgency,"actions":e.actions,"persistent":e.deadline.is_none()})).collect::<Vec<_>>()})
    }
}
struct Bus {
    channel: Channel,
    ready: Rc<Cell<bool>>,
    token: RegistrationToken,
    pending: Option<Message>,
}
#[derive(Default)]
pub struct Service {
    bus: Option<Bus>,
    attempted: Option<(bool, u64)>,
    retry: Option<Instant>,
}
impl Service {
    pub fn next_wake(&self) -> Option<Duration> {
        if self.bus.as_ref().is_some_and(|b| {
            b.ready.get() || b.pending.is_some() || b.channel.has_messages_to_send()
        }) {
            return Some(Duration::ZERO);
        }
        self.retry
            .map(|t| t.saturating_duration_since(Instant::now()))
    }
    pub fn update(
        &mut self,
        handle: &LoopHandle<'_, CalloopData>,
        enabled: bool,
        generation: u64,
        settings: &Settings,
        runtime: &mut Runtime,
    ) {
        if !enabled && self.bus.is_some() {
            runtime.clear(4);
            self.dispatch(runtime, settings);
            if let Some(bus) = self.bus.take() {
                handle.remove(bus.token);
            }
            runtime.server_running = false;
        }
        if enabled
            && self.bus.is_none()
            && (self.attempted != Some((enabled, generation))
                || self.retry.is_some_and(|t| Instant::now() >= t))
        {
            self.attempted = Some((enabled, generation));
            match Self::connect(handle) {
                Ok(bus) => {
                    self.bus = Some(bus);
                    runtime.server_running = true;
                    runtime.server_error = None;
                    self.retry = None;
                }
                Err(e) => {
                    runtime.server_error = Some(e);
                    runtime.server_running = false;
                    self.retry = Some(Instant::now() + Duration::from_secs(3));
                }
            }
        }
        if !enabled {
            self.attempted = None;
            runtime.server_error = None;
            self.retry = None;
        }
        self.dispatch(runtime, settings);
        if self.bus.as_ref().is_some_and(|b| !b.channel.is_connected()) {
            if let Some(bus) = self.bus.take() {
                handle.remove(bus.token);
            }
            runtime.server_running = false;
            self.retry = Some(Instant::now() + Duration::from_secs(3));
        }
    }
    fn connect(handle: &LoopHandle<'_, CalloopData>) -> Result<Bus, String> {
        let mut channel = Channel::get_private(BusType::Session).map_err(|e| e.to_string())?;
        let request = Message::new_method_call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
        )
        .unwrap()
        .append2(NAME, 4u32);
        let reply = channel
            .send_with_reply_and_block(request, Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        let result: u32 = reply.read1().map_err(|e| e.to_string())?;
        if result != 1 && result != 4 {
            return Err("another notification server already owns the session bus name".into());
        }
        let request = Message::new_method_call(
            "org.freedesktop.DBus",
            "/org/freedesktop/DBus",
            "org.freedesktop.DBus",
            "RequestName",
        )
        .unwrap()
        .append2(PORTAL_NAME, 4u32);
        let reply = channel
            .send_with_reply_and_block(request, Duration::from_secs(2))
            .map_err(|e| e.to_string())?;
        if !matches!(reply.read1::<u32>(), Ok(1 | 4)) {
            return Err("another Ember notification portal already owns the bus name".into());
        }
        channel.set_watch_enabled(true);
        // SAFETY: duplicate the live D-Bus descriptor; calloop owns only the duplicate.
        let fd = unsafe { libc::fcntl(channel.watch().fd, libc::F_DUPFD_CLOEXEC, 3) };
        if fd < 0 {
            return Err(std::io::Error::last_os_error().to_string());
        }
        let owned = unsafe { OwnedFd::from_raw_fd(fd) };
        let ready = Rc::new(Cell::new(true));
        let wake = ready.clone();
        let token = handle
            .insert_source(
                Generic::new(owned, Interest::READ, Mode::Level),
                move |_, _, _| {
                    wake.set(true);
                    Ok(PostAction::Continue)
                },
            )
            .map_err(|e| e.to_string())?;
        Ok(Bus {
            channel,
            ready,
            token,
            pending: None,
        })
    }
    fn dispatch(&mut self, runtime: &mut Runtime, settings: &Settings) {
        let Some(bus) = &mut self.bus else {
            runtime.signals.clear();
            return;
        };
        if bus.pending.is_none() {
            bus.pending = bus.channel.pop_message();
        }
        if bus.ready.replace(false)
            || bus.pending.is_some()
            || !runtime.signals.is_empty()
            || bus.channel.has_messages_to_send()
        {
            if bus.channel.read_write(Some(Duration::ZERO)).is_err() {
                runtime.server_running = false;
                runtime.server_error = Some("session bus disconnected".into());
                return;
            }
            for i in 0..32 {
                let Some(message) = bus.pending.take().or_else(|| bus.channel.pop_message()) else {
                    break;
                };
                if let Some(reply) = handle_message(&message, runtime, settings) {
                    let _ = bus.channel.send(reply);
                }
                if i == 31 {
                    bus.ready.set(true);
                }
            }
            for signal in std::mem::take(&mut runtime.signals) {
                let (mut msg, sender) = match signal {
                    Signal::Closed(id, reason, sender) => (
                        Message::new_signal(PATH, NAME, "NotificationClosed")
                            .unwrap()
                            .append2(id, reason),
                        sender,
                    ),
                    Signal::Action(id, key, sender) => (
                        Message::new_signal(PATH, NAME, "ActionInvoked")
                            .unwrap()
                            .append2(id, key),
                        sender,
                    ),
                    Signal::PortalAction(app, id, action, target) => {
                        let path = format!("/{}", app.replace('.', "/").replace('-', "_"));
                        if let Some(action) = action.strip_prefix("app.") {
                            if let Ok(message) = Message::new_method_call(
                                &app,
                                &path,
                                "org.freedesktop.Application",
                                "ActivateAction",
                            ) {
                                let mut message =
                                    message.append3(action, target, dbus::arg::PropMap::new());
                                message.set_no_reply(true);
                                let _ = bus.channel.send(message);
                                continue;
                            }
                        } else if !app.is_empty() {
                            if let Ok(message) = Message::new_method_call(
                                &app,
                                &path,
                                "org.freedesktop.Application",
                                "Activate",
                            ) {
                                let mut message = message.append1(dbus::arg::PropMap::new());
                                message.set_no_reply(true);
                                let _ = bus.channel.send(message);
                            }
                        }
                        (
                            Message::new_signal(PORTAL_PATH, PORTAL_INTERFACE, "ActionInvoked")
                                .unwrap()
                                .append3(app, id, action)
                                .append1(target),
                            None,
                        )
                    }
                };
                if let Some(sender) = sender {
                    msg.set_destination(Some(sender.into()));
                }
                let _ = bus.channel.send(msg);
            }
            let _ = bus.channel.read_write(Some(Duration::ZERO));
            // read_write also reads. Preserve a message it pulled into libdbus after
            // the drain: the socket can now be empty, so fd readiness alone loses it.
            bus.pending = bus.channel.pop_message();
            if bus.pending.is_some() {
                bus.ready.set(true);
            }
        }
    }
}
fn portal_message(m: &Message, r: &mut Runtime, settings: &Settings) -> Message {
    use dbus::arg::{PropMap, Variant};
    let interface = m.interface();
    let member = m.member();
    let member = member.as_deref().unwrap_or("");
    if interface.as_deref() == Some("org.freedesktop.DBus.Introspectable") && member == "Introspect"
    {
        return m.method_return().append1(PORTAL_INTROSPECTION);
    }
    if interface.as_deref() == Some("org.freedesktop.DBus.Properties") {
        match member {
            "Get" => {
                if let Ok((iface, key)) = m.read2::<String, String>() {
                    if iface == PORTAL_INTERFACE && key == "version" {
                        return m.method_return().append1(Variant(1u32));
                    }
                }
            }
            "GetAll" if m.read1::<String>().ok().as_deref() == Some(PORTAL_INTERFACE) => {
                let values: PropMap = std::collections::HashMap::from([(
                    "version".into(),
                    Variant(Box::new(1u32) as Box<dyn dbus::arg::RefArg>),
                )]);
                return m.method_return().append1(values);
            }
            _ => (),
        }
        return error(
            m,
            "org.freedesktop.DBus.Error.InvalidArgs",
            "unknown portal property",
        );
    }
    if interface.as_deref() != Some(PORTAL_INTERFACE) {
        return error(
            m,
            "org.freedesktop.DBus.Error.UnknownInterface",
            "unknown portal interface",
        );
    }
    match member {
        "AddNotification" => {
            let Ok((app, id, values)) = m.read3::<String, String, PropMap>() else {
                return error(
                    m,
                    "org.freedesktop.DBus.Error.InvalidArgs",
                    "expected app, id, notification",
                );
            };
            if app.len() > 255 || id.len() > 255 {
                return error(
                    m,
                    "org.freedesktop.DBus.Error.InvalidArgs",
                    "notification identity too long",
                );
            }
            let text = |key: &str| values.get(key).and_then(|v| v.0.as_str()).unwrap_or("");
            let replaces = r
                .entries
                .iter()
                .find(|n| {
                    n.portal
                        .as_ref()
                        .is_some_and(|p| p.app == app && p.id == id)
                })
                .map_or(0, |n| n.id);
            let mut actions = Vec::new();
            let mut targets = std::collections::HashMap::new();
            actions.push(("default".into(), "Open".into()));
            if !text("default-action").is_empty() {
                if let Some(v) = values.get("default-action-target") {
                    targets.insert("default".to_string(), Variant(v.0.box_clone()));
                }
            }
            if let Some(buttons) = values.get("buttons").and_then(|v| v.0.as_iter()) {
                for button in buttons.take(3) {
                    let mut label = String::new();
                    let mut action = String::new();
                    let mut target = None;
                    if let Some(pairs) = button.as_iter() {
                        for pair in pairs.take(8) {
                            if let Some(mut fields) = pair.as_iter() {
                                let key = fields.next().and_then(|v| v.as_str()).unwrap_or("");
                                if let Some(v) = fields
                                    .next()
                                    .and_then(|v| v.as_iter())
                                    .and_then(|mut it| it.next())
                                {
                                    match key {
                                        "label" => label = bounded(v.as_str().unwrap_or(""), 48),
                                        "action" => action = bounded(v.as_str().unwrap_or(""), 128),
                                        "target" => target = Some(Variant(v.box_clone())),
                                        _ => (),
                                    }
                                }
                            }
                        }
                    }
                    if !action.is_empty() {
                        if let Some(v) = target {
                            targets.insert(action.clone(), v);
                        }
                        actions.push((action, label));
                    }
                }
            }
            let urgency = match text("priority") {
                "urgent" => 2,
                "low" => 0,
                _ => 1,
            };
            let notice = r.notify(
                settings,
                Request {
                    app: &app,
                    replaces,
                    summary: text("title"),
                    body: text("body"),
                    actions,
                    urgency,
                    resident: false,
                    timeout: -1,
                    sender: m.sender().map(|s| s.to_string()),
                },
            );
            if let Some(n) = r.entries.iter_mut().find(|n| n.id == notice) {
                n.portal = Some(PortalNotice {
                    app,
                    id,
                    targets,
                    default_action: values
                        .get("default-action")
                        .and_then(|v| v.0.as_str())
                        .map(str::to_string),
                });
            }
            m.method_return()
        }
        "RemoveNotification" => {
            let Ok((app, id)) = m.read2::<String, String>() else {
                return error(
                    m,
                    "org.freedesktop.DBus.Error.InvalidArgs",
                    "expected app and id",
                );
            };
            if let Some(notice) = r
                .entries
                .iter()
                .find(|n| {
                    n.portal
                        .as_ref()
                        .is_some_and(|p| p.app == app && p.id == id)
                })
                .map(|n| n.id)
            {
                r.close(notice, 3);
            }
            m.method_return()
        }
        _ => error(
            m,
            "org.freedesktop.DBus.Error.UnknownMethod",
            "unknown notification portal method",
        ),
    }
}
const PORTAL_INTROSPECTION: &str = r#"<node><interface name="org.freedesktop.impl.portal.Notification"><property name="version" type="u" access="read"/><method name="AddNotification"><arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="a{sv}" direction="in"/></method><method name="RemoveNotification"><arg type="s" direction="in"/><arg type="s" direction="in"/></method><signal name="ActionInvoked"><arg type="s"/><arg type="s"/><arg type="s"/><arg type="av"/></signal></interface></node>"#;

fn signature(message: &Message) -> String {
    let mut it = message.iter_init();
    let mut signature = String::new();
    for _ in 0..16 {
        if it.arg_type() == ArgType::Invalid {
            break;
        }
        signature.push_str(&it.signature());
        if !it.next() {
            break;
        }
    }
    signature
}
fn error(message: &Message, name: &str, text: &str) -> Message {
    message.error(&name.into(), &CString::new(text).unwrap())
}
fn handle_message(m: &Message, r: &mut Runtime, settings: &Settings) -> Option<Message> {
    if m.msg_type() != MessageType::MethodCall {
        return None;
    }
    if m.path().as_deref() == Some(PORTAL_PATH) {
        return Some(portal_message(m, r, settings));
    }
    if m.path().as_deref() != Some(PATH) {
        return Some(error(
            m,
            "org.freedesktop.DBus.Error.UnknownObject",
            "unknown notification object",
        ));
    }
    let interface = m.interface();
    let member = m.member();
    let member = member.as_deref().unwrap_or("");
    if interface.as_deref() == Some("org.freedesktop.DBus.Introspectable") && member == "Introspect"
    {
        return Some(m.method_return().append1(INTROSPECTION));
    }
    if interface.as_deref() == Some("org.freedesktop.DBus.Peer") && member == "Ping" {
        return Some(m.method_return());
    }
    if interface.as_deref() != Some(NAME) {
        return Some(error(
            m,
            "org.freedesktop.DBus.Error.UnknownInterface",
            "unknown interface",
        ));
    }
    let invalid = || {
        error(
            m,
            "org.freedesktop.DBus.Error.InvalidArgs",
            "invalid notification arguments",
        )
    };
    let reply = match member {
        "GetCapabilities" if m.iter_init().arg_type() == ArgType::Invalid => {
            m.method_return().append1(vec!["body", "actions"])
        }
        "GetServerInformation" if m.iter_init().arg_type() == ArgType::Invalid => m
            .method_return()
            .append3("EmberWM", "EmberWM", env!("CARGO_PKG_VERSION"))
            .append1("1.2"),
        "CloseNotification" => {
            let Ok(id) = m.read1::<u32>() else {
                return Some(invalid());
            };
            if !r.close(id, 3) {
                return Some(error(
                    m,
                    "org.freedesktop.Notifications.Error.InvalidId",
                    "notification does not exist",
                ));
            }
            m.method_return()
        }
        "Notify" => {
            if signature(m) != "susssasa{sv}i" {
                return Some(invalid());
            }
            let mut it = m.iter_init();
            let parsed = (|| -> Option<_> {
                let app = it.read::<&str>().ok()?;
                let replaces = it.read::<u32>().ok()?;
                let _icon = it.read::<&str>().ok()?;
                let summary = it.read::<&str>().ok()?;
                let body = it.read::<&str>().ok()?;
                let mut a = it.recurse(ArgType::Array)?;
                let mut actions = Vec::new();
                for _ in 0..4 {
                    let Some(key) = a.get::<&str>() else {
                        break;
                    };
                    a.next();
                    let label = a.get::<&str>()?;
                    actions.push((bounded(key, 128), bounded(label, 48)));
                    a.next();
                }
                it.next();
                let mut hints = it.recurse(ArgType::Array)?;
                let mut urgency = 1;
                let mut resident = false;
                for _ in 0..64 {
                    let Some(mut pair) = hints.recurse(ArgType::DictEntry) else {
                        break;
                    };
                    let key = pair.read::<&str>().ok()?;
                    if let Some(mut value) = pair.recurse(ArgType::Variant) {
                        if key == "urgency" {
                            urgency = value.get::<u8>().unwrap_or(1).min(2);
                        }
                        if key == "resident" {
                            resident = value.get::<bool>().unwrap_or(false);
                        }
                    }
                    if !hints.next() {
                        break;
                    }
                }
                it.next();
                let timeout = it.read::<i32>().ok()?;
                Some((
                    app, replaces, summary, body, actions, urgency, resident, timeout,
                ))
            })();
            let Some((app, replaces, summary, body, actions, urgency, resident, timeout)) = parsed
            else {
                return Some(invalid());
            };
            let id = r.notify(
                settings,
                Request {
                    app,
                    replaces,
                    summary,
                    body,
                    actions,
                    urgency,
                    resident,
                    timeout,
                    sender: m.sender().map(|s| s.to_string()),
                },
            );
            m.method_return().append1(id)
        }
        _ => {
            return Some(error(
                m,
                "org.freedesktop.DBus.Error.UnknownMethod",
                "unknown notification method",
            ))
        }
    };
    Some(reply)
}
const INTROSPECTION: &str = r#"<node><interface name="org.freedesktop.Notifications"><method name="GetCapabilities"><arg direction="out" type="as"/></method><method name="GetServerInformation"><arg direction="out" type="s"/><arg direction="out" type="s"/><arg direction="out" type="s"/><arg direction="out" type="s"/></method><method name="Notify"><arg direction="in" type="s"/><arg direction="in" type="u"/><arg direction="in" type="s"/><arg direction="in" type="s"/><arg direction="in" type="s"/><arg direction="in" type="as"/><arg direction="in" type="a{sv}"/><arg direction="in" type="i"/><arg direction="out" type="u"/></method><method name="CloseNotification"><arg direction="in" type="u"/></method><signal name="NotificationClosed"><arg type="u"/><arg type="u"/></signal><signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal></interface><interface name="org.freedesktop.DBus.Introspectable"><method name="Introspect"><arg direction="out" type="s"/></method></interface><interface name="org.freedesktop.DBus.Peer"><method name="Ping"/></interface></node>"#;

#[cfg(test)]
mod tests {
    use super::*;
    fn request(replaces: u32, timeout: i32) -> Request<'static> {
        Request {
            app: "tests",
            replaces,
            summary: "summary",
            body: "body",
            actions: vec![
                ("default".into(), "Open".into()),
                ("copy".into(), "Copy".into()),
            ],
            urgency: 1,
            resident: false,
            timeout,
            sender: Some(":1.2".into()),
        }
    }
    #[test]
    fn replaces_persists_expires_and_emits_close_reasons() {
        let s = Settings::default();
        let mut r = Runtime::default();
        let id = r.notify(&s, request(0, 0));
        assert!(id > 0);
        assert!(r.entries[0].deadline.is_none());
        let mut replacement = request(id, 50);
        replacement.summary = "new";
        assert_eq!(r.notify(&s, replacement), id);
        assert_eq!(r.entries.len(), 1);
        assert_eq!(r.entries[0].summary, "new");
        r.entries[0].deadline = Some(Instant::now() - Duration::from_secs(1));
        r.advance(&s);
        assert!(r.entries.is_empty());
        assert!(matches!(&r.signals[0],Signal::Closed(n,1,_) if *n==id));
        let id = r.notify(&s, request(0, 0));
        r.invoke(id, "unknown");
        assert_eq!(r.entries.len(), 1);
        r.invoke(id, "copy");
        assert!(r.entries.is_empty());
        assert!(r
            .signals
            .iter()
            .any(|s| matches!(s,Signal::Action(n,k,_) if *n==id && k=="copy")));
    }
    #[test]
    fn bounds_retention_and_dnd_while_critical_bypasses() {
        let mut s = Settings {
            max_queued: 4,
            ..Default::default()
        };
        let mut r = Runtime::default();
        for _ in 0..100 {
            r.notify(&s, request(0, 0));
        }
        assert_eq!(r.entries.len(), 4);
        s.do_not_disturb = true;
        r.clear(2);
        let mut normal = request(0, 0);
        normal.body = "ignored";
        r.notify(&s, normal);
        assert!(r.entries.is_empty());
        let mut critical = request(0, -1);
        critical.urgency = 2;
        r.notify(&s, critical);
        assert_eq!(r.entries.len(), 1);
        assert!(r.entries[0].deadline.is_none());
        s.enabled = false;
        r.advance(&s);
        assert!(r.entries.is_empty());
        assert!(r.next_wake().is_none());
        assert!(s.validate().is_ok());
    }
    #[test]
    fn protocol_parses_hints_actions_and_rejects_wrong_types() {
        use dbus::arg::{PropMap, Variant};
        let mut hints = PropMap::new();
        hints.insert("urgency".into(), Variant(Box::new(2u8)));
        hints.insert("resident".into(), Variant(Box::new(true)));
        let mut m = Message::new_method_call(NAME, PATH, NAME, "Notify").unwrap();
        m.set_serial(1);
        m.append_all((
            "Test",
            0u32,
            "",
            "Hello",
            "World",
            vec!["default", "Open", "copy", "Copy"],
            hints,
            -1i32,
        ));
        let mut r = Runtime::default();
        let reply = handle_message(&m, &mut r, &Settings::default()).unwrap();
        assert_eq!(reply.msg_type(), MessageType::MethodReturn);
        assert!(reply.read1::<u32>().unwrap() > 0);
        assert_eq!(r.entries[0].urgency, 2);
        assert!(r.entries[0].resident);
        assert_eq!(r.entries[0].actions.len(), 2);
        let mut invalid = Message::new_method_call(NAME, PATH, NAME, "Notify")
            .unwrap()
            .append1("bad");
        invalid.set_serial(2);
        assert_eq!(
            handle_message(&invalid, &mut r, &Settings::default())
                .unwrap()
                .msg_type(),
            MessageType::Error
        );
    }
}
