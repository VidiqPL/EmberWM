//! On-demand shell chrome shared by Wi-Fi, tray and power controls.
use crate::{
    config::{color, Config},
    layout::Rect,
    panels,
    state::Ember,
    ui,
};
use serde::{Deserialize, Serialize};
use smithay::{
    input::keyboard::{Keysym, ModifiersState},
    utils::{Logical, Point, Rectangle, Size},
};
use std::path::Path;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Tab {
    #[default]
    Wifi,
    Tray,
    Power,
}
impl Tab {
    pub fn name(self) -> &'static str {
        match self {
            Self::Wifi => "Wi-Fi",
            Self::Tray => "Tray",
            Self::Power => "Power",
        }
    }
}
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub title: String,
    pub width: i32,
    pub height: i32,
    pub tabs: Vec<Tab>,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            title: "EmberWM".into(),
            width: 640,
            height: 620,
            tabs: vec![Tab::Wifi, Tab::Tray, Tab::Power],
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if !(400..=1200).contains(&self.width)
            || !(400..=1000).contains(&self.height)
            || self.title.chars().count() > 64
            || self.title.chars().any(char::is_control)
            || self
                .tabs
                .iter()
                .enumerate()
                .any(|(i, t)| self.tabs[..i].contains(t))
        {
            return Err("menu dimensions/title/tabs are invalid".into());
        }
        Ok(())
    }
    pub fn available(&self, c: &Config) -> Vec<Tab> {
        self.tabs
            .iter()
            .copied()
            .filter(|t| match t {
                Tab::Wifi => c.wifi.enabled,
                Tab::Tray => c.tray.enabled,
                Tab::Power => true,
            })
            .collect()
    }
}
#[derive(Default)]
pub struct Runtime {
    pub open: bool,
    pub tab: Tab,
    pub view: Option<ui::View>,
    pub body: Option<Rect>,
    rect: Option<Rect>,
    tabs: Vec<(Tab, Rect)>,
    key: Option<(u64, i32, i32, Tab)>,
}
impl Runtime {
    fn close(&mut self) {
        self.open = false;
        self.view = None;
        self.rect = None;
        self.body = None;
        self.key = None;
        self.tabs = Vec::new();
        crate::memory::request_reclaim();
    }
    pub fn status(&self) -> serde_json::Value {
        serde_json::json!({"open":self.open,"tab":self.tab,"buffer_bytes":self.view.as_ref().map_or(0,|v|v.rect.size.w as u64*v.rect.size.h as u64*4),"tabs":self.tabs.iter().map(|(tab,r)|serde_json::json!({"tab":tab,"x":r.loc.x,"y":r.loc.y,"width":r.size.w,"height":r.size.h})).collect::<Vec<_>>(),"body":self.body.map(|r|serde_json::json!({"x":r.loc.x,"y":r.loc.y,"width":r.size.w,"height":r.size.h}))})
    }
    pub fn update(
        &mut self,
        c: &Config,
        path: &Path,
        generation: u64,
        size: Size<i32, Logical>,
    ) -> bool {
        if !self.open {
            return false;
        }
        let key = (generation, size.w, size.h, self.tab);
        if self.key == Some(key) {
            return false;
        }
        self.key = Some(key);
        let width = c.menu.width.min((size.w - 24).max(1));
        let height = c.menu.height.min((size.h - 24).max(104));
        let rect = Rectangle::new(
            ((size.w - width) / 2, (size.h - height) / 2).into(),
            (width, height).into(),
        );
        self.rect = Some(rect);
        self.body = Some(Rectangle::new(
            (rect.loc.x, rect.loc.y + 102).into(),
            (width, height - 102).into(),
        ));
        let available = c.menu.available(c);
        let slot = (width - 32) / available.len().max(1) as i32;
        self.tabs = available
            .iter()
            .enumerate()
            .map(|(i, t)| {
                (
                    *t,
                    Rectangle::new(
                        (rect.loc.x + 16 + i as i32 * slot, rect.loc.y + 62).into(),
                        (slot - 6, 30).into(),
                    ),
                )
            })
            .collect();
        let Ok(font) = panels::load_font(&c.panels, path) else {
            return true;
        };
        let bg = color(&c.panels.background).unwrap();
        let fg = color(&c.panels.foreground).unwrap();
        let accent = color(&c.panels.accent).unwrap();
        let border = color(&c.panels.border_color).unwrap();
        let buffer = ui::surface(width, 102, |p| {
            panels::fill(p, (width, 102), (0, 0, width, 102), bg);
            for edge in [
                (0, 0, width, 1),
                (0, 101, width, 1),
                (0, 0, 1, 102),
                (width - 1, 0, 1, 102),
            ] {
                panels::fill(p, (width, 102), edge, accent);
            }
            ui::text(p, (width, 102), &font, &c.menu.title, 21., 18, 12, fg);
            ui::text(
                p,
                (width, 102),
                &font,
                "Control center  /  Ctrl+Tab: switch tabs",
                11.,
                18,
                39,
                accent,
            );
            ui::text(p, (width, 102), &font, "×", 20., width - 35, 13, accent);
            for (t, r) in &self.tabs {
                let x = r.loc.x - rect.loc.x;
                let y = r.loc.y - rect.loc.y;
                panels::fill(
                    p,
                    (width, 102),
                    (x, y, r.size.w, r.size.h),
                    if *t == self.tab { accent } else { border },
                );
                ui::text(p, (width, 102), &font, t.name(), 13., x + 12, y + 6, fg);
            }
        });
        self.view = Some(ui::View::overlay(
            Rectangle::new(rect.loc, (width, 102).into()),
            buffer,
        ));
        true
    }
}
impl Ember {
    pub fn close_menu(&mut self) {
        self.menu.close();
        self.wifi.close();
        self.tray.close();
        self.power.close();
        self.dirty = true;
    }
    pub fn select_menu(&mut self, tab: Tab) {
        if !self.config.menu.enabled || !self.config.menu.available(&self.config).contains(&tab) {
            return;
        }
        if self.menu.open && self.menu.tab == tab {
            return;
        }
        self.close_menu();
        self.launcher.close();
        self.ui.close();
        self.draw = None;
        self.drag = None;
        self.suspend_pointer();
        self.menu.open = true;
        self.menu.tab = tab;
        match tab {
            Tab::Wifi => self.wifi.toggle(),
            Tab::Tray => self.tray.toggle(),
            Tab::Power => self.power.open(),
        }
        self.dirty = true;
    }
    pub fn toggle_menu(&mut self) {
        if self.menu.open {
            self.close_menu();
        } else if let Some(tab) = self.config.menu.available(&self.config).first().copied() {
            self.select_menu(tab);
        }
    }
    pub fn menu_key(&mut self, sym: Keysym, modified: Keysym, mods: &ModifiersState) {
        if sym == Keysym::Escape {
            if self.menu.tab == Tab::Power && self.power.cancel_confirmation() {
                self.dirty = true;
            } else if self.menu.tab == Tab::Wifi && self.wifi.has_selection() {
                self.wifi.key(sym, mods);
            } else {
                self.close_menu();
            }
            return;
        }
        let tabs = self.config.menu.available(&self.config);
        if mods.ctrl && matches!(sym, Keysym::Tab | Keysym::ISO_Left_Tab) {
            let i = tabs.iter().position(|t| *t == self.menu.tab).unwrap_or(0);
            if !tabs.is_empty() {
                let next = if mods.shift {
                    (i + tabs.len() - 1) % tabs.len()
                } else {
                    (i + 1) % tabs.len()
                };
                self.select_menu(tabs[next]);
            }
        } else if mods.alt && (b'1' as u32..=b'3' as u32).contains(&sym.raw()) {
            if let Some(t) = tabs.get((sym.raw() - b'1' as u32) as usize) {
                self.select_menu(*t);
            }
        } else {
            match self.menu.tab {
                Tab::Wifi => self.wifi.key(modified, mods),
                Tab::Tray => self.tray.key(sym),
                Tab::Power => {
                    if self.power.key(sym) {
                        let _ = self.action("quit");
                    }
                }
            }
        }
        if self.power.release_modal {
            self.close_menu();
        }
        self.dirty = true;
    }
    pub fn menu_click(&mut self, pos: Point<f64, Logical>, right: bool) {
        let Some(rect) = self.menu.rect else { return };
        if !rect.to_f64().contains(pos) {
            self.close_menu();
            return;
        }
        if pos.y < (rect.loc.y + 102) as f64 {
            if !right {
                if pos.x > (rect.loc.x + rect.size.w - 48) as f64
                    && pos.y < (rect.loc.y + 52) as f64
                {
                    self.close_menu();
                } else if let Some(t) = self
                    .menu
                    .tabs
                    .iter()
                    .find(|(_, r)| r.to_f64().contains(pos))
                    .map(|(t, _)| *t)
                {
                    self.select_menu(t);
                }
            }
        } else {
            match self.menu.tab {
                Tab::Wifi => self.wifi.click(pos, right),
                Tab::Tray => self.tray.click(pos, right),
                Tab::Power => {
                    if self.power.click(pos, right) {
                        let _ = self.action("quit");
                    } else if self.power.release_modal {
                        self.close_menu();
                    }
                }
            }
        }
        if self.power.release_modal {
            self.close_menu();
        }
        self.dirty = true;
    }
    pub fn update_menu(&mut self, size: Size<i32, Logical>) -> bool {
        if self.menu.open {
            let available = self.config.menu.available(&self.config);
            if !self.config.menu.enabled || available.is_empty() {
                self.close_menu();
                return true;
            }
            if !available.contains(&self.menu.tab) {
                self.select_menu(available[0]);
            }
            let child_open = match self.menu.tab {
                Tab::Wifi => self.wifi.open,
                Tab::Tray => self.tray.open,
                Tab::Power => self.power.open,
            };
            if !child_open {
                self.close_menu();
                return true;
            }
        }
        self.menu
            .update(&self.config, &self.config_path, self.theme_generation, size)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tabs_follow_configuration_and_closed_chrome_releases_pixels() {
        let mut config = Config::default();
        config.menu.tabs = vec![Tab::Power, Tab::Wifi];
        config.wifi.enabled = false;
        assert_eq!(config.menu.available(&config), vec![Tab::Power]);
        let mut runtime = Runtime {
            open: true,
            tab: Tab::Power,
            ..Default::default()
        };
        assert!(runtime.update(&config, Path::new("/tmp/config.toml"), 0, (800, 700).into()));
        assert!(!runtime.update(&config, Path::new("/tmp/config.toml"), 0, (800, 700).into()));
        let body = runtime.body.unwrap();
        assert!(body.size.h > 300 && body.loc.y >= 102);
        assert_eq!(runtime.tabs.len(), 1);
        runtime.close();
        assert!(runtime.view.is_none() && runtime.body.is_none() && runtime.tabs.is_empty());
        config.menu.tabs = vec![Tab::Wifi, Tab::Wifi];
        assert!(config.menu.validate().is_err());
    }
}
