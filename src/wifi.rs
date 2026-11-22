//! On-demand native Wi-Fi popup. No polling, worker, font or pixel buffer at idle.
mod network;
use crate::{
    config::{color, Config},
    layout::Rect,
    panels, ui,
};
use fontdue::Font;
use serde::{Deserialize, Serialize};
use smithay::{
    input::keyboard::{xkb, Keysym, ModifiersState},
    utils::{Logical, Point, Rectangle, Size},
};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    thread::JoinHandle,
};

#[derive(Clone, Debug, Deserialize, Serialize)]
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
            width: 540,
            font_size: 13.0,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(320..=1200).contains(&self.width)
            || !self.font_size.is_finite()
            || !(10.0..=24.0).contains(&self.font_size)
        {
            Err("wifi width/font size out of range".into())
        } else {
            Ok(())
        }
    }
}

#[derive(Clone, Default, Serialize)]
pub struct Network {
    pub ssid: String,
    pub bssid: String,
    pub signal: u8,
    pub security: String,
    pub device: String,
    pub active: bool,
    pub path: String,
}
impl Network {
    fn enterprise(&self) -> bool {
        self.security.contains("802.1X") || self.security.contains("EAP")
    }
    fn personal(&self) -> bool {
        !self.enterprise() && self.security.contains("WPA")
    }
}
#[derive(Default)]
pub struct Snapshot {
    networks: Vec<Network>,
    devices: Vec<String>,
    radio: bool,
}

/// Fixed storage avoids leaving old allocations behind while editing a password.
struct Secret {
    bytes: [u8; 256],
    len: usize,
}
impl Default for Secret {
    fn default() -> Self {
        Self {
            bytes: [0; 256],
            len: 0,
        }
    }
}
impl Secret {
    fn is_empty(&self) -> bool {
        self.len == 0
    }
    fn as_str(&self) -> &str {
        std::str::from_utf8(&self.bytes[..self.len]).unwrap()
    }
    fn push(&mut self, c: char) {
        if c.is_control() || self.len + c.len_utf8() > self.bytes.len() {
            return;
        }
        c.encode_utf8(&mut self.bytes[self.len..self.len + c.len_utf8()]);
        self.len += c.len_utf8();
    }
    fn pop(&mut self) {
        if let Some((idx, _)) = self.as_str().char_indices().last() {
            for b in &mut self.bytes[idx..self.len] {
                unsafe {
                    std::ptr::write_volatile(b, 0);
                }
            }
            self.len = idx;
        }
    }
    fn clear(&mut self) {
        for b in &mut self.bytes {
            unsafe {
                std::ptr::write_volatile(b, 0);
            }
        }
        self.len = 0;
    }
}
impl Drop for Secret {
    fn drop(&mut self) {
        self.clear();
    }
}

enum Request {
    Scan(bool),
    Connect(Network, Box<Secret>),
    Disconnect(Network),
}
struct Reply {
    snapshot: Option<Snapshot>,
    message: String,
    failed: bool,
}
struct Job {
    receiver: Receiver<Reply>,
    _handle: JoinHandle<()>,
    cancel: Arc<AtomicBool>,
    mutation: bool,
}
impl Drop for Job {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}
#[derive(Clone, Copy)]
enum Hit {
    Close,
    Refresh,
    Select(usize),
    Back,
    Activate,
}
struct Button {
    rect: Rect,
    hit: Hit,
}

