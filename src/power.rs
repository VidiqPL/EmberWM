//! Native, on-demand power controls. No resident power daemon is required.
use crate::{
    config::{color, Config},
    layout::Rect,
    panels, ui,
};
use serde::Serialize;
use smithay::{
    input::keyboard::Keysym,
    utils::{Logical, Point},
};
use std::{
    fs,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::mpsc::{self, Receiver},
    time::{Duration, Instant},
};
const HELPER: &str = "/usr/local/libexec/emberwm-power";
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Action {
    Logout,
    Suspend,
    Reboot,
    PowerOff,
}
impl Action {
    fn label(self) -> &'static str {
        match self {
            Self::Logout => "Log out",
            Self::Suspend => "Suspend",
            Self::Reboot => "Restart",
            Self::PowerOff => "Shut down",
        }
    }
}
#[derive(Clone, Default, Serialize)]
pub struct Snapshot {
    driver: String,
    governor: String,
    preference: String,
    supported: Vec<String>,
    cpu_temperature: Option<f64>,
    gpu_mode: String,
    gpu_temperature: Option<f64>,
    gpu_power: Option<f64>,
    helper: bool,
}
impl Snapshot {
    fn same_display(&self, other: &Self) -> bool {
        // Match the integer precision actually drawn, while keeping exact values in IPC.
        let shown = |v: Option<f64>| v.map(f64::round_ties_even);
        self.driver == other.driver
            && self.governor == other.governor
            && self.preference == other.preference
            && self.supported == other.supported
            && self.helper == other.helper
            && self.gpu_mode == other.gpu_mode
            && shown(self.cpu_temperature) == shown(other.cpu_temperature)
            && shown(self.gpu_temperature) == shown(other.gpu_temperature)
            && shown(self.gpu_power) == shown(other.gpu_power)
    }
}
fn read(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path)
        .unwrap_or_default()
        .trim()
        .to_string()
}
fn entries(path: impl AsRef<Path>) -> Vec<PathBuf> {
    fs::read_dir(path)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .collect()
}
fn number(path: impl AsRef<Path>, scale: f64) -> Option<f64> {
    read(path).parse::<f64>().ok().map(|v| v / scale)
}
fn snapshot() -> Snapshot {
    let policies = entries("/sys/devices/system/cpu/cpufreq")
        .into_iter()
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| n.to_string_lossy().starts_with("policy"))
        })
        .collect::<Vec<_>>();
    let mut result = Snapshot {
        helper: Path::new(HELPER).is_file(),
        ..Default::default()
    };
    if let Some(p) = policies.first() {
        result.driver = read(p.join("scaling_driver"));
        result.governor = read(p.join("scaling_governor"));
        result.preference = read(p.join("energy_performance_preference"));
        result.supported = [
            ("performance", "performance"),
            ("balanced", "balance_performance"),
            ("power-saver", "power"),
        ]
        .into_iter()
        .filter(|(_, epp)| {
            policies.iter().all(|p| {
                read(p.join("energy_performance_available_preferences"))
                    .split_whitespace()
                    .any(|s| s == *epp)
            })
        })
        .map(|(label, _)| label.to_string())
        .collect();
        if policies
            .iter()
            .any(|p| read(p.join("energy_performance_preference")) != result.preference)
        {
            result.preference = "mixed".into();
        }
    }
    for h in entries("/sys/class/hwmon") {
        if matches!(
            read(h.join("name")).as_str(),
            "k10temp" | "coretemp" | "zenpower"
        ) {
            result.cpu_temperature = number(h.join("temp1_input"), 1000.);
            break;
        }
    }
    for card in entries("/sys/class/drm") {
        let device = card.join("device");
        let mode = read(device.join("power_dpm_force_performance_level"));
        if mode.is_empty() {
            continue;
        }
        result.gpu_mode = mode;
        if let Some(h) = entries(device.join("hwmon")).first() {
            result.gpu_temperature = number(h.join("temp1_input"), 1000.);
            result.gpu_power = number(h.join("power1_average"), 1_000_000.);
        }
        break;
    }
    result
}
struct AuthAgent(Option<std::process::Child>);
impl AuthAgent {
    fn start() -> Self {
        // Duplicate agents fail registration and exit. Only our child is reaped.
        let child = Command::new("/usr/libexec/polkit-kde-authentication-agent-1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .ok();
        if child.is_some() {
            std::thread::sleep(Duration::from_millis(350));
        }
        Self(child)
    }
}
impl Drop for AuthAgent {
    fn drop(&mut self) {
        if let Some(child) = &mut self.0 {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
fn set_profile(profile: &str) -> Result<(), String> {
    if !Path::new(HELPER).is_file() {
        return Err("Power helper is not installed. See docs/CONTROL_CENTER.md.".into());
    }
    let _agent = AuthAgent::start();
    let mut child = Command::new("pkexec")
        .arg("--disable-internal-agent")
        .arg(HELPER)
        .arg(profile)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + Duration::from_secs(120);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(100)),
            other => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(match other {
                    Err(e) => e.to_string(),
                    _ => "Authorization timed out.".into(),
                });
            }
        }
    }
    let output = child.wait_with_output().map_err(|e| e.to_string())?;
    if output.status.success() {
        Ok(())
    } else {
        let message = String::from_utf8_lossy(&output.stderr)
            .chars()
            .take(240)
            .collect::<String>();
        Err(if message.trim().is_empty() {
            "Power change was cancelled or denied.".into()
        } else {
            message
        })
    }
}
fn session_action(action: Action) -> Result<(), String> {
    let _agent = AuthAgent::start();
    let connection = dbus::blocking::Connection::new_system().map_err(|e| e.to_string())?;
    let proxy = connection.with_proxy(
        "org.freedesktop.login1",
        "/org/freedesktop/login1",
        Duration::from_secs(120),
    );
    let method = match action {
        Action::Suspend => "Suspend",
        Action::Reboot => "Reboot",
        Action::PowerOff => "PowerOff",
        Action::Logout => return Ok(()),
    };
    proxy
        .method_call::<(), _, _, _>("org.freedesktop.login1.Manager", method, (true,))
        .map_err(|e| e.to_string())
}
enum Request {
    Read,
    Profile(String),
    Session(Action),
}
#[derive(Default)]
pub struct Runtime {
    pub open: bool,
    pub view: Option<ui::View>,
    pub confirm: Option<Action>,
    pub release_modal: bool,
    pending_action: Option<String>,
    feedback: Option<String>,
    snapshot: Snapshot,
    job: Option<Receiver<Result<Snapshot, String>>>,
    next: Option<Instant>,
    message: String,
    selected: usize,
    buttons: Vec<(Rect, usize)>,
    key: Option<(u64, Rect)>,
    rasterizations: u64,
}
impl Runtime {
    pub fn open(&mut self) {
        self.open = true;
        self.selected = 0;
        self.confirm = None;
        self.key = None;
        self.start(Request::Read);
    }
    pub fn close(&mut self) {
        self.open = false;
        self.release_modal = false;
        self.view = None;
        self.confirm = None;
        self.snapshot = Snapshot::default();
        self.message = String::new();
        self.buttons = Vec::new();
        self.key = None;
        self.next = None;
        crate::memory::request_reclaim();
    }
    pub fn take_feedback(&mut self) -> Option<String> {
        self.feedback.take()
    }
    pub fn busy(&self) -> bool {
        self.job.is_some()
    }
    pub fn wake(&self) -> Option<Duration> {
        if self.open {
            self.next
                .map(|n| n.saturating_duration_since(Instant::now()))
        } else {
            None
        }
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"open":self.open,"busy":self.busy(),"rasterizations":self.rasterizations,"confirmation":self.confirm,"snapshot":self.snapshot,"message":self.message,"buffer_bytes":self.view.as_ref().map_or(0,|v|v.rect.size.w as u64*v.rect.size.h as u64*4),"buttons":self.buttons.iter().map(|(r,i)|serde_json::json!({"index":i,"x":r.loc.x,"y":r.loc.y,"width":r.size.w,"height":r.size.h})).collect::<Vec<_>>()})
    }
    fn start(&mut self, request: Request) {
        if self.busy() {
            return;
        }
        self.pending_action = match &request {
            Request::Read => None,
            Request::Profile(_) => Some("CPU profile applied.".into()),
            Request::Session(a) => Some(format!("{} requested.", a.label())),
        };
        self.release_modal = !matches!(&request, Request::Read);
        let (tx, rx) = mpsc::sync_channel(1);
        match std::thread::Builder::new()
            .name("ember-power".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let result = match request {
                    Request::Read => Ok(()),
                    Request::Profile(p) => set_profile(&p),
                    Request::Session(a) => session_action(a),
                }
                .map(|()| snapshot());
                let _ = tx.send(result);
            }) {
            Ok(_) => {
                self.job = Some(rx);
                self.next = None;
            }
            Err(e) => {
                self.message = e.to_string();
                self.release_modal = false;
                self.pending_action = None;
                self.next = Some(Instant::now() + Duration::from_secs(3));
            }
        }
        if self.release_modal || !self.message.is_empty() && self.job.is_none() {
            self.key = None;
        }
    }
    pub fn cancel_confirmation(&mut self) -> bool {
        let had = self.confirm.take().is_some();
        if had {
            self.key = None;
        }
        had
    }
    pub fn scroll(&mut self, down: bool) {
        self.selected = if down {
            (self.selected + 1).min(6)
        } else {
            self.selected.saturating_sub(1)
        };
        self.key = None;
    }
    fn choose(&mut self, index: usize) -> bool {
        if self.busy() {
            return false;
        }
        if index == 7 {
            self.confirm = None;
            self.key = None;
            return false;
        }
        if index == 8 {
            if let Some(action) = self.confirm.take() {
                self.key = None;
                if action == Action::Logout {
                    return true;
                }
                self.message = format!("{} requested…", action.label());
                self.start(Request::Session(action));
            }
            return false;
        }
        if index < 3 {
            if !self.snapshot.helper {
                self.message = "Install the CPU profile helper; see docs/CONTROL_CENTER.md.".into();
                self.key = None;
                return false;
            }
            let profile = ["performance", "balanced", "power-saver"][index];
            if !self.snapshot.supported.iter().any(|p| p == profile) {
                self.message = "This CPU driver does not expose that preference.".into();
            } else {
                self.message = "Authorizing CPU profile change…".into();
                self.start(Request::Profile(profile.into()));
            }
        } else {
            self.confirm = Some(
                [
                    Action::Logout,
                    Action::Suspend,
                    Action::Reboot,
                    Action::PowerOff,
                ][index - 3],
            );
        }
        self.key = None;
        false
    }
    pub fn key(&mut self, key: Keysym) -> bool {
        match key {
            Keysym::Up | Keysym::Left => self.scroll(false),
            Keysym::Down | Keysym::Right => self.scroll(true),
            Keysym::Return | Keysym::KP_Enter => {
                return self.choose(if self.confirm.is_some() {
                    8
                } else {
                    self.selected
                })
            }
            Keysym::F5 => self.start(Request::Read),
            _ => (),
        }
        false
    }
    pub fn click(&mut self, pos: Point<f64, Logical>, right: bool) -> bool {
        if right {
            self.cancel_confirmation();
            return false;
        }
        if let Some(index) = self
            .buttons
            .iter()
            .find(|(r, _)| r.to_f64().contains(pos))
            .map(|(_, i)| *i)
        {
            return self.choose(index);
        }
        false
    }
    pub fn update(&mut self, c: &Config, path: &Path, generation: u64, rect: Option<Rect>) -> bool {
        let mut changed = false;
        if let Some(result) = self.job.as_ref().and_then(|r| match r.try_recv() {
            Ok(v) => Some(v),
            Err(mpsc::TryRecvError::Disconnected) => {
                Some(Err("Power worker ended unexpectedly".into()))
            }
            Err(mpsc::TryRecvError::Empty) => None,
        }) {
            self.job = None;
            if let Some(message) = self.pending_action.take() {
                self.feedback = Some(match &result {
                    Ok(_) => message,
                    Err(e) => format!("Power action failed: {e}"),
                });
            }
            if self.open {
                match result {
                    Ok(snapshot) => {
                        changed = !self.snapshot.same_display(&snapshot);
                        self.snapshot = snapshot;
                        if self.message == "Authorizing CPU profile change…" {
                            self.message = "CPU profile applied.".into();
                            changed = true;
                        }
                    }
                    Err(error) => {
                        changed = self.message != error;
                        self.message = error;
                    }
                }
                if changed {
                    self.key = None;
                }
                self.next = Some(Instant::now() + Duration::from_secs(3));
            } else {
                // The tab may have closed before its short-lived worker completed.
                crate::memory::request_reclaim();
            }
        }
        if !self.open {
            return changed;
        }
        if self.next.is_some_and(|t| Instant::now() >= t) {
            self.start(Request::Read);
        }
        let Some(rect) = rect else {
            return changed;
        };
        if self.key == Some((generation, rect)) {
            return changed;
        }
        self.key = Some((generation, rect));
        let Ok(font) = panels::load_font(&c.panels, path) else {
            return changed;
        };
        let (w, h) = (rect.size.w, rect.size.h);
        let compact = h < 420;
        let bg = color(&c.panels.background).unwrap();
        let fg = color(&c.panels.foreground).unwrap();
        let accent = color(&c.panels.accent).unwrap();
        let border = color(&c.panels.border_color).unwrap();
        let mut buttons = Vec::new();
        let buffer = ui::surface(w, h, |p| {
            panels::fill(p, (w, h), (0, 0, w, h), bg);
            for edge in [(0, 0, 1, h), (w - 1, 0, 1, h), (0, h - 1, w, 1)] {
                panels::fill(p, (w, h), edge, accent);
            }
            let text = |p: &mut [u8], s: &str, y: i32, tint| {
                ui::text(
                    p,
                    (w, h),
                    &font,
                    &ui::fit(&font, s, 13., w - 36),
                    13.,
                    18,
                    y,
                    tint,
                )
            };
            let mut button = |p: &mut [u8],
                              label: &str,
                              index: usize,
                              x: i32,
                              y: i32,
                              width: i32,
                              active: bool| {
                panels::fill(
                    p,
                    (w, h),
                    (x, y, width, 34),
                    if active { accent } else { border },
                );
                ui::text(
                    p,
                    (w, h),
                    &font,
                    &ui::fit(&font, label, 12., width - 16),
                    12.,
                    x + 8,
                    y + 9,
                    fg,
                );
                buttons.push((
                    Rect::new((rect.loc.x + x, rect.loc.y + y).into(), (width, 34).into()),
                    index,
                ));
            };
            if let Some(action) = self.confirm {
                text(p, &format!("{} this session?", action.label()), 24, fg);
                text(p, "Save your work before continuing.", 58, accent);
                button(p, "Cancel", 7, 18, 105, (w - 48) / 2, false);
                button(p, "Confirm", 8, w / 2 + 6, 105, (w - 48) / 2, true);
                text(p, "Esc: cancel  /  Enter: confirm", 160, accent);
                return;
            }
            text(p, "CPU power preference", 18, fg);
            text(
                p,
                &if self.snapshot.driver.is_empty() {
                    "CPU power preferences unavailable".into()
                } else {
                    format!("{}  /  {}", self.snapshot.driver, self.snapshot.preference)
                },
                45,
                accent,
            );
            let width = (w - 48) / 3;
            for (i, label, epp) in [
                (0, "Performance", "performance"),
                (1, "Balanced", "balance_performance"),
                (2, "Power saver", "power"),
            ] {
                let active = self.snapshot.preference == epp;
                button(
                    p,
                    &format!("{}{}", if active { "• " } else { "" }, label),
                    i,
                    18 + i as i32 * (width + 6),
                    77,
                    width,
                    self.selected == i
                        && self.snapshot.helper
                        && !self.snapshot.supported.is_empty(),
                );
            }
            let cpu = self
                .snapshot
                .cpu_temperature
                .map_or("—".into(), |t| format!("{t:.0}°C"));
            let gpu = self
                .snapshot
                .gpu_temperature
                .map_or("—".into(), |t| format!("{t:.0}°C"));
            let watts = self
                .snapshot
                .gpu_power
                .map_or("—".into(), |p| format!("{p:.0} W"));
            if compact {
                text(p, &format!("CPU {cpu}  /  GPU {gpu}  /  {watts}"), 132, fg);
            } else {
                text(
                    p,
                    &format!("CPU temperature  {cpu}  /  boost and clock limits preserved"),
                    132,
                    fg,
                );
                text(
                    p,
                    &format!(
                        "GPU  {}  /  {gpu}  /  {watts}",
                        if self.snapshot.gpu_mode.is_empty() {
                            "driver managed"
                        } else {
                            &self.snapshot.gpu_mode
                        }
                    ),
                    162,
                    fg,
                );
                text(
                    p,
                    "Profiles change CPU preference; GPU settings are unchanged.",
                    190,
                    accent,
                );
            }
            text(p, "Session", if compact { 160 } else { 236 }, fg);
            let width = (w - 42) / 2;
            for (i, action) in [
                Action::Logout,
                Action::Suspend,
                Action::Reboot,
                Action::PowerOff,
            ]
            .into_iter()
            .enumerate()
            {
                button(
                    p,
                    action.label(),
                    i + 3,
                    18 + (i % 2) as i32 * (width + 6),
                    if compact {
                        190 + (i / 2) as i32 * 40
                    } else {
                        266 + (i / 2) as i32 * 42
                    },
                    width,
                    self.selected == i + 3,
                );
            }
            let hint = if self.pending_action.is_some() {
                "Working…"
            } else if !self.snapshot.helper {
                "CPU profile helper unavailable; session controls still work."
            } else {
                "↑↓: choose  /  Enter: apply  /  F5: refresh"
            };
            if compact {
                text(
                    p,
                    if self.message.is_empty() {
                        hint
                    } else {
                        &self.message
                    },
                    h - 26,
                    accent,
                );
            } else {
                text(p, hint, h - 65, accent);
            }
            if !compact && !self.message.is_empty() {
                text(p, &self.message, h - 36, fg);
            }
        });
        self.buttons = buttons;
        self.view = Some(ui::View::overlay(rect, buffer));
        self.rasterizations += 1;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn unchanged_displayed_samples_reuse_pixels_but_keep_precise_ipc_values() {
        let config = Config::default();
        let path = Path::new("/tmp/config.toml");
        let rect = Some(Rect::new((0, 0).into(), (640, 518).into()));
        let mut runtime = Runtime {
            open: true,
            snapshot: Snapshot {
                driver: "fixture-epp".into(),
                cpu_temperature: Some(50.1),
                gpu_temperature: Some(32.1),
                gpu_power: Some(5.1),
                ..Default::default()
            },
            ..Default::default()
        };
        assert!(runtime.update(&config, path, 0, rect));
        assert_eq!(runtime.rasterizations, 1);
        let (tx, rx) = mpsc::sync_channel(1);
        let mut sample = runtime.snapshot.clone();
        sample.cpu_temperature = Some(50.2);
        sample.gpu_power = Some(5.2);
        tx.send(Ok(sample)).unwrap();
        runtime.job = Some(rx);
        assert!(!runtime.update(&config, path, 0, rect));
        assert_eq!(runtime.rasterizations, 1);
        assert_eq!(runtime.snapshot.cpu_temperature, Some(50.2));
        let (tx, rx) = mpsc::sync_channel(1);
        let mut sample = runtime.snapshot.clone();
        sample.cpu_temperature = Some(51.2);
        tx.send(Ok(sample)).unwrap();
        runtime.job = Some(rx);
        assert!(runtime.update(&config, path, 0, rect));
        assert_eq!(runtime.rasterizations, 2);
        runtime.close();
        assert!(runtime.wake().is_none() && runtime.view.is_none());
    }
    #[test]
    fn session_actions_require_confirmation_and_close_releases_state() {
        let mut r = Runtime {
            open: true,
            ..Default::default()
        };
        assert!(!r.choose(3));
        assert_eq!(r.confirm, Some(Action::Logout));
        assert!(!r.busy());
        assert!(r.cancel_confirmation());
        assert!(!r.choose(8));
        assert!(!r.choose(3));
        assert!(r.choose(8));
        assert!(!r.choose(6));
        assert_eq!(r.confirm, Some(Action::PowerOff));
        assert!(!r.choose(7));
        assert!(r.confirm.is_none() && !r.busy());
        r.confirm = None;
        let area = Rect::new((0, 0).into(), (400, 298).into());
        r.update(
            &Config::default(),
            Path::new("/tmp/config.toml"),
            0,
            Some(area),
        );
        assert!(r
            .buttons
            .iter()
            .all(|(rect, _)| rect.loc.y + rect.size.h <= area.size.h));
        r.close();
        assert!(r.view.is_none() && r.buttons.is_empty() && r.wake().is_none());
    }
}
