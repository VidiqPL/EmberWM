//! Cached native notification cards and a shortcut sheet derived from live bindings.
use crate::{
    config::{color, Config},
    layout::Rect,
    notifications, panels,
};
use fontdue::Font;
use serde::{Deserialize, Serialize};
use smithay::{
    backend::{allocator::Fourcc, renderer::element::memory::MemoryRenderBuffer},
    utils::{Logical, Point, Rectangle, Size, Transform},
};
use std::{path::Path, sync::Arc};
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ShortcutSettings {
    pub enabled: bool,
    pub width: i32,
    pub font_size: f32,
}
impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            width: 860,
            font_size: 12.0,
        }
    }
}
impl ShortcutSettings {
    pub fn validate(&self) -> Result<(), String> {
        if !(320..=1600).contains(&self.width)
            || !self.font_size.is_finite()
            || !(9.0..=24.0).contains(&self.font_size)
        {
            Err("shortcut width/font size out of range".into())
        } else {
            Ok(())
        }
    }
}
pub fn validate_resources(config: &Config, path: &Path) -> Result<(), String> {
    if (config.shortcuts.enabled
        || config.notifications.enabled
        || config.wifi.enabled
        || config.launcher.enabled
        || config.tray.enabled
        || config.audio.osd_enabled)
        && config.panels.font != "builtin"
    {
        panels::load_font(&config.panels, path)?;
    }
    Ok(())
}
#[derive(Clone, Serialize)]
pub struct ShortcutRow {
    pub keys: String,
    pub description: String,
    pub action: String,
}
pub fn shortcut_rows(config: &Config, native: bool) -> Vec<ShortcutRow> {
    let modifier = &config.keyboard.modifier;
    let mut rows: Vec<_> = config
        .bindings
        .iter()
        .map(|b| {
            let key = match b.key.as_str() {
                "Return" => "Enter",
                "slash" => "/",
                "period" => ".",
                "comma" => ",",
                "space" => "Space",
                "Escape" => "Esc",
                k => k,
            };
            let mut keys = String::new();
            if b.modifier {
                keys.push_str(modifier);
                keys.push('+');
            }
            if b.shift {
                keys.push_str("Shift+");
            }
            keys.push_str(key);
            ShortcutRow {
                keys,
                description: b.description.clone().unwrap_or_else(|| describe(&b.action)),
                action: b.action.clone(),
            }
        })
        .collect();
    for (keys, description) in [
        (format!("{modifier}+Left drag"), "Move / float window"),
        (format!("{modifier}+Right drag"), "Resize window"),
        ("Esc / Right click".into(), "Cancel drawing"),
        ("Esc".into(), "Close shortcut sheet"),
    ] {
        rows.push(ShortcutRow {
            keys,
            description: description.into(),
            action: String::new(),
        });
    }
    if native {
        rows.push(ShortcutRow {
            keys: "Ctrl+Alt+F1…F12".into(),
            description: "Switch virtual terminal".into(),
            action: String::new(),
        });
    }
    rows
}
fn describe(action: &str) -> String {
    let text = match action {
        "terminal" => "Open terminal",
        "draw" => "Draw a terminal rectangle",
        "scratchpad-mark" => "Add / remove window from scratchpad",
        "scratchpad-toggle" => "Show / hide scratchpad",
        "screenshot-region" => "Select screenshot to clipboard",
        "quit" => "Log out",
        "close" => "Close focused window",
        "float" => "Toggle floating / tiling",
        "fullscreen" => "Toggle fullscreen",
        "focus-next" => "Focus next window",
        "focus-prev" => "Focus previous window",
        "ratio-down" => "Shrink master area",
        "ratio-up" => "Grow master area",
        "reload" => "Reload config",
        "rules-apply" => "Reapply window rules",
        "panels-toggle" => "Toggle panels",
        "shortcuts-toggle" => "Toggle shortcut sheet",
        "wifi-toggle" => "Toggle Wi-Fi controls",
        "tray-toggle" => "Tray apps and context menus",
        "pointer-release" => "Release game mouse capture",
        "rice-cycle" => "Cycle rice profile",
        "theme-cycle" => "Cycle theme preset",
        "launcher-toggle" => "Search and launch applications",
        "volume-up" => "Volume up",
        "volume-down" => "Volume down",
        "mute" => "Mute output",
        "mic-mute" => "Mute microphone",
        "notification-dismiss" => "Dismiss latest notification",
        "notifications-clear" => "Clear notifications",
        "notifications-toggle-dnd" => "Toggle Do Not Disturb",
        _ => "",
    };
    if !text.is_empty() {
        text.into()
    } else if let Some(name) = action.strip_prefix("rice ") {
        format!("Load rice profile {name}")
    } else if let Some(n) = action.strip_prefix("workspace ") {
        format!("Switch to workspace {n}")
    } else if let Some(n) = action.strip_prefix("send ") {
        format!("Send window to workspace {n}")
    } else if let Some(cmd) = action.strip_prefix("exec ") {
        format!("Run {cmd}")
    } else {
        action.into()
    }
}
pub enum Interaction {
    CloseHelp,
    Dismiss(u32),
    Invoke(u32, String),
}
struct Button {
    rect: Rect,
    key: String,
}
pub struct View {
    pub rect: Rect,
    pub buffer: MemoryRenderBuffer,
    id: u32,
    buttons: Vec<Button>,
    default_action: Option<String>,
}
impl View {
    pub(crate) fn overlay(rect: Rect, buffer: MemoryRenderBuffer) -> Self {
        Self {
            rect,
            buffer,
            id: 0,
            buttons: Vec::new(),
            default_action: None,
        }
    }
}
#[derive(Default)]
pub struct Runtime {
    pub help_open: bool,
    pub help_page: usize,
    pub help: Option<View>,
    pub cards: Vec<View>,
    font: Option<Arc<Font>>,
    key: Option<(bool, usize, u64, u64, i32, i32)>,
    rows: Vec<ShortcutRow>,
    pages: usize,
    pub error: Option<String>,
    rasterizations: u64,
}
impl Runtime {
    pub fn toggle(&mut self) {
        self.help_open = !self.help_open;
        self.help_page = 0;
    }
    pub fn close(&mut self) {
        self.help_open = false;
    }
    pub fn scroll(&mut self, down: bool) {
        if self.help_open {
            self.help_page = if down {
                (self.help_page + 1).min(self.pages.saturating_sub(1))
            } else {
                self.help_page.saturating_sub(1)
            };
        }
    }
    pub fn contains(&self, pos: Point<f64, Logical>) -> bool {
        self.help_open || self.cards.iter().any(|v| v.rect.to_f64().contains(pos))
    }
    pub fn click(&self, pos: Point<f64, Logical>, right: bool) -> Option<Interaction> {
        if self.help_open {
            return if right
                || self.help.as_ref().is_none_or(|v| {
                    !v.rect.to_f64().contains(pos)
                        || pos.y < v.rect.loc.y as f64 + 34.0
                            && pos.x > (v.rect.loc.x + v.rect.size.w - 36) as f64
                }) {
                Some(Interaction::CloseHelp)
            } else {
                None
            };
        }
        let view = self.cards.iter().find(|v| v.rect.to_f64().contains(pos))?;
        if right
            || pos.x >= (view.rect.loc.x + view.rect.size.w - 28) as f64
                && pos.y < (view.rect.loc.y + 30) as f64
        {
            return Some(Interaction::Dismiss(view.id));
        }
        if let Some(b) = view.buttons.iter().find(|b| b.rect.to_f64().contains(pos)) {
            return Some(Interaction::Invoke(view.id, b.key.clone()));
        }
        if let Some(key) = &view.default_action {
            Some(Interaction::Invoke(view.id, key.clone()))
        } else {
            Some(Interaction::Dismiss(view.id))
        }
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"shortcuts_open":self.help_open,"page":self.help_page+1,"pages":self.pages,"rows":self.rows,"font_loaded":self.font.is_some(),"error":self.error,"rasterizations":self.rasterizations,"buffer_bytes":self.help.iter().chain(self.cards.iter()).map(|v|v.rect.size.w as u64*v.rect.size.h as u64*4).sum::<u64>(),"cards":self.cards.iter().map(|v|serde_json::json!({"id":v.id,"x":v.rect.loc.x,"y":v.rect.loc.y,"width":v.rect.size.w,"height":v.rect.size.h,"buttons":v.buttons.iter().map(|b|serde_json::json!({"key":b.key,"x":b.rect.loc.x,"y":b.rect.loc.y,"width":b.rect.size.w,"height":b.rect.size.h})).collect::<Vec<_>>()})).collect::<Vec<_>>(),"help":self.help.as_ref().map(|v|serde_json::json!({"x":v.rect.loc.x,"y":v.rect.loc.y,"width":v.rect.size.w,"height":v.rect.size.h}))})
    }
    pub fn update(
        &mut self,
        config: &Config,
        path: &Path,
        generation: u64,
        native: bool,
        size: Size<i32, Logical>,
        notices: &notifications::Runtime,
    ) -> bool {
        if !config.shortcuts.enabled {
            self.help_open = false;
        }
        let key = (
            self.help_open,
            self.help_page,
            generation,
            notices.revision,
            size.w,
            size.h,
        );
        if self.key == Some(key) {
            return false;
        }
        self.key = Some(key);
        if self.help.is_some() && !self.help_open
            || !self.cards.is_empty() && notices.entries.is_empty()
        {
            crate::memory::request_reclaim();
        }
        let changed = self.help.is_some()
            || !self.cards.is_empty()
            || self.help_open
            || !notices.entries.is_empty();
        self.help = None;
        self.cards = Vec::new();
        if !self.help_open && notices.entries.is_empty() {
            self.font = None;
            self.rows = Vec::new();
            self.pages = 0;
            self.error = None;
            return changed;
        }
        // The weak font cache shares geometry with panels and never keeps a font alive itself.
        match panels::load_font(&config.panels, path) {
            Ok(font) => {
                self.font = Some(font);
                self.error = None;
            }
            Err(e) => {
                self.font = None;
                self.error = Some(e);
                return changed;
            }
        }
        let font = self.font.as_ref().unwrap();
        let fg = color(&config.panels.foreground).unwrap();
        let bg = color(&config.panels.background).unwrap();
        let accent = color(&config.panels.accent).unwrap();
        let px = config.panels.font_size.max(11.0);
        let line = px.ceil() as i32 + 5;
        let ns = &config.notifications;
        let width = ns.width.min((size.w - 2 * ns.margin).max(1));
        let mut y = ns.margin
            + if ns.respect_panel_space {
                panels::work_area(&config.panels, size).loc.y
            } else {
                0
            };
        for notice in notices.entries.iter().rev().take(ns.max_visible) {
            let summary = wrap(
                font,
                &notice.summary,
                px + 1.0,
                if ns.compact { width - 36 } else { width - 28 },
                if ns.compact { 1 } else { 2 },
            );
            let body = wrap(
                font,
                &notice.body,
                px,
                width - 28,
                if ns.compact { 2 } else { 3 },
            );
            let buttons: Vec<_> = notice
                .actions
                .iter()
                .filter(|(k, _)| k != "default")
                .take(4)
                .collect();
            let action_rows = buttons.len().div_ceil(2) as i32;
            let height = (if ns.compact { 10 } else { 32 })
                + summary.len() as i32 * (line + 1)
                + body.len() as i32 * line
                + action_rows * (line + 8)
                + 12;
            if y + height > size.h - ns.margin {
                break;
            }
            let rect = Rectangle::new(
                (size.w - ns.margin - width, y).into(),
                (width, height).into(),
            );
            let mut hits = Vec::new();
            let buffer = surface(width, height, |pixels| {
                panels::fill(pixels, (width, height), (0, 0, width, height), bg);
                let edge = if notice.urgency == 2 {
                    [1.0, 0.35, 0.4, 1.0]
                } else {
                    color(&config.panels.border_color).unwrap()
                };
                if ns.compact {
                    for rect in [
                        (0, 0, width, 1),
                        (0, height - 1, width, 1),
                        (0, 0, 1, height),
                        (width - 1, 0, 1, height),
                    ] {
                        panels::fill(pixels, (width, height), rect, edge);
                    }
                } else {
                    panels::fill(
                        pixels,
                        (width, height),
                        (0, 0, 3, height),
                        if notice.urgency == 2 { edge } else { accent },
                    );
                }
                if !ns.compact {
                    text(
                        pixels,
                        (width, height),
                        font,
                        &fit(
                            font,
                            if notice.app.is_empty() {
                                "Notification"
                            } else {
                                &notice.app
                            },
                            px,
                            width - 55,
                        ),
                        px,
                        12,
                        7,
                        accent,
                    );
                }
                text(
                    pixels,
                    (width, height),
                    font,
                    "×",
                    px + 2.0,
                    width - 24,
                    6,
                    fg,
                );
                let mut row_y = if ns.compact { 8 } else { 30 };
                for s in &summary {
                    text(pixels, (width, height), font, s, px + 1.0, 12, row_y, fg);
                    row_y += line + 1;
                }
                for s in &body {
                    text(pixels, (width, height), font, s, px, 12, row_y, fg);
                    row_y += line;
                }
                for (i, (k, label)) in buttons.iter().enumerate() {
                    let bw = (width - 36) / 2;
                    let bx = 12 + (i % 2) as i32 * (bw + 8);
                    let by = row_y + (i / 2) as i32 * (line + 8) + 5;
                    panels::fill(
                        pixels,
                        (width, height),
                        (bx, by, bw, line + 4),
                        color(&config.panels.border_color).unwrap(),
                    );
                    text(
                        pixels,
                        (width, height),
                        font,
                        &fit(font, label, px, bw - 8),
                        px,
                        bx + 4,
                        by + 2,
                        fg,
                    );
                    hits.push(Button {
                        rect: Rectangle::new(
                            (rect.loc.x + bx, rect.loc.y + by).into(),
                            (bw, line + 4).into(),
                        ),
                        key: k.clone(),
                    });
                }
            });
            self.rasterizations += 1;
            self.cards.push(View {
                rect,
                buffer,
                id: notice.id,
                buttons: hits,
                default_action: notice
                    .actions
                    .iter()
                    .find(|(k, _)| k == "default")
                    .map(|(k, _)| k.clone()),
            });
            y += height + ns.gap;
        }
        if self.help_open {
            self.rows = shortcut_rows(config, native);
            let px = config.shortcuts.font_size;
            let line = px.ceil() as i32 + 9;
            let width = config.shortcuts.width.min((size.w - 32).max(1));
            let columns = if width >= 700 { 2 } else { 1 };
            let per_column = ((size.h - 150) / line).max(1) as usize;
            let capacity = per_column * columns;
            self.pages = self.rows.len().div_ceil(capacity).max(1);
            self.help_page = self.help_page.min(self.pages - 1);
            let start = self.help_page * capacity;
            let visible = &self.rows[start..(start + capacity).min(self.rows.len())];
            let rows = visible.len().div_ceil(columns);
            let height = (rows as i32 * line + 100).min(size.h.max(1));
            let rect = Rectangle::new(
                ((size.w - width) / 2, (size.h - height) / 2).into(),
                (width, height).into(),
            );
            let buffer = surface(width, height, |pixels| {
                panels::fill(pixels, (width, height), (0, 0, width, height), bg);
                for edge in [
                    (0, 0, width, 1),
                    (0, height - 1, width, 1),
                    (0, 0, 1, height),
                    (width - 1, 0, 1, height),
                ] {
                    panels::fill(pixels, (width, height), edge, accent);
                }
                text(
                    pixels,
                    (width, height),
                    font,
                    "Ember shortcuts",
                    px + 2.0,
                    18,
                    14,
                    accent,
                );
                text(
                    pixels,
                    (width, height),
                    font,
                    "×",
                    px + 3.0,
                    width - 28,
                    12,
                    fg,
                );
                let col_width = (width - 36) / columns as i32;
                for (i, row) in visible.iter().enumerate() {
                    let col = i / rows.max(1);
                    let r = i % rows.max(1);
                    let x = 18 + col as i32 * col_width;
                    let y = 52 + r as i32 * line;
                    let key_width = (col_width / 2).min(180);
                    text(
                        pixels,
                        (width, height),
                        font,
                        &fit(font, &row.keys, px, key_width - 10),
                        px,
                        x,
                        y,
                        accent,
                    );
                    text(
                        pixels,
                        (width, height),
                        font,
                        &fit(font, &row.description, px, col_width - key_width - 10),
                        px,
                        x + key_width,
                        y,
                        fg,
                    );
                }
                text(
                    pixels,
                    (width, height),
                    font,
                    &format!(
                        "Live config · page {}/{} · scroll pages · Esc closes",
                        self.help_page + 1,
                        self.pages
                    ),
                    px.min(11.0),
                    18,
                    height - 29,
                    fg,
                );
            });
            self.rasterizations += 1;
            self.help = Some(View {
                rect,
                buffer,
                id: 0,
                buttons: Vec::new(),
                default_action: None,
            });
        } else {
            self.rows = Vec::new();
            self.pages = 0;
        }
        changed
    }
}
pub(crate) fn surface(width: i32, height: i32, draw: impl FnOnce(&mut [u8])) -> MemoryRenderBuffer {
    let mut buffer = MemoryRenderBuffer::new(
        Fourcc::Abgr8888,
        (width, height),
        1,
        Transform::Normal,
        None,
    );
    buffer
        .render()
        .draw(|pixels| {
            draw(pixels);
            Ok::<_, ()>(vec![Rectangle::from_size((width, height).into())])
        })
        .unwrap();
    buffer
}
pub(crate) fn fit(font: &Font, text: &str, px: f32, width: i32) -> String {
    let mut out = String::new();
    let mut used = 0.;
    let ellipsis = font.metrics('…', px).advance_width;
    for c in text.chars().filter(|c| !c.is_control()) {
        let advance = font.metrics(c, px).advance_width;
        if used + advance > width as f32 {
            while used + ellipsis > width as f32 {
                let Some(last) = out.pop() else {
                    break;
                };
                used -= font.metrics(last, px).advance_width;
            }
            out.push('…');
            break;
        }
        out.push(c);
        used += advance;
    }
    out
}
fn wrap(font: &Font, text: &str, px: f32, width: i32, max_lines: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut used = 0.;
    let mut chars = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .peekable();
    while let Some(c) = chars.next() {
        let advance = font.metrics(c, px).advance_width;
        if c == '\n' || used + advance > width.max(1) as f32 {
            lines.push(std::mem::take(&mut line));
            used = 0.;
            if lines.len() >= max_lines {
                if let Some(last) = lines.last_mut() {
                    *last = fit(font, &format!("{last}…"), px, width);
                }
                return lines;
            }
            if c == '\n' {
                continue;
            }
        }
        line.push(c);
        used += advance;
        if chars.peek().is_none() {
            lines.push(line);
            break;
        }
    }
    lines
}
#[allow(clippy::too_many_arguments)] // Explicit raster destination, position and text style.
pub(crate) fn text(
    pixels: &mut [u8],
    size: (i32, i32),
    font: &Font,
    text: &str,
    px: f32,
    x: i32,
    y: i32,
    fg: [f32; 4],
) {
    let h = (px.ceil() as i32 + 5).min((size.1 - y).max(0));
    if y < 0 || h == 0 {
        return;
    }
    let begin = (y * size.0 * 4) as usize;
    let end = begin + (h * size.0 * 4) as usize;
    panels::draw_text(&mut pixels[begin..end], (size.0, h), font, text, px, x, fg);
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shortcut_rows_follow_real_config_including_custom_descriptions() {
        let mut config = Config::default();
        config.keyboard.modifier = "Super".into();
        let rows = shortcut_rows(&config, true);
        assert!(rows
            .iter()
            .any(|r| r.keys == "Super+/" && r.action == "shortcuts-toggle"));
        config.bindings[0].key = "t".into();
        config.bindings[0].description = Some("My terminal".into());
        let rows = shortcut_rows(&config, false);
        assert_eq!(rows[0].keys, "Super+t");
        assert_eq!(rows[0].description, "My terminal");
        assert!(!rows.iter().any(|r| r.keys == "Ctrl+Alt+F1…F12"));
    }
    #[test]
    fn overlays_share_font_cache_reuse_buffers_and_drop_when_closed() {
        let config = Config::default();
        let path = Path::new("/tmp/config.toml");
        let notices = notifications::Runtime::default();
        let mut ui = Runtime::default();
        ui.toggle();
        assert!(ui.update(&config, path, 0, false, (1280, 800).into(), &notices));
        let count = ui.rasterizations;
        let font = panels::load_font(&config.panels, path).unwrap();
        assert!(Arc::ptr_eq(ui.font.as_ref().unwrap(), &font));
        for _ in 0..100 {
            assert!(!ui.update(&config, path, 0, false, (1280, 800).into(), &notices));
        }
        assert_eq!(ui.rasterizations, count);
        ui.close();
        assert!(ui.update(&config, path, 0, false, (1280, 800).into(), &notices));
        assert!(ui.font.is_none() && ui.help.is_none() && ui.rows.is_empty());
        assert_eq!(ui.status()["buffer_bytes"], 0);
    }
    #[test]
    fn small_shortcut_windows_page_and_custom_fonts_validate_when_panels_are_off() {
        let mut config = Config::default();
        config.shortcuts.width = 320;
        let mut ui = Runtime::default();
        let notices = notifications::Runtime::default();
        ui.toggle();
        ui.update(
            &config,
            Path::new("/tmp/config.toml"),
            0,
            false,
            (640, 400).into(),
            &notices,
        );
        assert!(ui.pages > 1);
        ui.scroll(true);
        assert!(ui.update(
            &config,
            Path::new("/tmp/config.toml"),
            0,
            false,
            (640, 400).into(),
            &notices
        ));
        assert_eq!(ui.help_page, 1);
        config.panels.font = "/missing/font.ttf".into();
        assert!(validate_resources(&config, Path::new("/tmp/config.toml")).is_err());
        config.shortcuts.enabled = false;
        config.notifications.enabled = false;
        config.wifi.enabled = false;
        config.launcher.enabled = false;
        assert!(validate_resources(&config, Path::new("/tmp/config.toml")).is_err());
        config.tray.enabled = false;
        config.audio.osd_enabled = false;
        assert!(validate_resources(&config, Path::new("/tmp/config.toml")).is_ok());
    }
    #[test]
    fn notification_buttons_hit_actions_and_wrap_is_bounded() {
        let config = Config::default();
        let mut notices = notifications::Runtime::default();
        let id = notices.notify(
            &config.notifications,
            notifications::Request {
                app: "Tests",
                replaces: 0,
                summary: "Title",
                body: "Body",
                actions: vec![("copy".into(), "Copy".into())],
                urgency: 1,
                resident: false,
                timeout: 0,
                sender: None,
            },
        );
        let mut ui = Runtime::default();
        ui.update(
            &config,
            Path::new("/tmp/config.toml"),
            0,
            false,
            (1280, 800).into(),
            &notices,
        );
        let button = &ui.cards[0].buttons[0];
        let pos = button.rect.loc.to_f64() + Point::from((2., 2.));
        assert!(matches!(ui.click(pos,false),Some(Interaction::Invoke(n,k)) if n==id&&k=="copy"));
        assert!(matches!(ui.click(pos,true),Some(Interaction::Dismiss(n)) if n==id));
        let font = ui.font.as_ref().unwrap();
        assert!(wrap(font, &"x".repeat(500), 12.0, 100, 3).len() <= 3);
        assert!(wrap(font, "", 12.0, 100, 3).is_empty());
    }
}