#[derive(Default)]
pub struct Runtime {
    pub open: bool,
    pub view: Option<ui::View>,
    snapshot: Snapshot,
    selected: Option<Network>,
    cursor: usize,
    password: Secret,
    job: Option<Job>,
    message: String,
    failed: bool,
    revision: u64,
    render_key: Option<(u64, u64, i32, i32)>,
    font: Option<Arc<Font>>,
    buttons: Vec<Button>,
    rasterizations: u64,
    visible_rows: usize,
    refresh_after_job: bool,
}
impl Runtime {
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
        } else {
            self.open = true;
            self.refresh_after_job = self.job.is_some();
            self.message = "Looking for Wi-Fi networks…".into();
            self.failed = false;
            self.start(Request::Scan(false));
            self.revision += 1;
        }
    }
    pub fn close(&mut self) {
        self.open = false;
        self.password.clear();
        self.selected = None;
        self.snapshot = Snapshot::default();
        self.cursor = 0;
        self.message.clear();
        self.buttons = Vec::new();
        self.view = None;
        self.font = None;
        self.render_key = None;
        if let Some(job) = &self.job {
            if !job.mutation {
                job.cancel.store(true, Ordering::Relaxed);
            }
        }
        self.revision += 1;
        crate::memory::request_reclaim();
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    fn start(&mut self, request: Request) {
        if self.job.is_some() {
            return;
        }
        let mutation = !matches!(request, Request::Scan(_));
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, receiver) = mpsc::sync_channel(1);
        let result = std::thread::Builder::new()
            .name("ember-wifi".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let action = match request {
                    Request::Scan(rescan) => {
                        let result = network::scan(rescan, &worker_cancel);
                        let reply = match result {
                            Ok(snapshot) => Reply {
                                snapshot: Some(snapshot),
                                message: String::new(),
                                failed: false,
                            },
                            Err(message) => Reply {
                                snapshot: None,
                                message,
                                failed: true,
                            },
                        };
                        let _ = sender.send(reply);
                        return;
                    }
                    Request::Connect(n, p) => {
                        network::connect(&n, *p, &worker_cancel).map(|_| "Connected.".to_string())
                    }
                    Request::Disconnect(n) => {
                        network::disconnect(&n).map(|_| "Disconnected.".to_string())
                    }
                };
                let failed = action.is_err();
                let message = action.unwrap_or_else(|e| e);
                let snapshot = network::scan(false, &worker_cancel).ok();
                let _ = sender.send(Reply {
                    snapshot,
                    message,
                    failed,
                });
            });
        match result {
            Ok(handle) => {
                self.job = Some(Job {
                    receiver,
                    _handle: handle,
                    cancel,
                    mutation,
                })
            }
            Err(_) => {
                self.message = "Could not start network operation.".into();
                self.failed = true;
            }
        }
        self.revision += 1;
    }
    fn poll(&mut self) {
        let reply = self
            .job
            .as_ref()
            .and_then(|job| match job.receiver.try_recv() {
                Ok(reply) => Some(reply),
                Err(mpsc::TryRecvError::Disconnected) => Some(Reply {
                    snapshot: None,
                    message: "Network operation ended unexpectedly.".into(),
                    failed: true,
                }),
                Err(mpsc::TryRecvError::Empty) => None,
            });
        if let Some(reply) = reply {
            let job = self.job.take().unwrap();
            // Sending is the worker's last operation; detach its handle without blocking input.
            drop(job);
            if self.open && self.refresh_after_job {
                self.refresh_after_job = false;
                self.start(Request::Scan(false));
                return;
            }
            if self.open {
                if let Some(snapshot) = reply.snapshot {
                    self.snapshot = snapshot;
                    if let Some(old) = self.selected.take() {
                        self.selected = self
                            .snapshot
                            .networks
                            .iter()
                            .find(|n| {
                                n.ssid == old.ssid
                                    && n.device == old.device
                                    && n.security == old.security
                            })
                            .cloned();
                    }
                    self.cursor = self
                        .cursor
                        .min(self.snapshot.networks.len().saturating_sub(1));
                }
                self.message = reply.message;
                self.failed = reply.failed;
            }
            self.revision += 1;
        }
    }
    fn select(&mut self, index: usize) {
        if self.busy() {
            return;
        }
        self.password.clear();
        self.selected = self.snapshot.networks.get(index).cloned();
        self.cursor = index;
        self.message.clear();
        self.failed = false;
        self.revision += 1;
    }
    fn activate(&mut self) {
        if self.busy() {
            return;
        }
        let Some(n) = self.selected.clone() else {
            self.select(self.cursor);
            return;
        };
        self.message = if n.active {
            "Disconnecting…"
        } else {
            "Connecting…"
        }
        .into();
        self.failed = false;
        let secret = std::mem::take(&mut self.password);
        if n.active {
            self.start(Request::Disconnect(n));
        } else {
            self.start(Request::Connect(n, Box::new(secret)));
        }
    }
    fn back(&mut self) {
        self.password.clear();
        self.selected = None;
        self.message.clear();
        self.failed = false;
        self.revision += 1;
    }
    /// All keys are consumed while open, keeping typed passwords out of Wayland clients.
    pub fn key(&mut self, sym: Keysym, mods: &ModifiersState) {
        match sym {
            Keysym::Escape => {
                if self.selected.is_some() && !self.busy() {
                    self.back();
                } else {
                    self.close();
                }
            }
            Keysym::Return | Keysym::KP_Enter => self.activate(),
            _ if self.busy() => (),
            Keysym::Up | Keysym::Down | Keysym::Tab if self.selected.is_none() => {
                if !self.snapshot.networks.is_empty() {
                    if sym == Keysym::Up || (sym == Keysym::Tab && mods.shift) {
                        self.cursor = self.cursor.saturating_sub(1);
                    } else {
                        self.cursor = (self.cursor + 1).min(self.snapshot.networks.len() - 1);
                    }
                    self.revision += 1;
                }
            }
            Keysym::BackSpace => {
                self.password.pop();
                self.revision += 1;
            }
            _ if mods.ctrl && !mods.alt && !mods.logo && sym == Keysym::u => {
                self.password.clear();
                self.revision += 1;
            }
            Keysym::F5 => self.refresh(),
            _ if !mods.ctrl
                && !mods.alt
                && !mods.logo
                && self
                    .selected
                    .as_ref()
                    .is_some_and(|n| n.personal() && !n.active) =>
            {
                if let Some(c) = char::from_u32(xkb::keysym_to_utf32(sym)) {
                    self.password.push(c);
                    self.revision += 1;
                }
            }
            _ => (),
        }
    }
    pub fn refresh(&mut self) {
        if !self.busy() {
            self.password.clear();
            self.message = "Refreshing networks…".into();
            self.failed = false;
            self.start(Request::Scan(true));
        }
    }
    pub fn scroll(&mut self, down: bool) {
        if !self.busy() && self.selected.is_none() && !self.snapshot.networks.is_empty() {
            self.cursor = if down {
                (self.cursor + 1).min(self.snapshot.networks.len() - 1)
            } else {
                self.cursor.saturating_sub(1)
            };
            self.revision += 1;
        }
    }
    pub fn click(&mut self, pos: Point<f64, Logical>, right: bool) {
        if right
            || self
                .view
                .as_ref()
                .is_none_or(|v| !v.rect.to_f64().contains(pos))
        {
            self.close();
            return;
        }
        let hit = self
            .buttons
            .iter()
            .find(|b| b.rect.to_f64().contains(pos))
            .map(|b| b.hit);
        match hit {
            Some(Hit::Close) => self.close(),
            Some(_) if self.busy() => (),
            Some(Hit::Back) => self.back(),
            Some(Hit::Refresh) => self.refresh(),
            Some(Hit::Select(n)) => self.select(n),
            Some(Hit::Activate) => self.activate(),
            None => (),
        }
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"open":self.open,"busy":self.busy(),"networks":self.snapshot.networks,"devices":self.snapshot.devices,"radio_enabled":self.snapshot.radio,"selected":self.selected,"cursor":self.cursor,"password_entered":!self.password.is_empty(),"message":self.message,"failed":self.failed,"font_loaded":self.font.is_some(),"buffer_bytes":self.view.as_ref().map_or(0,|v|v.rect.size.w as u64*v.rect.size.h as u64*4),"rasterizations":self.rasterizations,"view":self.view.as_ref().map(|v|serde_json::json!({"x":v.rect.loc.x,"y":v.rect.loc.y,"width":v.rect.size.w,"height":v.rect.size.h})),"buttons":self.buttons.iter().map(|b|serde_json::json!({"action":match b.hit {Hit::Close=>"close",Hit::Refresh=>"refresh",Hit::Back=>"back",Hit::Activate=>"activate",Hit::Select(_)=>"select"},"index":match b.hit { Hit::Select(n)=>Some(n),_=>None },"x":b.rect.loc.x,"y":b.rect.loc.y,"width":b.rect.size.w,"height":b.rect.size.h})).collect::<Vec<_>>()})
    }
    pub fn update(
        &mut self,
        config: &Config,
        path: &Path,
        generation: u64,
        size: Size<i32, Logical>,
    ) -> bool {
        let revision = self.revision;
        self.poll();
        if !config.wifi.enabled && self.open {
            self.close();
        }
        if !self.open {
            return self.revision != revision;
        }
        let key = (self.revision, generation, size.w, size.h);
        if self.render_key == Some(key) {
            return false;
        }
        self.render_key = Some(key);
        match panels::load_font(&config.panels, path) {
            Ok(font) => self.font = Some(font),
            Err(_) => {
                self.message = "Could not load panel font.".into();
                self.failed = true;
                return true;
            }
        }
        self.draw(config, size);
        true
    }
    fn draw(&mut self, config: &Config, size: Size<i32, Logical>) {
        let font = self.font.as_ref().unwrap();
        let px = config.wifi.font_size;
        let line = px.ceil() as i32 + 8;
        let row = line + 22;
        let width = config.wifi.width.min((size.w - 32).max(1));
        let max_height = (size.h - 32).max(1);
        let rows = ((max_height - 170) / row).clamp(1, 8) as usize;
        self.visible_rows = rows;
        let height = if self.selected.is_some() {
            286
        } else {
            170 + row * self.snapshot.networks.len().clamp(1, rows) as i32
        }
        .min(max_height);
        let rect = Rectangle::new(
            ((size.w - width) / 2, (size.h - height) / 2).into(),
            (width, height).into(),
        );
        let fg = color(&config.panels.foreground).unwrap();
        let bg = color(&config.panels.background).unwrap();
        let accent = color(&config.panels.accent).unwrap();
        let border = color(&config.panels.border_color).unwrap();
        let mut buttons = Vec::new();
        let buffer = ui::surface(width, height, |pixels| {
            panels::fill(pixels, (width, height), (0, 0, width, height), bg);
            for edge in [
                (0, 0, width, 1),
                (0, height - 1, width, 1),
                (0, 0, 1, height),
                (width - 1, 0, 1, height),
            ] {
                panels::fill(pixels, (width, height), edge, accent);
            }
            ui::text(
                pixels,
                (width, height),
                font,
                "Wi-Fi",
                px + 3.,
                18,
                14,
                accent,
            );
            let mut button = |pixels: &mut [u8], hit, label: &str, x, y, w, h| {
                panels::fill(pixels, (width, height), (x, y, w, h), border);
                ui::text(
                    pixels,
                    (width, height),
                    font,
                    &ui::fit(font, label, px, w - 12),
                    px,
                    x + 6,
                    y + 5,
                    fg,
                );
                buttons.push(Button {
                    rect: Rectangle::new((rect.loc.x + x, rect.loc.y + y).into(), (w, h).into()),
                    hit,
                });
            };
            button(pixels, Hit::Close, "×", width - 40, 10, 28, 30);
            if let Some(n) = &self.selected {
                ui::text(
                    pixels,
                    (width, height),
                    font,
                    &ui::fit(font, &n.ssid, px + 1., width - 36),
                    px + 1.,
                    18,
                    55,
                    fg,
                );
                ui::text(
                    pixels,
                    (width, height),
                    font,
                    &ui::fit(
                        font,
                        &format!("{} · {}% · {}", n.security, n.signal, n.device),
                        px,
                        width - 36,
                    ),
                    px,
                    18,
                    84,
                    accent,
                );
                let hint = if n.active {
                    "Connected"
                } else if n.personal() {
                    "Password · leave blank to use saved credentials"
                } else if n.enterprise() || n.security.contains("WEP") {
                    "Requires an existing NetworkManager profile"
                } else {
                    "No password required"
                };
                ui::text(
                    pixels,
                    (width, height),
                    font,
                    &ui::fit(font, hint, px, width - 36),
                    px,
                    18,
                    117,
                    fg,
                );
                if n.personal() && !n.active {
                    panels::fill(pixels, (width, height), (18, 146, width - 36, 32), border);
                    let masked = if self.password.is_empty() {
                        "Type password here…".into()
                    } else {
                        "•".repeat(self.password.as_str().chars().count().min(64))
                    };
                    ui::text(
                        pixels,
                        (width, height),
                        font,
                        &ui::fit(font, &masked, px, width - 52),
                        px,
                        26,
                        151,
                        fg,
                    );
                }
                button(pixels, Hit::Back, "Back", 18, height - 62, 82, 32);
                button(
                    pixels,
                    Hit::Activate,
                    if self.busy() {
                        "Please wait…"
                    } else if n.active {
                        "Disconnect"
                    } else {
                        "Connect"
                    },
                    width - 154,
                    height - 62,
                    136,
                    32,
                );
            } else {
                let heading = if self.busy() {
                    "Looking for networks…"
                } else if self.snapshot.devices.is_empty() {
                    "No Wi-Fi adapter found"
                } else if !self.snapshot.radio {
                    "Wi-Fi is disabled or blocked"
                } else if self.snapshot.networks.is_empty() {
                    "No networks found · try Refresh"
                } else {
                    "Choose a network"
                };
                ui::text(
                    pixels,
                    (width, height),
                    font,
                    &ui::fit(font, heading, px, width - 144),
                    px,
                    18,
                    56,
                    fg,
                );
                button(pixels, Hit::Refresh, "Refresh", width - 112, 48, 94, 30);
                let start = self.cursor / rows * rows;
                for (idx, n) in self
                    .snapshot
                    .networks
                    .iter()
                    .enumerate()
                    .skip(start)
                    .take(rows)
                {
                    let y = 90 + (idx - start) as i32 * row;
                    if idx == self.cursor {
                        panels::fill(
                            pixels,
                            (width, height),
                            (12, y, width - 24, row - 4),
                            border,
                        );
                    }
                    ui::text(
                        pixels,
                        (width, height),
                        font,
                        &ui::fit(font, &n.ssid, px, width - 140),
                        px,
                        20,
                        y + 4,
                        if n.active { accent } else { fg },
                    );
                    ui::text(
                        pixels,
                        (width, height),
                        font,
                        &format!("{}%", n.signal),
                        px,
                        width - 65,
                        y + 4,
                        accent,
                    );
                    let subtitle = format!(
                        "{} · {}{}",
                        n.security,
                        n.device,
                        if n.active { " · connected" } else { "" }
                    );
                    ui::text(
                        pixels,
                        (width, height),
                        font,
                        &ui::fit(font, &subtitle, px.min(11.), width - 40),
                        px.min(11.),
                        20,
                        y + line,
                        fg,
                    );
                    buttons.push(Button {
                        rect: Rectangle::new(
                            (rect.loc.x + 12, rect.loc.y + y).into(),
                            (width - 24, row - 4).into(),
                        ),
                        hit: Hit::Select(idx),
                    });
                }
            }
            if !self.message.is_empty() {
                let y = if self.selected.is_some() {
                    height - 98
                } else {
                    height - 56
                };
                ui::text(
                    pixels,
                    (width, height),
                    font,
                    &ui::fit(font, &self.message, px.min(11.), width - 36),
                    px.min(11.),
                    18,
                    y,
                    if self.failed {
                        [1., 0.4, 0.4, 1.]
                    } else {
                        accent
                    },
                );
            }
            ui::text(
                pixels,
                (width, height),
                font,
                &ui::fit(
                    font,
                    if self.selected.is_some() {
                        "Enter: apply · Ctrl+U: clear · Esc: back/close"
                    } else {
                        "↑↓ / scroll: choose · Enter: open · F5: refresh · Esc: close"
                    },
                    10.,
                    width - 36,
                ),
                10.,
                18,
                height - 23,
                fg,
            );
        });
        self.buttons = buttons;
        self.view = Some(ui::View::overlay(rect, buffer));
        self.rasterizations += 1;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn password_editing_handles_unicode_and_wipes_storage() {
        let mut s = Secret::default();
        s.push('a');
        s.push('é');
        s.push('🐈');
        s.pop();
        assert_eq!(s.as_str(), "aé");
        assert!(s.bytes[s.len..].iter().all(|b| *b == 0));
        s.clear();
        assert!(s.bytes.iter().all(|b| *b == 0));
        for _ in 0..300 {
            s.push('x');
        }
        assert_eq!(s.len, 256);
        s.push('\n');
        assert_eq!(s.len, 256);
    }
    #[test]
    fn popup_caches_pixels_masks_password_and_releases_resources() {
        let config = Config::default();
        let path = Path::new("/tmp/config.toml");
        let mut r = Runtime {
            open: true,
            ..Default::default()
        };
        r.snapshot = Snapshot {
            radio: true,
            devices: vec!["wlan0".into()],
            networks: vec![Network {
                ssid: "Test network".into(),
                security: "WPA2".into(),
                ..Default::default()
            }],
        };
        r.select(0);
        r.key(Keysym::a, &ModifiersState::default());
        assert!(r.update(&config, path, 0, (1280, 800).into()));
        assert!(r.status()["password_entered"].as_bool().unwrap());
        assert!(!r.status().to_string().contains("\"password\":"));
        for _ in 0..100 {
            assert!(!r.update(&config, path, 0, (1280, 800).into()));
        }
        assert_eq!(r.rasterizations, 1);
        r.close();
        assert!(r.view.is_none() && r.font.is_none() && r.password.is_empty());
        assert_eq!(r.status()["buffer_bytes"], 0);
    }
    #[test]
    fn keyboard_and_pointer_navigation_and_reload_disable() {
        let mut r = Runtime {
            open: true,
            ..Default::default()
        };
        let mut config = Config::default();
        let path = Path::new("/tmp/config.toml");
        r.snapshot.networks = vec![
            Network {
                ssid: "First".into(),
                ..Default::default()
            },
            Network {
                ssid: "Second".into(),
                ..Default::default()
            },
        ];
        r.key(Keysym::Down, &ModifiersState::default());
        r.key(Keysym::Return, &ModifiersState::default());
        assert_eq!(r.selected.as_ref().unwrap().ssid, "Second");
        r.key(Keysym::Escape, &ModifiersState::default());
        assert!(r.open && r.selected.is_none());
        r.update(&config, path, 0, (640, 400).into());
        let b = r
            .buttons
            .iter()
            .find(|b| matches!(b.hit, Hit::Select(0)))
            .unwrap();
        let pos = b.rect.loc.to_f64() + Point::from((2., 2.));
        r.click(pos, false);
        assert_eq!(r.selected.as_ref().unwrap().ssid, "First");
        config.wifi.enabled = false;
        r.update(&config, path, 1, (640, 400).into());
        assert!(!r.open && r.view.is_none());
    }
}
