//! Small compositor-native panels. No panel daemon, shell loops or UI toolkit.
use crate::{
    config::{color, valid_action},
    layout::Rect,
};
use fontdue::{Font, FontSettings};
use serde::{Deserialize, Serialize};
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::{Logical, Point, Rectangle, Size, Transform},
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    ffi::CString,
    io::Read,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    sync::{Arc, Weak},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub font: String,
    pub font_size: f32,
    pub height: i32,
    pub margin: i32,
    pub padding: i32,
    pub spacing: i32,
    pub workspace_padding: i32,
    pub background: String,
    pub foreground: String,
    pub accent: String,
    pub border_color: String,
    pub border_width: i32,
    pub opacity: f32,
    pub hide_empty_workspaces: bool,
    pub hide_on_fullscreen: bool,
    pub poll_interval_secs: u64,
    pub items: Vec<Item>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: false,
            font: "builtin".into(),
            font_size: 11.0,
            height: 22,
            margin: 8,
            padding: 8,
            spacing: 7,
            workspace_padding: 4,
            background: "#211317".into(),
            foreground: "#ead8dd".into(),
            accent: "#cf4763".into(),
            border_color: "#4c2835".into(),
            border_width: 0,
            opacity: 0.94,
            hide_empty_workspaces: true,
            hide_on_fullscreen: true,
            poll_interval_secs: 5,
            items: vec![],
        }
    }
}
#[derive(Clone, Debug, PartialEq, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Item {
    pub name: String,
    pub enabled: bool,
    pub anchor: String,
    pub modules: Vec<String>,
    /// 0 = content width, -1 = screen width minus margins.
    pub width: i32,
    pub height: Option<i32>,
    pub font_size: Option<f32>,
    pub margin: Option<i32>,
    pub offset_x: i32,
    pub offset_y: i32,
    pub reserve_space: bool,
    pub clock_format: String,
    pub date_format: String,
    pub max_title_chars: usize,
    pub background: Option<String>,
    pub foreground: Option<String>,
    pub accent: Option<String>,
    pub on_click: Option<String>,
}
impl Default for Item {
    fn default() -> Self {
        Self {
            name: String::new(),
            enabled: true,
            anchor: "top-left".into(),
            modules: vec![],
            width: 0,
            height: None,
            font_size: None,
            margin: None,
            offset_x: 0,
            offset_y: 0,
            reserve_space: true,
            clock_format: "%H:%M".into(),
            date_format: "%a %d %b".into(),
            max_title_chars: 32,
            background: None,
            foreground: None,
            accent: None,
            on_click: None,
        }
    }
}
const MODULES: &[&str] = &[
    "workspaces",
    "clock",
    "date",
    "title",
    "layout",
    "memory",
    "cpu",
    "uptime",
    "volume",
];
const ANCHORS: &[&str] = &[
    "top-left",
    "top-center",
    "top-right",
    "bottom-left",
    "bottom-center",
    "bottom-right",
];
impl Settings {
    pub fn active(&self) -> impl Iterator<Item = &Item> {
        self.items
            .iter()
            .filter(|p| self.enabled && p.enabled && !p.modules.is_empty())
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.font_size.is_finite() || !(8.0..=32.0).contains(&self.font_size) {
            return Err("panels.font_size must be 8..32".into());
        }
        if !(12..=128).contains(&self.height)
            || !(0..=200).contains(&self.margin)
            || !(0..=40).contains(&self.padding)
            || !(0..=40).contains(&self.spacing)
            || !(0..=24).contains(&self.workspace_padding)
            || !(0..=4).contains(&self.border_width)
        {
            return Err("panel dimensions out of range".into());
        }
        if !self.opacity.is_finite() || !(0.0..=1.0).contains(&self.opacity) {
            return Err("panels.opacity must be 0..1".into());
        }
        if !(1..=600).contains(&self.poll_interval_secs) {
            return Err("panel polling interval must be 1..600 seconds".into());
        }
        if self.items.len() > 16 {
            return Err("at most 16 panels are supported".into());
        }
        for c in [
            &self.background,
            &self.foreground,
            &self.accent,
            &self.border_color,
        ] {
            color(c)?;
        }
        let mut names = HashSet::new();
        for p in &self.items {
            if p.name.is_empty() || p.name.len() > 48 || !names.insert(&p.name) {
                return Err("panel names must be nonempty and unique (max 48 bytes)".into());
            }
            if !ANCHORS.contains(&p.anchor.as_str()) {
                return Err(format!("unknown panel anchor: {}", p.anchor));
            }
            if p.modules.len() > 12 {
                return Err("at most 12 modules per panel".into());
            }
            for module in &p.modules {
                if !MODULES.contains(&module.as_str()) && !module.starts_with("text:") {
                    return Err(format!("unknown panel module: {module}"));
                }
                if module.len() > 256 || module.chars().any(char::is_control) {
                    return Err("panel text is too long or contains control characters".into());
                }
            }
            if p.width < -1
                || p.width > 8192
                || p.offset_x.unsigned_abs() > 8192
                || p.offset_y.unsigned_abs() > 8192
            {
                return Err("panel width/offset out of range".into());
            }
            let px = p.font_size.unwrap_or(self.font_size);
            if !px.is_finite()
                || !(8.0..=32.0).contains(&px)
                || p.height.unwrap_or(self.height) < px.ceil() as i32 + 4
                || p.height.unwrap_or(self.height) > 128
            {
                return Err(
                    "panel height must fit its font (font size 8..32, height <=128)".into(),
                );
            }
            if !(0..=200).contains(&p.margin.unwrap_or(self.margin))
                || !(1..=120).contains(&p.max_title_chars)
            {
                return Err("panel margin or title length out of range".into());
            }
            for fmt in [&p.clock_format, &p.date_format] {
                if fmt.len() > 128 || fmt.contains('\0') {
                    return Err("invalid panel time format".into());
                }
            }
            for c in [&p.background, &p.foreground, &p.accent]
                .into_iter()
                .flatten()
            {
                color(c)?;
            }
            if p.on_click.as_ref().is_some_and(|a| !valid_action(a)) {
                return Err("unknown panel click action".into());
            }
        }
        Ok(())
    }
}
fn font_path(settings: &Settings, config_path: &Path) -> Option<PathBuf> {
    if settings.font == "builtin" {
        None
    } else {
        let path = PathBuf::from(&settings.font);
        Some(if path.is_absolute() {
            path
        } else {
            config_path.parent().unwrap_or(Path::new(".")).join(path)
        })
    }
}
type FontCache = Option<(PathBuf, Option<SystemTime>, Weak<Font>)>;
thread_local! {
    static FONT_CACHE: RefCell<FontCache> = const { RefCell::new(None) };
}
pub(crate) fn load_font(settings: &Settings, config_path: &Path) -> Result<Arc<Font>, String> {
    let path = font_path(settings, config_path).unwrap_or_else(|| PathBuf::from("<builtin>"));
    let modified = std::fs::metadata(&path)
        .ok()
        .and_then(|m| m.modified().ok());
    if let Some(font) = FONT_CACHE.with(|cache| {
        cache
            .borrow()
            .as_ref()
            .filter(|(p, m, _)| p == &path && m == &modified)
            .and_then(|(_, _, w)| w.upgrade())
    }) {
        return Ok(font);
    }

    let bytes = if let Some(path) = font_path(settings, config_path) {
        let len = std::fs::metadata(&path)
            .map_err(|e| format!("panel font {}: {e}", path.display()))?
            .len();
        if len > 16 * 1024 * 1024 {
            return Err("panel font must be <=16 MiB".into());
        }
        std::borrow::Cow::Owned(std::fs::read(path).map_err(|e| e.to_string())?)
    } else {
        std::borrow::Cow::Borrowed(include_bytes!("../assets/fonts/EmberPanelMono.ttf").as_slice())
    };
    let font = Font::from_bytes(
        bytes,
        FontSettings {
            load_substitutions: false,
            ..FontSettings::default()
        },
    )
    .map_err(|e| format!("invalid panel font: {e}"))?;
    if font.horizontal_line_metrics(settings.font_size).is_none() {
        return Err("panel font has no horizontal line metrics".into());
    }
    let font = Arc::new(font);
    FONT_CACHE.with(|cache| *cache.borrow_mut() = Some((path, modified, Arc::downgrade(&font))));
    Ok(font)
}
pub fn validate_resources(settings: &Settings, path: &Path) -> Result<(), String> {
    if settings.active().next().is_some() && settings.font != "builtin" {
        load_font(settings, path)?;
    }
    Ok(())
}
pub fn work_area(settings: &Settings, size: Size<i32, Logical>) -> Rect {
    let mut top = 0;
    let mut bottom = 0;
    for p in settings.active().filter(|p| p.reserve_space) {
        let height = p.height.unwrap_or(settings.height);
        let margin = p.margin.unwrap_or(settings.margin);
        if p.anchor.starts_with("top-") {
            top = top.max(height + margin + p.offset_y);
        } else {
            bottom = bottom.max(height + margin - p.offset_y);
        }
    }
    top = top.clamp(0, (size.h - 64).max(0));
    bottom = bottom.clamp(0, (size.h - top - 64).max(0));
    Rectangle::new(
        (0, top).into(),
        (size.w, (size.h - top - bottom).max(1)).into(),
    )
}

