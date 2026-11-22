//! Desktop entry launcher: scan and draw on demand, then drop the index and pixels.
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
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc::{self, Receiver},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub width: i32,
    pub font_size: f32,
    pub rows: usize,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            width: 640,
            font_size: 14.,
            rows: 8,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(320..=1200).contains(&self.width)
            || !self.font_size.is_finite()
            || !(10.0..=24.0).contains(&self.font_size)
            || !(3..=16).contains(&self.rows)
        {
            Err("launcher dimensions/font/rows out of range".into())
        } else {
            Ok(())
        }
    }
}
#[derive(Clone)]
struct App {
    id: String,
    name: String,
    detail: String,
    terms: String,
    path: PathBuf,
}
#[derive(Clone, Default, Serialize, Deserialize)]
struct Usage {
    count: u64,
    last: u64,
}
type History = HashMap<String, Usage>;
struct Index {
    apps: Vec<App>,
    history: History,
}
fn history_path() -> PathBuf {
    std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".local/state")
        })
        .join("emberwm/launcher-usage.json")
}
fn bounded_read(path: &Path, limit: usize) -> Option<Vec<u8>> {
    use std::io::Read;
    let mut result = Vec::new();
    std::fs::File::open(path)
        .ok()?
        .take(limit as u64 + 1)
        .read_to_end(&mut result)
        .ok()?;
    (result.len() <= limit).then_some(result)
}
fn load_history(path: &Path) -> History {
    bounded_read(path, 262144)
        .and_then(|v| serde_json::from_slice::<History>(&v).ok())
        .filter(|h| h.len() <= 2048)
        .unwrap_or_default()
}
fn record_usage(path: &Path, id: &str) -> Result<(), String> {
    use std::io::Write;
    use std::os::unix::fs::OpenOptionsExt;
    let mut history = load_history(path);
    let entry = history.entry(id.into()).or_default();
    entry.count = entry.count.saturating_add(1);
    entry.last = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if history.len() > 1024 {
        let mut keys: Vec<_> = history.iter().map(|(k, v)| (k.clone(), v.last)).collect();
        keys.sort_by_key(|(_, t)| *t);
        for (key, _) in keys.into_iter().take(history.len() - 1024) {
            history.remove(&key);
        }
    }
    let parent = path.parent().ok_or("invalid launcher history path")?;
    std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    let temp = path.with_extension(format!("{}.tmp", std::process::id()));
    let result = (|| {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|e| e.to_string())?;
        file.write_all(&serde_json::to_vec(&history).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
        std::fs::rename(&temp, path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(temp);
    }
    result
}
fn data_dirs() -> Vec<PathBuf> {
    let home = PathBuf::from(std::env::var_os("HOME").unwrap_or_default());
    let mut dirs = vec![
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share")),
        home.join(".local/share/flatpak/exports/share"),
    ];
    dirs.extend(std::env::split_paths(
        &std::env::var_os("XDG_DATA_DIRS").unwrap_or_else(|| "/usr/local/share:/usr/share".into()),
    ));
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/share"));
    let mut seen = HashSet::new();
    dirs.retain(|d| seen.insert(d.clone()));
    dirs
}
fn unescape(s: &str) -> String {
    let mut chars = s.chars();
    let mut out = String::new();
    while let Some(c) = chars.next() {
        if c == '\\' {
            out.push(match chars.next() {
                Some('s') => ' ',
                Some('n') => '\n',
                Some('t') => '\t',
                Some('r') => '\r',
                Some(c) => c,
                None => '\\',
            });
        } else {
            out.push(c);
        }
    }
    out
}
fn parse_entry(path: &Path, id: String, desktop: &str, locale: &str) -> Option<App> {
    let data = bounded_read(path, 131072)?;
    let text = std::str::from_utf8(&data).ok()?;
    let mut values = HashMap::new();
    let mut group = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            group = line == "[Desktop Entry]";
        } else if group && !line.starts_with('#') {
            if let Some((key, value)) = line.split_once('=') {
                values.insert(key.trim(), value.trim());
            }
        }
    }
    if values.get("Type") != Some(&"Application")
        || ["Hidden", "NoDisplay"]
            .iter()
            .any(|k| values.get(k) == Some(&"true"))
    {
        return None;
    }
    if values.get("OnlyShowIn").is_some_and(|v| {
        !v.split(';').any(|d| {
            desktop
                .split(':')
                .any(|current| d.eq_ignore_ascii_case(current))
        })
    }) || values.get("NotShowIn").is_some_and(|v| {
        v.split(';').any(|d| {
            desktop
                .split(':')
                .any(|current| d.eq_ignore_ascii_case(current))
        })
    }) {
        return None;
    }
    if !values.contains_key("Exec") && values.get("DBusActivatable") != Some(&"true") {
        return None;
    }
    if values.get("TryExec").is_some_and(|p| !executable(p)) {
        return None;
    }
    let name = values
        .get(format!("Name[{locale}]").as_str())
        .or_else(|| {
            values.get(format!("Name[{}]", locale.split('_').next().unwrap_or(locale)).as_str())
        })
        .or_else(|| values.get("Name"))?;
    let name = unescape(name);
    if name.is_empty() || name.chars().any(char::is_control) {
        return None;
    }
    let detail = unescape(
        values
            .get("GenericName")
            .or_else(|| values.get("Comment"))
            .copied()
            .unwrap_or(""),
    );
    let terms = format!(
        "{} {} {} {}",
        name,
        detail,
        values.get("Keywords").unwrap_or(&""),
        id
    )
    .to_lowercase();
    Some(App {
        id,
        name,
        detail,
        terms,
        path: path.to_path_buf(),
    })
}
fn executable(p: &str) -> bool {
    use std::os::unix::fs::PermissionsExt;
    let exists = |p: &Path| {
        p.metadata()
            .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
    };
    if p.contains('/') {
        exists(Path::new(p))
    } else {
        std::env::var_os("PATH")
            .is_some_and(|v| std::env::split_paths(&v).any(|d| exists(&d.join(p))))
    }
}
fn scan(dirs: &[PathBuf], desktop: &str, locale: &str, cancel: &AtomicBool) -> Vec<App> {
    fn walk(
        base: &Path,
        dir: &Path,
        depth: usize,
        seen: &mut HashSet<String>,
        apps: &mut Vec<App>,
        context: (&str, &str, &AtomicBool),
    ) {
        if depth > 6 || seen.len() >= 4096 || context.2.load(Ordering::Relaxed) {
            return;
        }
        let Ok(entries) = std::fs::read_dir(dir) else {
            return;
        };
        let mut paths: Vec<_> = entries
            .take(4096)
            .filter_map(Result::ok)
            .map(|e| e.path())
            .collect();
        paths.sort();
        for path in paths {
            if seen.len() >= 4096 || context.2.load(Ordering::Relaxed) {
                break;
            }
            if path.is_dir() && !path.is_symlink() {
                walk(base, &path, depth + 1, seen, apps, context);
            } else if path.extension().is_some_and(|e| e == "desktop") {
                let Ok(relative) = path.strip_prefix(base) else {
                    continue;
                };
                let id = relative.to_string_lossy().replace('/', "-");
                if seen.insert(id.clone()) {
                    if let Some(app) = parse_entry(&path, id, context.0, context.1) {
                        apps.push(app);
                    }
                }
            }
        }
    }
    let mut seen = HashSet::new();
    let mut apps = Vec::new();
    for dir in dirs {
        let base = dir.join("applications");
        walk(
            &base,
            &base,
            0,
            &mut seen,
            &mut apps,
            (desktop, locale, cancel),
        );
    }
    apps
}
fn matches(app: &App, query: &str) -> Option<u8> {
    if query.is_empty() {
        return Some(0);
    }
    let name = app.name.to_lowercase();
    if name == query {
        return Some(0);
    }
    if name.starts_with(query) {
        return Some(1);
    }
    if name.split_whitespace().any(|w| w.starts_with(query)) {
        return Some(2);
    }
    if name.contains(query) {
        return Some(3);
    }
    if app.terms.contains(query) {
        return Some(4);
    }
    let mut chars = query.chars();
    let mut next = chars.next();
    for c in name.chars() {
        if Some(c) == next {
            next = chars.next();
            if next.is_none() {
                return Some(5);
            }
        }
    }
    None
}
#[derive(Default)]
pub struct Runtime {
    display: Option<std::ffi::OsString>,
    socket: Option<PathBuf>,
    pub open: bool,
    pub view: Option<ui::View>,
    font: Option<Arc<Font>>,
    index: Option<Index>,
    scan_job: Option<Receiver<Index>>,
    scan_cancel: Option<Arc<AtomicBool>>,
    launch_job: Option<Receiver<Result<(), String>>>,
    query: String,
    results: Vec<usize>,
    cursor: usize,
    hits: Vec<(Rect, usize)>,
    revision: u64,
    render_key: Option<(u64, u64, i32, i32)>,
    rasterizations: u64,
    error: Option<String>,
}
impl Runtime {
    pub fn set_session(&mut self, display: std::ffi::OsString, socket: PathBuf) {
        self.display = Some(display);
        self.socket = Some(socket);
    }
    pub fn busy(&self) -> bool {
        self.scan_job.is_some() || self.launch_job.is_some()
    }
    pub fn toggle(&mut self) {
        if self.open {
            self.close();
            return;
        }
        self.open = true;
        self.query.clear();
        self.cursor = 0;
        self.error = None;
        let (tx, rx) = mpsc::sync_channel(1);
        let dirs = data_dirs();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        self.scan_cancel = Some(cancel);
        let desktop = "Ember".to_string();
        let locale = std::env::var("LC_MESSAGES")
            .or_else(|_| std::env::var("LANG"))
            .unwrap_or_default()
            .split('.')
            .next()
            .unwrap_or("")
            .to_string();
        let history = history_path();
        match std::thread::Builder::new()
            .name("ember-app-index".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let result = Index {
                    apps: scan(&dirs, &desktop, &locale, &worker_cancel),
                    history: load_history(&history),
                };
                let _ = tx.send(result);
                crate::memory::request_reclaim();
            }) {
            Ok(_) => self.scan_job = Some(rx),
            Err(_) => self.error = Some("Could not load applications.".into()),
        }
        self.revision += 1;
    }
    pub fn close(&mut self) {
        self.open = false;
        self.view = None;
        self.font = None;
        self.index = None;
        self.scan_job = None;
        if let Some(cancel) = self.scan_cancel.take() {
            cancel.store(true, Ordering::Relaxed);
        }
        self.query = String::new();
        self.results = Vec::new();
        self.hits = Vec::new();
        self.cursor = 0;
        self.error = None;
        self.render_key = None;
        self.revision += 1;
        crate::memory::request_reclaim();
    }
    fn rank(&mut self) {
        self.results.clear();
        if let Some(index) = &self.index {
            let query = self.query.to_lowercase();
            let mut ranked: Vec<_> = index
                .apps
                .iter()
                .enumerate()
                .filter_map(|(i, a)| matches(a, &query).map(|score| (i, score)))
                .collect();
            ranked.sort_by(|(a, sa), (b, sb)| {
                let a = &index.apps[*a];
                let b = &index.apps[*b];
                let ua = index.history.get(&a.id).cloned().unwrap_or_default();
                let ub = index.history.get(&b.id).cloned().unwrap_or_default();
                sa.cmp(sb)
                    .then(ub.count.cmp(&ua.count))
                    .then(ub.last.cmp(&ua.last))
                    .then(a.name.cmp(&b.name))
            });
            self.results = ranked.into_iter().map(|(i, _)| i).collect();
        }
        self.cursor = self.cursor.min(self.results.len().saturating_sub(1));
        self.revision += 1;
    }
    pub fn take_launch_error(&mut self) -> Option<String> {
        if let Some(receiver) = &self.launch_job {
            match receiver.try_recv() {
                Ok(result) => {
                    self.launch_job = None;
                    return result.err();
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.launch_job = None;
                    return Some("Application launch worker stopped.".into());
                }
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        None
    }
    fn launch(&mut self, index: usize) {
        let Some(app) = self.index.as_ref().and_then(|i| i.apps.get(index)).cloned() else {
            return;
        };
        if self.launch_job.is_some() {
            return;
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let history = history_path();
        let name = app.name.clone();
        let display = self.display.clone();
        let socket = self.socket.clone();
        match std::thread::Builder::new()
            .name("ember-app-launch".into())
            .stack_size(256 * 1024)
            .spawn(move || {
                let result = (|| {
                    let mut command = Command::new("gio");
                    command
                        .arg("launch")
                        .arg(&app.path)
                        .env("XDG_CURRENT_DESKTOP", "Ember");
                    if let Some(display) = display {
                        command.env("WAYLAND_DISPLAY", display);
                    }
                    if let Some(socket) = socket {
                        command.env("EMBER_SOCKET", socket);
                    }
                    let mut child = command
                        .stdin(Stdio::null())
                        .stdout(Stdio::null())
                        .stderr(Stdio::null())
                        .spawn()
                        .map_err(|_| format!("Could not launch {name}; gio is required."))?;
                    let deadline = Instant::now() + Duration::from_secs(15);
                    loop {
                        match child.try_wait() {
                            Ok(Some(status)) if status.success() => {
                                let _ = record_usage(&history, &app.id);
                                return Ok(());
                            }
                            Ok(Some(_)) => {
                                return Err(format!(
                                    "Could not launch {name}. Check its desktop entry."
                                ))
                            }
                            Ok(None) if Instant::now() < deadline => {
                                std::thread::sleep(Duration::from_millis(30))
                            }
                            _ => {
                                let _ = child.kill();
                                let _ = child.wait();
                                return Err(format!("Launch timed out: {name}."));
                            }
                        }
                    }
                })();
                let _ = tx.send(result);
                crate::memory::request_reclaim();
            }) {
            Ok(_) => {
                self.launch_job = Some(rx);
                self.close();
            }
            Err(_) => {
                self.error = Some("Could not start application.".into());
                self.revision += 1;
            }
        }
    }
    pub fn key(&mut self, key: Keysym, mods: &ModifiersState) {
        match key {
            Keysym::Escape => self.close(),
            Keysym::Return | Keysym::KP_Enter => {
                if let Some(i) = self.results.get(self.cursor).copied() {
                    self.launch(i);
                }
            }
            Keysym::Up => self.scroll(false),
            Keysym::Down | Keysym::Tab => self.scroll(!mods.shift),
            Keysym::BackSpace => {
                self.query.pop();
                self.cursor = 0;
                self.rank();
            }
            _ if mods.ctrl && key == Keysym::u => {
                self.query.clear();
                self.rank();
            }
            _ if !mods.ctrl && !mods.alt && !mods.logo => {
                if let Some(c) =
                    char::from_u32(xkb::keysym_to_utf32(key)).filter(|c| !c.is_control())
                {
                    if self.query.len() + c.len_utf8() <= 128 {
                        self.query.push(c);
                        self.cursor = 0;
                        self.rank();
                    }
                }
            }
            _ => (),
        }
    }
    pub fn scroll(&mut self, down: bool) {
        if !self.results.is_empty() {
            self.cursor = if down {
                (self.cursor + 1).min(self.results.len() - 1)
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
        if let Some((_, index)) = self.hits.iter().find(|(r, _)| r.to_f64().contains(pos)) {
            self.launch(*index);
        }
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"open":self.open,"loading":self.scan_job.is_some(),"launching":self.launch_job.is_some(),"query":self.query,"cursor":self.cursor,"error":self.error,"count":self.index.as_ref().map_or(0,|i|i.apps.len()),"results":self.index.as_ref().map(|index|self.results.iter().take(32).map(|i|{let a=&index.apps[*i];serde_json::json!({"id":a.id,"name":a.name,"uses":index.history.get(&a.id).map_or(0,|u|u.count)})}).collect::<Vec<_>>()).unwrap_or_default(),"buffer_bytes":self.view.as_ref().map_or(0,|v|v.rect.size.w as u64*v.rect.size.h as u64*4),"font_loaded":self.font.is_some(),"rasterizations":self.rasterizations,"view":self.view.as_ref().map(|v|serde_json::json!({"x":v.rect.loc.x,"y":v.rect.loc.y,"width":v.rect.size.w,"height":v.rect.size.h}))})
    }
    pub fn update(
        &mut self,
        config: &Config,
        path: &Path,
        generation: u64,
        size: Size<i32, Logical>,
    ) -> bool {
        if !config.launcher.enabled && self.open {
            self.close();
            return true;
        }
        if !self.open {
            return false;
        }
        if let Some(job) = &self.scan_job {
            match job.try_recv() {
                Ok(index) => {
                    self.index = Some(index);
                    self.scan_job = None;
                    self.rank();
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.error = Some("Could not load applications.".into());
                    self.scan_job = None;
                    self.revision += 1;
                }
                Err(mpsc::TryRecvError::Empty) => (),
            }
        }
        let key = (self.revision, generation, size.w, size.h);
        if self.render_key == Some(key) {
            return false;
        }
        self.render_key = Some(key);
        let Ok(font) = panels::load_font(&config.panels, path) else {
            return false;
        };
        self.font = Some(font.clone());
        let px = config.launcher.font_size;
        let row = px.ceil() as i32 + 28;
        let width = config.launcher.width.min((size.w - 32).max(1));
        let rows = config
            .launcher
            .rows
            .min(((size.h - 150) / row).max(1) as usize);
        let height = (118 + row * rows as i32).min((size.h - 32).max(1));
        let rect = Rectangle::new(
            ((size.w - width) / 2, (size.h - height) / 2).into(),
            (width, height).into(),
        );
        let fg = color(&config.panels.foreground).unwrap();
        let bg = color(&config.panels.background).unwrap();
        let accent = color(&config.panels.accent).unwrap();
        let border = color(&config.panels.border_color).unwrap();
        let mut hits = Vec::new();
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
                &font,
                "Applications",
                px + 2.,
                18,
                12,
                accent,
            );
            panels::fill(pixels, (width, height), (14, 44, width - 28, 32), border);
            ui::text(
                pixels,
                (width, height),
                &font,
                &ui::fit(
                    &font,
                    if self.query.is_empty() {
                        "Search apps…"
                    } else {
                        &self.query
                    },
                    px,
                    width - 48,
                ),
                px,
                22,
                49,
                fg,
            );
            if let Some(index) = &self.index {
                let start = self.cursor / rows * rows;
                for (i, app_index) in self.results.iter().enumerate().skip(start).take(rows) {
                    let app = &index.apps[*app_index];
                    let y = 86 + (i - start) as i32 * row;
                    if i == self.cursor {
                        panels::fill(
                            pixels,
                            (width, height),
                            (12, y, width - 24, row - 3),
                            border,
                        );
                    }
                    ui::text(
                        pixels,
                        (width, height),
                        &font,
                        &ui::fit(&font, &app.name, px, width - 40),
                        px,
                        20,
                        y + 3,
                        if i == self.cursor { accent } else { fg },
                    );
                    ui::text(
                        pixels,
                        (width, height),
                        &font,
                        &ui::fit(&font, &app.detail, 11., width - 40),
                        11.,
                        20,
                        y + px.ceil() as i32 + 7,
                        fg,
                    );
                    hits.push((
                        Rectangle::new(
                            (rect.loc.x + 12, rect.loc.y + y).into(),
                            (width - 24, row - 3).into(),
                        ),
                        *app_index,
                    ));
                }
            }
            if self.scan_job.is_some() || self.results.is_empty() || self.error.is_some() {
                ui::text(
                    pixels,
                    (width, height),
                    &font,
                    self.error.as_deref().unwrap_or(if self.scan_job.is_some() {
                        "Loading applications…"
                    } else {
                        "No matching applications"
                    }),
                    px,
                    20,
                    90,
                    fg,
                );
            }
            ui::text(
                pixels,
                (width, height),
                &font,
                &ui::fit(
                    &font,
                    "↑↓ / scroll: choose · Enter: launch · Esc: close · ranked by usage",
                    10.,
                    width - 36,
                ),
                10.,
                18,
                height - 24,
                fg,
            );
        });
        self.hits = hits;
        self.view = Some(ui::View::overlay(rect, buffer));
        self.rasterizations += 1;
        true
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prefix_matches_rank_by_usage_and_close_releases_index() {
        let mut r = Runtime {
            open: true,
            ..Default::default()
        };
        r.index = Some(Index {
            apps: vec![
                App {
                    id: "files.desktop".into(),
                    name: "Files".into(),
                    detail: String::new(),
                    terms: "files".into(),
                    path: "/tmp/files.desktop".into(),
                },
                App {
                    id: "firefox.desktop".into(),
                    name: "Firefox".into(),
                    detail: String::new(),
                    terms: "firefox browser".into(),
                    path: "/tmp/firefox.desktop".into(),
                },
            ],
            history: HashMap::from([("firefox.desktop".into(), Usage { count: 9, last: 1 })]),
        });
        r.key(Keysym::f, &ModifiersState::default());
        r.key(Keysym::i, &ModifiersState::default());
        assert_eq!(r.index.as_ref().unwrap().apps[r.results[0]].name, "Firefox");
        r.close();
        assert!(r.index.is_none() && r.view.is_none() && r.query.is_empty());
    }
    #[test]
    fn user_hidden_entry_masks_system_entry_and_localized_names_work() {
        let base = std::env::temp_dir().join(format!("ember-apps-{}", std::process::id()));
        let user = base.join("user/applications");
        let system = base.join("system/applications");
        std::fs::create_dir_all(&user).unwrap();
        std::fs::create_dir_all(&system).unwrap();
        std::fs::write(
            user.join("hidden.desktop"),
            "[Desktop Entry]\nHidden=true\n",
        )
        .unwrap();
        std::fs::write(
            system.join("hidden.desktop"),
            "[Desktop Entry]\nType=Application\nName=Hidden\nExec=true\n",
        )
        .unwrap();
        std::fs::write(
            system.join("app.desktop"),
            "[Desktop Entry]\nType=Application\nName=Browser\nName[pl]=Przeglądarka\nExec=true\n",
        )
        .unwrap();
        let apps = scan(
            &[base.join("user"), base.join("system")],
            "Ember",
            "pl_PL",
            &AtomicBool::new(false),
        );
        assert_eq!(apps.len(), 1);
        assert_eq!(apps[0].name, "Przeglądarka");
        std::fs::remove_dir_all(base).unwrap();
    }
}
