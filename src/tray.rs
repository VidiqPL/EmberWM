//! StatusNotifier registry on a watched bus fd; popup work exists only on demand.
use crate::{
    config::{color, Config},
    panels, ui, CalloopData,
};
use dbus::{
    arg::{PropMap, RefArg, Variant},
    channel::{BusType, Channel},
    Message,
};
use smithay::{
    reexports::calloop::{
        generic::Generic, Interest, LoopHandle, Mode, PostAction, RegistrationToken,
    },
    utils::{Logical, Point, Rectangle, Size},
};
use std::{
    cell::Cell,
    os::fd::{FromRawFd, OwnedFd},
    path::Path,
    rc::Rc,
    sync::mpsc::{self, Receiver},
    time::Duration,
};
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub width: i32,
    pub font_size: f32,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            width: 460,
            font_size: 13.,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(240..=1200).contains(&self.width)
            || !self.font_size.is_finite()
            || !(9.0..=24.0).contains(&self.font_size)
        {
            Err("Tray width or font size out of range".into())
        } else {
            Ok(())
        }
    }
}
const NAME: &str = "org.kde.StatusNotifierWatcher";
const PATH: &str = "/StatusNotifierWatcher";
const ITEM: &str = "org.kde.StatusNotifierItem";
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize)]
pub struct Item {
    pub service: String,
    pub path: String,
    pub title: String,
}
#[derive(Clone, Debug)]
struct Row {
    label: String,
    id: i32,
    enabled: bool,
    submenu: bool,
}
enum ResultData {
    Items(Vec<Item>),
    Menu(Item, String, Vec<Row>),
    Done,
}
enum Job {
    List(Vec<Item>),
    Menu(Item, i32, Option<String>),
    Activate(Item),
    Event(Item, String, i32),
}
struct Bus {
    pending: Option<Message>,
    channel: Channel,
    ready: Rc<Cell<bool>>,
    token: RegistrationToken,
}
#[derive(Default)]
pub struct Runtime {
    pub open: bool,
    pub view: Option<ui::View>,
    pub error: Option<String>,
    bus: Option<Bus>,
    attempted: bool,
    retry_at: Option<std::time::Instant>,
    items: Vec<Item>,
    rows: Vec<Row>,
    menu: Option<(Item, String)>,
    selected: usize,
    offset: usize,
    job: Option<Receiver<Result<ResultData, String>>>,
    needs_list: bool,
    generation: u64,
    size: (i32, i32),
}
impl Runtime {
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"open":self.open,"watcher":self.bus.is_some(),"items":self.items,"busy":self.busy(),"error":self.error,"menu":self.menu.is_some()})
    }
    pub fn close(&mut self) {
        self.open = false;
        self.view = None;
        self.rows.clear();
        self.menu = None;
        self.offset = 0;
        crate::memory::request_reclaim();
    }
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open = true;
            self.needs_list = true;
            self.selected = 0;
            self.offset = 0;
            self.view = None;
        }
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub fn scroll(&mut self, down: bool) {
        let n = self.len();
        if n > 0 {
            self.selected = if down {
                (self.selected + 1).min(n - 1)
            } else {
                self.selected.saturating_sub(1)
            };
            self.view = None;
        }
    }
    fn len(&self) -> usize {
        if self.menu.is_some() {
            self.rows.len()
        } else {
            self.items.len()
        }
    }
    pub fn key(&mut self, key: smithay::input::keyboard::Keysym) {
        use smithay::input::keyboard::Keysym;
        match key {
            Keysym::Escape => self.close(),
            Keysym::Up => self.scroll(false),
            Keysym::Down => self.scroll(true),
            Keysym::Return => self.choose(false),
            Keysym::Right => self.choose(true),
            Keysym::Left | Keysym::BackSpace => {
                self.menu = None;
                self.rows.clear();
                self.selected = 0;
                self.offset = 0;
                self.view = None;
            }
            _ => (),
        }
    }
    pub fn click(&mut self, pos: Point<f64, Logical>, right: bool) {
        let Some(v) = &self.view else { return };
        if !v.rect.contains(pos.to_i32_floor()) {
            self.close();
            return;
        }
        let row = ((pos.y - v.rect.loc.y as f64 - 52.) / 32.).floor() as i32;
        if row >= 0 {
            self.selected = self.offset + row as usize;
            if self.selected < self.len() {
                self.choose(right);
            }
        }
    }
    pub fn select(&mut self, index: usize, menu: bool) {
        self.selected = index;
        self.choose(menu);
    }
    fn choose(&mut self, right: bool) {
        if self.busy() {
            return;
        }
        if let Some((item, path)) = self.menu.clone() {
            if let Some(row) = self.rows.get(self.selected).cloned().filter(|r| r.enabled) {
                self.start(if row.submenu {
                    Job::Menu(item, row.id, Some(path))
                } else {
                    Job::Event(item, path, row.id)
                });
            }
        } else if let Some(item) = self.items.get(self.selected).cloned() {
            self.start(if right {
                Job::Menu(item, 0, None)
            } else {
                Job::Activate(item)
            });
        }
    }
    fn start(&mut self, job: Job) {
        let (tx, rx) = mpsc::sync_channel(1);
        self.job = Some(rx);
        let result = std::thread::Builder::new()
            .name("ember-tray-job".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let _ = tx.send(run(job));
            });
        if let Err(e) = result {
            self.job = None;
            self.error = Some(e.to_string());
        }
        self.view = None;
    }
    pub fn maintain(&mut self, enabled: bool, handle: LoopHandle<'static, CalloopData>) {
        if self.bus.as_ref().is_some_and(|b| !b.channel.is_connected()) || !enabled {
            if let Some(bus) = self.bus.take() {
                handle.remove(bus.token);
                self.items.clear();
                self.close();
            }
            self.attempted = false;
        }
        if !enabled {
            return;
        }
        if self.bus.is_some()
            || self.attempted
                && self
                    .retry_at
                    .is_none_or(|at| std::time::Instant::now() < at)
        {
            return;
        }
        self.attempted = true;
        match connect(handle) {
            Ok(bus) => {
                self.bus = Some(bus);
                self.error = None;
                self.retry_at = None;
            }
            Err(e) => {
                self.error = Some(e);
                self.retry_at = Some(std::time::Instant::now() + Duration::from_secs(3));
            }
        }
    }
    fn dispatch(&mut self) -> bool {
        let Some(bus) = &mut self.bus else {
            return false;
        };
        if !bus.ready.replace(false) && !bus.channel.has_messages_to_send() {
            return false;
        }
        let _ = bus.channel.read_write(Some(Duration::ZERO));
        let mut changed = false;
        for _ in 0..64 {
            let Some(m) = bus.pending.take().or_else(|| bus.channel.pop_message()) else {
                break;
            };
            let member = m.member().map(|m| m.to_string()).unwrap_or_default();
            let interface = m.interface().map(|m| m.to_string()).unwrap_or_default();
            if interface == "org.freedesktop.DBus" && member == "NameOwnerChanged" {
                if let Ok((name, _, new)) = m.read3::<String, String, String>() {
                    if new.is_empty() {
                        let n = self.items.len();
                        for item in self.items.iter().filter(|i| i.service == name) {
                            let _ = bus.channel.send(
                                Message::new_signal(PATH, NAME, "StatusNotifierItemUnregistered")
                                    .unwrap()
                                    .append1(format!("{}{}", item.service, item.path)),
                            );
                        }
                        self.items.retain(|i| i.service != name);
                        changed |= n != self.items.len();
                    }
                }
                continue;
            }
            let reply = if interface == NAME && member == "RegisterStatusNotifierItem" {
                match m.read1::<String>() {
                    Ok(service) => {
                        let sender = m.sender().map(|s| s.to_string()).unwrap_or_default();
                        let (path, service) = if service.starts_with('/') {
                            (service, sender)
                        } else {
                            ("/StatusNotifierItem".into(), service)
                        };
                        let item = Item {
                            title: service.clone(),
                            service,
                            path,
                        };
                        if self.items.len() < 128
                            && !self
                                .items
                                .iter()
                                .any(|i| i.service == item.service && i.path == item.path)
                        {
                            let id = format!("{}{}", item.service, item.path);
                            self.items.push(item);
                            let _ = bus.channel.send(
                                Message::new_signal(PATH, NAME, "StatusNotifierItemRegistered")
                                    .unwrap()
                                    .append1(id),
                            );
                            changed = true;
                        }
                        m.method_return()
                    }
                    Err(_) => m.error(
                        &"org.freedesktop.DBus.Error.InvalidArgs".into(),
                        &std::ffi::CString::new("Expected service or object path").unwrap(),
                    ),
                }
            } else if interface == NAME && member == "RegisterStatusNotifierHost" {
                m.method_return()
            } else if interface == "org.freedesktop.DBus.Properties"
                && (member == "Get" || member == "GetAll")
            {
                let ids: Vec<String> = self
                    .items
                    .iter()
                    .map(|i| format!("{}{}", i.service, i.path))
                    .collect();
                let mut props = PropMap::new();
                props.insert(
                    "RegisteredStatusNotifierItems".into(),
                    Variant(Box::new(ids)),
                );
                props.insert(
                    "IsStatusNotifierHostRegistered".into(),
                    Variant(Box::new(true)),
                );
                props.insert("ProtocolVersion".into(), Variant(Box::new(0i32)));
                if member == "GetAll" {
                    m.method_return().append1(props)
                } else {
                    let key = m
                        .read2::<String, String>()
                        .map(|(_, p)| p)
                        .unwrap_or_default();
                    if let Some(v) = props.remove(&key) {
                        m.method_return().append1(v)
                    } else {
                        m.error(
                            &"org.freedesktop.DBus.Error.InvalidArgs".into(),
                            &std::ffi::CString::new("Unknown property").unwrap(),
                        )
                    }
                }
            } else if interface == "org.freedesktop.DBus.Introspectable" {
                m.method_return().append1(INTROSPECTION)
            } else {
                continue;
            };
            let _ = bus.channel.send(reply);
        }
        // Always drain again next iteration if the bounded batch left queued messages.
        let _ = bus.channel.read_write(Some(Duration::ZERO));
        bus.pending = bus.channel.pop_message();
        bus.ready.set(bus.pending.is_some());
        changed
    }
    pub fn update(
        &mut self,
        c: &Config,
        path: &Path,
        generation: u64,
        size: Size<i32, Logical>,
    ) -> bool {
        let registry_changed = self.dispatch();
        if registry_changed {
            self.needs_list = true;
            self.view = None;
        }
        let mut changed = registry_changed && self.open;
        if let Some(result) = self.job.as_ref().and_then(|r| match r.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Background task ended unexpectedly".into()))
            }
        }) {
            self.job = None;
            if self.open {
                match result {
                    Ok(ResultData::Items(items)) => {
                        for item in items {
                            if let Some(current) = self
                                .items
                                .iter_mut()
                                .find(|i| i.service == item.service && i.path == item.path)
                            {
                                current.title = item.title;
                            }
                        }
                    }
                    Ok(ResultData::Menu(item, path, rows)) => {
                        self.menu = Some((item, path));
                        self.rows = rows;
                        self.selected = 0;
                        self.offset = 0;
                    }
                    Ok(ResultData::Done) => self.close(),
                    Err(e) => self.error = Some(e),
                }
                self.view = None;
                changed = true;
            }
        }
        if !self.open {
            return changed;
        }
        if self.needs_list && !self.busy() {
            self.needs_list = false;
            self.start(Job::List(self.items.clone()));
        }
        if self.size != (size.w, size.h) {
            self.size = (size.w, size.h);
            self.view = None;
        }
        if self.generation != generation {
            self.generation = generation;
            self.view = None;
        }
        if self.view.is_none() {
            let width = c.tray.width.min(size.w.max(1));
            let capacity = ((size.h - 150) / 32).clamp(1, 16) as usize;
            let count = self.len();
            self.selected = self.selected.min(count.saturating_sub(1));
            if self.selected < self.offset {
                self.offset = self.selected;
            }
            if self.selected >= self.offset + capacity {
                self.offset = self.selected + 1 - capacity;
            }
            let rows: Vec<String> = if self.menu.is_some() {
                self.rows
                    .iter()
                    .map(|r| {
                        format!(
                            "{}{}{}",
                            if r.enabled { "" } else { "(disabled) " },
                            r.label,
                            if r.submenu { "  >" } else { "" }
                        )
                    })
                    .collect()
            } else {
                self.items.iter().map(|i| i.title.clone()).collect()
            };
            let shown = count.saturating_sub(self.offset).min(capacity);
            let height = (shown.max(1) as i32 * 32 + 92).min(size.h.max(1));
            let rect = Rectangle::new(
                ((size.w - width) / 2, (size.h - height) / 2).into(),
                (width, height).into(),
            );
            if let Ok(font) = panels::load_font(&c.panels, path) {
                let bg = color(&c.panels.background).unwrap();
                let fg = color(&c.panels.foreground).unwrap();
                let accent = color(&c.panels.accent).unwrap();
                let buffer = ui::surface(width, height, |p| {
                    panels::fill(p, (width, height), (0, 0, width, height), bg);
                    ui::text(
                        p,
                        (width, height),
                        &font,
                        if self.menu.is_some() {
                            "Tray menu   (Left: back)"
                        } else {
                            "Tray apps   (right-click: menu)"
                        },
                        15.,
                        14,
                        14,
                        fg,
                    );
                    for (n, label) in rows.iter().skip(self.offset).take(capacity).enumerate() {
                        let y = 52 + n as i32 * 32;
                        if n + self.offset == self.selected {
                            panels::fill(p, (width, height), (8, y - 3, width - 16, 29), accent);
                        }
                        ui::text(
                            p,
                            (width, height),
                            &font,
                            &ui::fit(&font, label, c.tray.font_size, width - 34),
                            13.,
                            16,
                            y,
                            fg,
                        );
                    }
                    if count == 0 {
                        ui::text(
                            p,
                            (width, height),
                            &font,
                            if self.busy() {
                                "Loading..."
                            } else {
                                "No tray apps registered"
                            },
                            13.,
                            16,
                            52,
                            fg,
                        );
                    }
                    if let Some(e) = &self.error {
                        ui::text(
                            p,
                            (width, height),
                            &font,
                            &ui::fit(&font, e, 11., width - 28),
                            11.,
                            14,
                            height - 24,
                            fg,
                        );
                    }
                });
                self.view = Some(ui::View::overlay(rect, buffer));
                changed = true;
            }
        }
        changed
    }
}
fn connect(handle: LoopHandle<'static, CalloopData>) -> Result<Bus, String> {
    let mut channel = Channel::get_private(BusType::Session).map_err(|e| e.to_string())?;
    let req = Message::new_method_call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "RequestName",
    )
    .unwrap()
    .append2(NAME, 4u32);
    let reply = channel
        .send_with_reply_and_block(req, Duration::from_secs(2))
        .map_err(|e| e.to_string())?;
    if !matches!(reply.read1::<u32>(), Ok(1 | 4)) {
        return Err("Another tray watcher owns this session".into());
    }
    let req = Message::new_method_call(
        "org.freedesktop.DBus",
        "/org/freedesktop/DBus",
        "org.freedesktop.DBus",
        "AddMatch",
    )
    .unwrap()
    .append1("type='signal',interface='org.freedesktop.DBus',member='NameOwnerChanged'");
    channel
        .send_with_reply_and_block(req, Duration::from_secs(2))
        .map_err(|e| e.to_string())?;
    channel.set_watch_enabled(true);
    // SAFETY: duplicate the live bus fd; calloop owns only the duplicate.
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
        pending: None,
        channel,
        ready,
        token,
    })
}
fn run(job: Job) -> Result<ResultData, String> {
    let bus = dbus::blocking::Connection::new_session().map_err(|e| e.to_string())?;
    match job {
        Job::List(mut items) => {
            let until = std::time::Instant::now() + Duration::from_secs(2);
            for item in &mut items {
                if std::time::Instant::now() >= until {
                    break;
                }
                let proxy = bus.with_proxy(&item.service, &item.path, Duration::from_millis(300));
                use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
                if let Ok(title) = proxy.get::<String>(ITEM, "Title") {
                    if !title.is_empty() {
                        item.title = title.chars().take(256).collect();
                    }
                }
            }
            Ok(ResultData::Items(items))
        }
        Job::Activate(item) => {
            use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
            if bus
                .with_proxy(&item.service, &item.path, Duration::from_secs(1))
                .get::<bool>(ITEM, "ItemIsMenu")
                .unwrap_or(false)
            {
                return run(Job::Menu(item, 0, None));
            }
            bus.with_proxy(item.service, item.path, Duration::from_secs(1))
                .method_call::<(), _, _, _>(ITEM, "Activate", (0i32, 0i32))
                .map_err(|e| e.to_string())?;
            Ok(ResultData::Done)
        }
        Job::Event(item, path, id) => {
            bus.with_proxy(item.service, path, Duration::from_secs(1))
                .method_call::<(), _, _, _>(
                    "com.canonical.dbusmenu",
                    "Event",
                    (id, "clicked", Variant(0i32), 0u32),
                )
                .map_err(|e| e.to_string())?;
            Ok(ResultData::Done)
        }
        Job::Menu(item, id, path) => {
            use dbus::blocking::stdintf::org_freedesktop_dbus::Properties;
            let path = match path {
                Some(p) => p,
                None => bus
                    .with_proxy(&item.service, &item.path, Duration::from_secs(1))
                    .get::<dbus::Path<'static>>(ITEM, "Menu")
                    .map(|p| p.to_string())
                    .or_else(|_| {
                        bus.with_proxy(&item.service, &item.path, Duration::from_secs(1))
                            .method_call::<(), _, _, _>(ITEM, "ContextMenu", (0i32, 0i32))?;
                        Ok::<String, dbus::Error>(String::new())
                    })
                    .map_err(|e| e.to_string())?,
            };
            if path == "/" {
                bus.with_proxy(&item.service, &item.path, Duration::from_secs(1))
                    .method_call::<(), _, _, _>(ITEM, "ContextMenu", (0i32, 0i32))
                    .map_err(|e| e.to_string())?;
                return Ok(ResultData::Done);
            }
            if path.is_empty() {
                return Ok(ResultData::Done);
            }
            let proxy = bus.with_proxy(&item.service, &path, Duration::from_secs(1));
            let _: Result<(bool,), _> =
                proxy.method_call("com.canonical.dbusmenu", "AboutToShow", (id,));
            type Layout = (i32, PropMap, Vec<Variant<Box<dyn RefArg>>>);
            let (_, layout): (u32, Layout) = proxy
                .method_call(
                    "com.canonical.dbusmenu",
                    "GetLayout",
                    (id, 1i32, Vec::<String>::new()),
                )
                .map_err(|e| e.to_string())?;
            let mut rows = Vec::new();
            for child in layout.2.into_iter().take(128) {
                if let Some(mut fields) = child.0.as_iter() {
                    let id = fields.next().and_then(|v| v.as_i64()).unwrap_or(-1) as i32;
                    let props = fields.next();
                    let mut label = String::new();
                    let mut enabled = true;
                    let mut visible = true;
                    let mut submenu = false;
                    let mut separator = false;
                    if let Some(props) = props.and_then(|p| p.as_iter()) {
                        let values: Vec<_> = props.collect();
                        for pair in values.chunks(2) {
                            if pair.len() != 2 {
                                continue;
                            }
                            let key = pair[0].as_str().unwrap_or("");
                            let v = pair[1]
                                .as_iter()
                                .and_then(|mut i| i.next())
                                .unwrap_or(pair[1]);
                            match key {
                                "label" => {
                                    label = v
                                        .as_str()
                                        .unwrap_or("")
                                        .replace('_', "")
                                        .chars()
                                        .take(256)
                                        .collect()
                                }
                                "enabled" => enabled = v.as_u64() != Some(0),
                                "visible" => visible = v.as_u64() != Some(0),
                                "children-display" => submenu = v.as_str() == Some("submenu"),
                                "type" => separator = v.as_str() == Some("separator"),
                                "toggle-state" if v.as_i64() == Some(1) => {
                                    label = format!("✓ {label}");
                                }
                                _ => (),
                            }
                        }
                    }
                    if visible && !separator {
                        rows.push(Row {
                            label,
                            id,
                            enabled,
                            submenu,
                        });
                    }
                }
            }
            Ok(ResultData::Menu(item, path, rows))
        }
    }
}
const INTROSPECTION: &str = r#"<node><interface name="org.kde.StatusNotifierWatcher"><method name="RegisterStatusNotifierItem"><arg type="s" direction="in"/></method><method name="RegisterStatusNotifierHost"><arg type="s" direction="in"/></method><property name="RegisteredStatusNotifierItems" type="as" access="read"/><property name="IsStatusNotifierHostRegistered" type="b" access="read"/><property name="ProtocolVersion" type="i" access="read"/><signal name="StatusNotifierItemRegistered"><arg type="s"/></signal><signal name="StatusNotifierItemUnregistered"><arg type="s"/></signal><signal name="StatusNotifierHostRegistered"/></interface></node>"#;