#[derive(Clone)]
pub struct Input {
    pub size: Size<i32, Logical>,
    pub workspace: usize,
    pub workspaces: usize,
    pub occupied: Vec<usize>,
    pub title: String,
    pub floating: bool,
    pub fullscreen: bool,
}
impl Input {
    pub fn from_state(state: &crate::Ember) -> Self {
        let title = state
            .focused
            .as_ref()
            .map(|w| crate::wm::metadata(w).1)
            .unwrap_or_default();
        Self {
            size: state.output_size(),
            workspace: state.workspace,
            workspaces: state.config.layout.workspaces,
            occupied: state
                .windows
                .iter()
                .filter(|w| w.scratchpad.is_none())
                .map(|w| w.workspace)
                .collect(),
            title,
            floating: state
                .windows
                .iter()
                .find(|m| Some(&m.window) == state.focused.as_ref())
                .is_some_and(|m| m.floating),
            fullscreen: state.windows.iter().any(|m| {
                m.fullscreen
                    && if m.scratchpad.is_some() {
                        state.scratchpad.visible
                    } else {
                        m.workspace == state.workspace
                    }
            }),
        }
    }
}
#[derive(Clone, PartialEq)]
struct Segment {
    text: String,
    kind: String,
    action: Option<String>,
    active: bool,
}
pub struct Widget {
    pub rect: Rect,
    pub kind: String,
    pub action: Option<String>,
}
pub struct View {
    pub name: String,
    pub rect: Rect,
    pub buffer: MemoryRenderBuffer,
    pub widgets: Vec<Widget>,
    key: Vec<Segment>,
    pub on_click: Option<String>,
}
#[derive(Default)]
pub struct Runtime {
    pub views: Vec<View>,
    font: Option<Arc<Font>>,
    font_key: Option<(String, Option<SystemTime>)>,
    font_deadline: Option<Instant>,
    size: Option<Size<i32, Logical>>,
    settings: Option<Settings>,
    generation: u64,
    next_wake: Option<Instant>,
    next_poll: Option<Instant>,
    clocks: HashMap<String, (u64, String)>,
    rasterizations: u64,
    memory: String,
    cpu: String,
    uptime: String,
    volume: String,
    cpu_last: Option<(u64, u64)>,
    volume_child: Option<(Child, Instant)>,
    last_error: Option<String>,
}
impl Drop for Runtime {
    fn drop(&mut self) {
        self.stop_volume();
    }
}
impl Runtime {
    fn stop_volume(&mut self) {
        if let Some((mut child, _)) = self.volume_child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    fn clear(&mut self) -> bool {
        let changed = !self.views.is_empty();
        if changed || self.font.is_some() {
            crate::memory::request_reclaim();
        }
        self.views = Vec::new();
        self.clocks = HashMap::new();
        self.font = None;
        self.font_key = None;
        self.font_deadline = None;
        self.size = None;
        self.next_wake = None;
        self.next_poll = None;
        self.cpu_last = None;
        self.stop_volume();
        changed
    }
    pub fn disable(&mut self) -> bool {
        let changed = self.clear();
        self.settings = None;
        self.last_error = None;
        changed
    }
    pub fn next_wake(&self) -> Option<Duration> {
        self.next_wake
            .into_iter()
            .chain(self.font_deadline)
            .min()
            .map(|t| t.saturating_duration_since(Instant::now()))
    }
    pub fn contains(&self, pos: Point<f64, Logical>) -> bool {
        self.views.iter().any(|v| v.rect.to_f64().contains(pos))
    }
    pub fn click(&self, pos: Point<f64, Logical>) -> Option<String> {
        let view = self
            .views
            .iter()
            .rev()
            .find(|v| v.rect.to_f64().contains(pos))?;
        view.widgets
            .iter()
            .find(|w| w.rect.to_f64().contains(pos))
            .and_then(|w| w.action.clone())
            .or_else(|| view.on_click.clone())
    }
    pub fn scroll(
        &self,
        pos: Point<f64, Logical>,
        down: bool,
        current: usize,
        count: usize,
    ) -> Option<String> {
        let widget = self
            .views
            .iter()
            .rev()
            .flat_map(|v| v.widgets.iter())
            .find(|w| w.rect.to_f64().contains(pos))?;
        if widget.kind == "volume" {
            Some(if down { "volume-down" } else { "volume-up" }.into())
        } else if widget.kind == "workspaces" {
            Some(format!(
                "workspace {}",
                if down {
                    (current + 1) % count + 1
                } else {
                    (current + count - 1) % count + 1
                }
            ))
        } else {
            None
        }
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"font_loaded":self.font.is_some(),"buffer_bytes":self.views.iter().map(|v|v.rect.size.w as u64*v.rect.size.h as u64*4).sum::<u64>(),
            "polling":self.next_poll.is_some(),"rasterizations":self.rasterizations,"volume_query_running":self.volume_child.is_some(),"error":self.last_error,
            "items":self.views.iter().map(|v|serde_json::json!({"name":v.name,"x":v.rect.loc.x,"y":v.rect.loc.y,"width":v.rect.size.w,"height":v.rect.size.h,
                "on_click":v.on_click,"widgets":v.widgets.iter().zip(v.key.iter()).map(|(w,k)|serde_json::json!({"kind":w.kind,"text":k.text,"active":k.active,"action":w.action,
                    "x":w.rect.loc.x,"y":w.rect.loc.y,"width":w.rect.size.w,"height":w.rect.size.h})).collect::<Vec<_>>()})).collect::<Vec<_>>()})
    }
    pub fn update(
        &mut self,
        settings: &Settings,
        path: &Path,
        generation: u64,
        input: Input,
    ) -> bool {
        if settings.active().next().is_none() {
            let changed = self.clear();
            self.settings = Some(settings.clone());
            self.last_error = None;
            return changed;
        }
        if settings.hide_on_fullscreen && input.fullscreen {
            return self.clear();
        }
        let changed_settings =
            self.settings.as_ref() != Some(settings) || self.generation != generation;
        let resized = self.size != Some(input.size);
        self.size = Some(input.size);
        if changed_settings {
            let key = (
                settings.font.clone(),
                font_path(settings, path)
                    .and_then(|p| std::fs::metadata(p).ok())
                    .and_then(|m| m.modified().ok()),
            );
            if self.font_key.as_ref() != Some(&key) {
                self.font = None;
                self.font_deadline = None;
            }
            self.font_key = Some(key);
            self.generation = generation;
            self.settings = Some(settings.clone());
            self.next_poll = None;
            self.clocks.clear();
        }
        let now = Instant::now();
        let modules: Vec<_> = settings
            .active()
            .flat_map(|p| p.modules.iter().map(String::as_str))
            .collect();
        let polls = modules
            .iter()
            .any(|m| ["cpu", "memory", "uptime", "volume"].contains(m));
        if polls && self.next_poll.is_none_or(|t| now >= t) {
            self.sample(&modules);
            self.next_poll = Some(now + Duration::from_secs(settings.poll_interval_secs));
        }
        if !polls {
            self.next_poll = None;
            self.cpu_last = None;
            self.stop_volume();
        }
        if !modules.contains(&"volume") {
            self.stop_volume();
        }
        let _ = self.read_volume();
        let secs = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        let period = settings
            .active()
            .flat_map(|p| {
                p.modules.iter().filter_map(|m| match m.as_str() {
                    "clock" => Some(clock_period(&p.clock_format)),
                    "date" => Some(clock_period(&p.date_format)),
                    _ => None,
                })
            })
            .min();
        for item in settings.active() {
            for module in &item.modules {
                let format = match module.as_str() {
                    "clock" => &item.clock_format,
                    "date" => &item.date_format,
                    _ => continue,
                };
                let bucket = secs / clock_period(format);
                if self
                    .clocks
                    .get(format)
                    .is_none_or(|(old, _)| *old != bucket)
                {
                    self.clocks
                        .insert(format.clone(), (bucket, clock_text(format)));
                }
            }
        }
        self.next_wake = period.map(|p| now + Duration::from_secs(p - secs % p));
        if let Some(poll) = self.next_poll {
            self.next_wake = Some(self.next_wake.map(|t| t.min(poll)).unwrap_or(poll));
        }
        if self.volume_child.is_some() {
            self.next_wake = Some(
                self.next_wake
                    .map(|t| t.min(now + Duration::from_millis(100)))
                    .unwrap_or(now + Duration::from_millis(100)),
            );
        }
        let mut old = std::mem::take(&mut self.views);
        let mut views = Vec::new();
        let mut changed = changed_settings;
        for p in settings.active() {
            let segments = self.segments(settings, p, &input);
            // The bitmap and geometry are sufficient for unchanged content. Do not
            // parse or retain a whole Unicode font just to redisplay cached pixels.
            if !changed_settings && !resized {
                if let Some(i) = old
                    .iter()
                    .position(|v| v.name == p.name && v.key == segments)
                {
                    views.push(old.swap_remove(i));
                    continue;
                }
            }
            if self.font.is_none() {
                match load_font(settings, path) {
                    Ok(font) => {
                        self.font = Some(font);
                        self.last_error = None;
                    }
                    Err(error) => {
                        self.last_error = Some(error);
                        return self.clear();
                    }
                }
            }
            let font = self.font.as_ref().unwrap();
            self.font_deadline = Some(now + Duration::from_secs(2));
            let font_size = p.font_size.unwrap_or(settings.font_size);
            let height = p.height.unwrap_or(settings.height).min(input.size.h.max(1));
            let widths: Vec<i32> = segments
                .iter()
                .map(|s| {
                    text_width(font, &s.text, font_size)
                        + if s.kind == "workspaces" {
                            2 * settings.workspace_padding
                        } else {
                            0
                        }
                })
                .collect();
            let content = widths.iter().sum::<i32>()
                + settings.spacing * (segments.len().saturating_sub(1)) as i32
                + settings.padding * 2;
            let margin = p.margin.unwrap_or(settings.margin);
            let width = (match p.width {
                -1 => input.size.w - 2 * margin,
                0 => content,
                _ => p.width,
            })
            .clamp(1, input.size.w.max(1));
            let rect = position(p, input.size, width, height, margin);
            if let Some(i) = old.iter().position(|v| {
                v.name == p.name && v.rect == rect && v.key == segments && !changed_settings
            }) {
                views.push(old.swap_remove(i));
                continue;
            }
            let mut pixels = vec![0u8; width as usize * height as usize * 4];
            let bg = color(p.background.as_ref().unwrap_or(&settings.background)).unwrap();
            for px in pixels.as_chunks_mut::<4>().0 {
                for c in 0..3 {
                    px[c] = (bg[c] * settings.opacity * 255.0).round() as u8;
                }
                px[3] = (settings.opacity * 255.0).round() as u8;
            }
            let fg = color(p.foreground.as_ref().unwrap_or(&settings.foreground)).unwrap();
            let accent = color(p.accent.as_ref().unwrap_or(&settings.accent)).unwrap();
            let b = settings.border_width;
            let border = color(&settings.border_color).unwrap();
            for (x, y, w, h) in [
                (0, 0, width, b),
                (0, height - b, width, b),
                (0, 0, b, height),
                (width - b, 0, b, height),
            ] {
                fill(&mut pixels, (width, height), (x, y, w, h), border);
            }
            let mut x = settings.padding;
            let mut widgets = Vec::new();
            for (segment, w) in segments.iter().zip(widths) {
                let extra = if segment.kind == "workspaces" {
                    settings.workspace_padding
                } else {
                    0
                };
                draw_text(
                    &mut pixels,
                    (width, height),
                    font,
                    &segment.text,
                    font_size,
                    x + extra,
                    if segment.active { accent } else { fg },
                );
                if segment.active {
                    fill(&mut pixels, (width, height), (x, height - 2, w, 2), accent);
                }
                let start = x.clamp(0, width);
                let end = (x + w).clamp(0, width);
                widgets.push(Widget {
                    rect: Rectangle::new(
                        (rect.loc.x + start, rect.loc.y).into(),
                        ((end - start).max(0), height).into(),
                    ),
                    kind: segment.kind.clone(),
                    action: segment.action.clone(),
                });
                x += w + settings.spacing;
            }
            self.rasterizations += 1;
            let buffer = MemoryRenderBuffer::from_slice(
                &pixels,
                Fourcc::Abgr8888,
                (width, height),
                1,
                Transform::Normal,
                None,
            );
            views.push(View {
                name: p.name.clone(),
                rect,
                buffer,
                widgets,
                key: segments,
                on_click: p.on_click.clone(),
            });
            changed = true;
        }
        if !old.is_empty() {
            changed = true;
        }
        self.views = views;
        if self.font_deadline.is_some_and(|t| now >= t) {
            self.font = None;
            self.font_deadline = None;
            crate::memory::request_reclaim();
        }
        changed
    }
    fn segments(&self, settings: &Settings, p: &Item, input: &Input) -> Vec<Segment> {
        let mut segments = Vec::new();
        for module in &p.modules {
            if module == "workspaces" {
                for n in 0..input.workspaces {
                    if !settings.hide_empty_workspaces
                        || n == input.workspace
                        || input.occupied.contains(&n)
                    {
                        segments.push(Segment {
                            text: (n + 1).to_string(),
                            kind: module.clone(),
                            active: n == input.workspace,
                            action: Some(format!("workspace {}", n + 1)),
                        });
                    }
                }
                continue;
            }
            let text = match module.as_str() {
                "clock" => self
                    .clocks
                    .get(&p.clock_format)
                    .map(|(_, s)| s.clone())
                    .unwrap_or_default(),
                "date" => self
                    .clocks
                    .get(&p.date_format)
                    .map(|(_, s)| s.clone())
                    .unwrap_or_default(),
                "title" => truncate(&input.title, p.max_title_chars),
                "layout" => if input.floating { "FLOAT" } else { "TILE" }.into(),
                "memory" => {
                    if self.memory.is_empty() {
                        "RAM --".into()
                    } else {
                        self.memory.clone()
                    }
                }
                "cpu" => {
                    if self.cpu.is_empty() {
                        "CPU --".into()
                    } else {
                        self.cpu.clone()
                    }
                }
                "uptime" => self.uptime.clone(),
                "volume" => {
                    if self.volume.is_empty() {
                        "VOL --".into()
                    } else {
                        self.volume.clone()
                    }
                }
                _ => module.strip_prefix("text:").unwrap_or_default().into(),
            };
            segments.push(Segment {
                text,
                kind: module.clone(),
                active: false,
                action: if module == "volume" {
                    Some("mute".into())
                } else {
                    None
                },
            });
        }
        segments
    }
    fn sample(&mut self, modules: &[&str]) {
        if modules.contains(&"memory") {
            if let Ok(data) = std::fs::read_to_string("/proc/meminfo") {
                let get = |key: &str| {
                    data.lines()
                        .find_map(|l| {
                            l.strip_prefix(key)
                                .and_then(|v| v.split_whitespace().next())
                                .and_then(|v| v.parse::<u64>().ok())
                        })
                        .unwrap_or(0)
                };
                self.memory = format!(
                    "RAM {}MiB",
                    get("MemTotal:").saturating_sub(get("MemAvailable:")) / 1024
                );
            }
        }
        if modules.contains(&"cpu") {
            if let Ok(data) = std::fs::read_to_string("/proc/stat") {
                let nums: Vec<u64> = data
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .split_whitespace()
                    .skip(1)
                    .take(8)
                    .filter_map(|n| n.parse().ok())
                    .collect();
                if nums.len() >= 5 {
                    let total = nums.iter().sum::<u64>();
                    let idle = nums[3] + nums[4];
                    self.cpu = if let Some((old_total, old_idle)) = self.cpu_last {
                        let delta = total.saturating_sub(old_total);
                        format!(
                            "CPU {:.0}%",
                            100.0 * (delta.saturating_sub(idle.saturating_sub(old_idle))) as f64
                                / delta.max(1) as f64
                        )
                    } else {
                        "CPU --".into()
                    };
                    self.cpu_last = Some((total, idle));
                }
            }
        }
        if modules.contains(&"uptime") {
            if let Ok(data) = std::fs::read_to_string("/proc/uptime") {
                let secs = data
                    .split_whitespace()
                    .next()
                    .and_then(|s| s.parse::<f64>().ok())
                    .unwrap_or(0.0) as u64;
                self.uptime = format!("UP {}h{:02}m", secs / 3600, secs % 3600 / 60);
            }
        }
        if modules.contains(&"volume") && self.volume_child.is_none() {
            if let Ok(child) = Command::new("wpctl")
                .args(["get-volume", "@DEFAULT_AUDIO_SINK@"])
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            {
                self.volume_child = Some((child, Instant::now()));
            }
        }
    }
    fn read_volume(&mut self) -> bool {
        let Some((child, start)) = &mut self.volume_child else {
            return false;
        };
        match child.try_wait() {
            Ok(Some(status)) => {
                let (mut child, _) = self.volume_child.take().unwrap();
                let mut output = String::new();
                if let Some(stdout) = child.stdout.take() {
                    let _ = stdout.take(1024).read_to_string(&mut output);
                }
                let _ = child.wait();
                let text = if status.success() {
                    let value = output
                        .split_whitespace()
                        .nth(1)
                        .and_then(|s| s.parse::<f64>().ok())
                        .filter(|v| v.is_finite())
                        .unwrap_or(0.0);
                    if output.contains("MUTED") {
                        "VOL MUTE".into()
                    } else {
                        format!("VOL {:.0}%", value * 100.0)
                    }
                } else {
                    "VOL --".into()
                };
                let changed = self.volume != text;
                self.volume = text;
                changed
            }
            _ if start.elapsed() > Duration::from_secs(2) => {
                self.stop_volume();
                false
            }
            _ => false,
        }
    }
}
fn position(p: &Item, size: Size<i32, Logical>, width: i32, height: i32, margin: i32) -> Rect {
    let x = if p.anchor.ends_with("right") {
        size.w - margin - width
    } else if p.anchor.ends_with("center") {
        (size.w - width) / 2
    } else {
        margin
    };
    let y = if p.anchor.starts_with("bottom") {
        size.h - margin - height
    } else {
        margin
    };
    Rectangle::new(
        (
            (x + p.offset_x).clamp(0, (size.w - width).max(0)),
            (y + p.offset_y).clamp(0, (size.h - height).max(0)),
        )
            .into(),
        (width, height).into(),
    )
}
pub(crate) fn truncate(text: &str, max: usize) -> String {
    let chars: Vec<_> = text
        .chars()
        .filter(|c| !c.is_control())
        .take(max + 1)
        .collect();
    if chars.len() <= max {
        chars.into_iter().collect()
    } else {
        chars[..max.saturating_sub(1)]
            .iter()
            .chain(std::iter::once(&'…'))
            .collect()
    }
}
pub(crate) fn text_width(font: &Font, text: &str, px: f32) -> i32 {
    text.chars()
        .map(|c| font.metrics(c, px).advance_width)
        .sum::<f32>()
        .ceil() as i32
}
pub(crate) fn draw_text(
    pixels: &mut [u8],
    size: (i32, i32),
    font: &Font,
    text: &str,
    px: f32,
    x: i32,
    rgba: [f32; 4],
) {
    let line = font.horizontal_line_metrics(px).unwrap();
    let baseline =
        ((size.1 as f32 - (line.ascent - line.descent)) / 2.0 + line.ascent).round() as i32;
    let mut cursor = x as f32;
    for c in text.chars() {
        let (metrics, bitmap) = font.rasterize(c, px);
        for y in 0..metrics.height {
            for x in 0..metrics.width {
                let dx = cursor.round() as i32 + metrics.xmin + x as i32;
                let dy = baseline - metrics.ymin - metrics.height as i32 + y as i32;
                if dx >= 0 && dx < size.0 && dy >= 0 && dy < size.1 {
                    let a = f32::from(bitmap[y * metrics.width + x]) / 255.0;
                    let i = ((dy * size.0 + dx) * 4) as usize;
                    for channel in 0..3 {
                        pixels[i + channel] = (rgba[channel] * a * 255.0
                            + f32::from(pixels[i + channel]) * (1.0 - a))
                            .round()
                            .clamp(0.0, 255.0) as u8;
                    }
                    pixels[i + 3] = (a * 255.0 + f32::from(pixels[i + 3]) * (1.0 - a))
                        .round()
                        .clamp(0.0, 255.0) as u8;
                }
            }
        }
        cursor += metrics.advance_width;
        if cursor >= size.0 as f32 {
            break;
        }
    }
}
pub(crate) fn fill(
    pixels: &mut [u8],
    size: (i32, i32),
    rect: (i32, i32, i32, i32),
    color: [f32; 4],
) {
    for y in rect.1.max(0)..(rect.1 + rect.3).min(size.1) {
        for x in rect.0.max(0)..(rect.0 + rect.2).min(size.0) {
            let i = ((y * size.0 + x) * 4) as usize;
            for c in 0..4 {
                pixels[i + c] = (color[c] * 255.0).round() as u8;
            }
        }
    }
}
fn clock_period(format: &str) -> u64 {
    if ["%S", "%T", "%X", "%c", "%s", "%r"]
        .iter()
        .any(|s| format.contains(s))
    {
        1
    } else {
        60
    }
}
fn clock_text(format: &str) -> String {
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .min(i64::MAX as u64) as libc::time_t;
    let format = CString::new(format).unwrap_or_default();
    let mut tm = std::mem::MaybeUninit::<libc::tm>::uninit();
    let mut bytes = [0u8; 512];
    // SAFETY: localtime_r writes to a valid tm; strftime is bounded by this buffer.
    unsafe {
        if libc::localtime_r(&time, tm.as_mut_ptr()).is_null() {
            return String::new();
        }
        let n = libc::strftime(
            bytes.as_mut_ptr().cast(),
            bytes.len(),
            format.as_ptr(),
            tm.as_ptr(),
        );
        String::from_utf8_lossy(&bytes[..n])
            .chars()
            .filter(|c| !c.is_control())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn settings() -> Settings {
        Settings {
            enabled: true,
            items: vec![Item {
                name: "main".into(),
                modules: vec!["workspaces".into(), "text:ember".into()],
                ..Default::default()
            }],
            ..Default::default()
        }
    }
    fn input() -> Input {
        Input {
            size: (1280, 800).into(),
            workspace: 0,
            workspaces: 9,
            occupied: vec![0, 2],
            title: "terminal".into(),
            floating: false,
            fullscreen: false,
        }
    }
    #[test]
    fn multiple_panels_reserve_edges_and_anchor_independently() {
        let mut s = settings();
        s.items.push(Item {
            name: "bottom".into(),
            anchor: "bottom-right".into(),
            modules: vec!["text:bottom".into()],
            height: Some(30),
            ..Default::default()
        });
        assert_eq!(
            work_area(&s, input().size),
            Rectangle::new((0, 30).into(), (1280, 732).into())
        );
        let mut r = Runtime::default();
        assert!(r.update(&s, Path::new("/tmp/config.toml"), 0, input()));
        assert_eq!(r.views[0].rect.loc, (8, 8).into());
        assert_eq!(r.views[1].rect.loc.y, 762);
        assert_eq!(r.views[1].rect.loc.x + r.views[1].rect.size.w, 1272);
        s.enabled = false;
        assert_eq!(
            work_area(&s, input().size),
            Rectangle::new((0, 0).into(), input().size)
        );
    }
    #[test]
    fn static_panels_reuse_buffers_and_disabled_panels_release_resources() {
        let mut r = Runtime::default();
        let mut s = settings();
        assert!(r.update(&s, Path::new("/tmp/config.toml"), 0, input()));
        let count = r.rasterizations;
        for _ in 0..100 {
            assert!(!r.update(&s, Path::new("/tmp/config.toml"), 0, input()));
        }
        assert_eq!(count, r.rasterizations);
        assert!(r.font.is_some());
        assert!(r.next_wake().is_some());
        r.font_deadline = Some(Instant::now());
        assert!(!r.update(&s, Path::new("/tmp/config.toml"), 0, input()));
        assert!(r.font.is_none() && r.next_wake().is_none());
        assert_eq!(count, r.rasterizations);
        for _ in 0..100 {
            assert!(!r.update(&s, Path::new("/tmp/config.toml"), 0, input()));
        }
        assert!(r.font.is_none());
        s.items.clear();
        assert!(r.update(&s, Path::new("/tmp/config.toml"), 1, input()));
        assert!(r.views.is_empty() && r.font.is_none() && r.next_wake().is_none());
        assert_eq!(r.status()["buffer_bytes"], 0);
        assert!(!r.disable());
    }
    #[test]
    fn workspace_hitboxes_and_fullscreen_hiding_follow_state() {
        let s = settings();
        let mut r = Runtime::default();
        r.update(&s, Path::new("/tmp/config.toml"), 0, input());
        let widget = &r.views[0].widgets[1]; // occupied workspace 3
        let pos = widget.rect.loc.to_f64() + Point::from((2., 2.));
        assert_eq!(r.click(pos).as_deref(), Some("workspace 3"));
        assert_eq!(r.scroll(pos, true, 0, 9).as_deref(), Some("workspace 2"));
        assert_eq!(r.scroll(pos, false, 0, 9).as_deref(), Some("workspace 9"));
        let mut i = input();
        i.fullscreen = true;
        assert!(r.update(&s, Path::new("/tmp/config.toml"), 0, i));
        assert!(
            !r.contains(pos) && r.views.is_empty() && r.next_wake().is_none() && r.font.is_none()
        );
        assert!(r.update(&s, Path::new("/tmp/config.toml"), 0, input()));
        assert_eq!(r.views.len(), 1);
    }
    #[test]
    fn validates_bad_modules_fonts_and_extreme_offsets_without_panicking() {
        let mut s = settings();
        assert!(s.validate().is_ok());
        s.items[0].offset_x = i32::MIN;
        assert!(s.validate().is_err());
        s.items[0].offset_x = 0;
        s.items[0].modules.push("unknown".into());
        assert!(s.validate().is_err());
        s.items[0].modules.pop();
        s.items.push(s.items[0].clone());
        assert!(s.validate().is_err());
        s.items.pop();
        s.font = "/nonexistent/ember-font.ttf".into();
        assert!(validate_resources(&s, Path::new("/tmp/config.toml")).is_err());
        s.enabled = false;
        assert!(validate_resources(&s, Path::new("/tmp/config.toml")).is_ok());
        assert_eq!(truncate("é世界\nabc", 4), "é世界…");
        assert_eq!(clock_period("%H:%M:%S"), 1);
        assert_eq!(clock_period("%H:%M"), 60);
    }
    #[test]
    fn disabling_reaps_only_the_owned_pending_volume_query() {
        let child = Command::new("sleep").arg("30").spawn().unwrap();
        let pid = child.id();
        let mut unrelated = Command::new("sleep").arg("30").spawn().unwrap();
        let mut r = Runtime::default();
        r.volume_child = Some((child, Instant::now()));
        r.disable();
        assert!(!Path::new(&format!("/proc/{pid}")).exists());
        assert!(unrelated.try_wait().unwrap().is_none());
        unrelated.kill().unwrap();
        unrelated.wait().unwrap();
    }
}
