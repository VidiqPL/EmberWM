use serde::{Deserialize, Serialize};
use smithay::input::keyboard::{xkb, Keysym, ModifiersState};
use std::{
    collections::HashSet,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    pub xwayland: crate::xwayland::Settings,
    pub rice: crate::rice::Settings,
    pub screencast: crate::screencast::Settings,
    pub launcher: crate::launcher::Settings,
    pub menu: crate::menu::Settings,
    pub tray: crate::tray::Settings,
    pub wifi: crate::wifi::Settings,
    pub panels: crate::panels::Settings,
    pub notifications: crate::notifications::Settings,
    pub shortcuts: crate::ui::ShortcutSettings,
    pub display: Display,
    pub audio: Audio,
    pub theme: Theme,
    pub layout: Layout,
    pub keyboard: Keyboard,
    pub terminal: Vec<String>,
    pub bindings: Vec<Binding>,
    pub window_rules: Vec<crate::rules::Rule>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Display {
    /// Zero selects the preferred mode. Otherwise choose the nearest advertised rate.
    pub refresh_hz: f64,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Audio {
    pub enabled: bool,
    pub osd_enabled: bool,
    pub osd_duration_ms: u64,
}
impl Default for Audio {
    fn default() -> Self {
        Self {
            enabled: true,
            osd_enabled: true,
            osd_duration_ms: 1000,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Theme {
    pub preset: String,
    pub background: String,
    pub active_border: String,
    pub inactive_border: String,
    pub selection: String,
    pub selection_opacity: f32,
    pub border_width: i32,
    /// PNG/JPEG path, relative to the config file; empty means solid background.
    pub wallpaper: String,
    pub wallpaper_mode: String,
    /// Empty keeps original colours; [shadow, highlight] maps luminance on upload.
    pub wallpaper_palette: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Layout {
    pub gap: i32,
    pub master_ratio: f64,
    pub workspaces: usize,
    pub default_floating: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Keyboard {
    pub modifier: String,
    pub xkb_layout: String,
    pub repeat_delay: i32,
    pub repeat_rate: i32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Binding {
    pub key: String,
    pub action: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub shift: bool,
    #[serde(default = "default_true")]
    pub modifier: bool,
}
fn default_true() -> bool {
    true
}

impl Default for Theme {
    fn default() -> Self {
        Self {
            preset: String::new(),
            background: "#190e14".into(),
            active_border: "#cf4763".into(),
            inactive_border: "#4c2835".into(),
            selection: "#e76b85".into(),
            selection_opacity: 0.0,
            border_width: 2,
            wallpaper: String::new(),
            wallpaper_mode: "fill".into(),
            wallpaper_palette: Vec::new(),
        }
    }
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            gap: 12,
            master_ratio: 0.58,
            workspaces: 10,
            default_floating: false,
        }
    }
}
impl Default for Keyboard {
    fn default() -> Self {
        Self {
            modifier: "Alt".into(),
            xkb_layout: "us".into(),
            repeat_delay: 300,
            repeat_rate: 30,
        }
    }
}
impl Default for Config {
    fn default() -> Self {
        let mut bindings = vec![
            Binding::new("Return", "terminal"),
            Binding::new("Escape", "pointer-release"),
            Binding::new("d", "draw"),
            Binding::new("g", "scratchpad-toggle"),
            Binding::new("q", "close"),
            Binding::new("space", "launcher-toggle"),
            Binding::new("j", "focus-next"),
            Binding::new("k", "focus-prev"),
            Binding::new("h", "ratio-down"),
            Binding::new("l", "ratio-up"),
            Binding::new("r", "reload"),
            Binding::new("t", "theme-cycle"),
            Binding::new("f", "fullscreen"),
            Binding::new("b", "panels-toggle"),
            Binding::new("slash", "shortcuts-toggle"),
            Binding::new("period", "menu-toggle"),
            Binding::new("comma", "tray-toggle"),
            Binding::new("n", "notification-dismiss"),
        ];
        bindings.push(Binding {
            key: "g".into(),
            action: "scratchpad-mark".into(),
            shift: true,
            modifier: true,
            description: None,
        });
        bindings.push(Binding {
            key: "t".into(),
            action: "rice-cycle".into(),
            shift: true,
            modifier: true,
            description: None,
        });
        bindings.push(Binding {
            key: "space".into(),
            action: "float".into(),
            shift: true,
            modifier: true,
            description: None,
        });
        bindings.push(Binding {
            key: "s".into(),
            action: "screenshot-region".into(),
            description: None,
            shift: true,
            modifier: true,
        });
        bindings.push(Binding {
            key: "Escape".into(),
            action: "quit".into(),
            shift: true,
            modifier: true,
            description: None,
        });
        for n in 1..=10 {
            let key = (n % 10).to_string();
            bindings.push(Binding::new(&key, &format!("workspace {n}")));
            bindings.push(Binding {
                key,
                action: format!("send {n}"),
                shift: true,
                modifier: true,
                description: None,
            });
        }
        bindings.push(Binding {
            key: "F11".into(),
            action: "fullscreen".into(),
            description: None,
            shift: false,
            modifier: false,
        });
        for (key, action) in [
            ("XF86AudioRaiseVolume", "volume-up"),
            ("XF86AudioLowerVolume", "volume-down"),
            ("XF86AudioMute", "mute"),
            ("XF86AudioMicMute", "mic-mute"),
        ] {
            bindings.push(Binding {
                key: key.into(),
                action: action.into(),
                shift: false,
                modifier: false,
                description: None,
            });
        }
        Self {
            xwayland: Default::default(),
            rice: crate::rice::Settings::default(),
            screencast: crate::screencast::Settings::default(),
            launcher: crate::launcher::Settings::default(),
            menu: Default::default(),
            tray: Default::default(),
            wifi: crate::wifi::Settings::default(),
            panels: crate::panels::Settings::default(),
            notifications: crate::notifications::Settings::default(),
            shortcuts: crate::ui::ShortcutSettings::default(),
            display: Display::default(),
            audio: Audio::default(),
            theme: Theme::default(),
            layout: Layout::default(),
            keyboard: Keyboard::default(),
            terminal: vec![
                "kitty".into(),
                "--class".into(),
                "{app_id}".into(),
                "--override".into(),
                "linux_display_server=wayland".into(),
                "--override".into(),
                "background={background}".into(),
                "--override".into(),
                "foreground={foreground}".into(),
                "--override".into(),
                "hide_window_decorations=yes".into(),
            ],
            bindings,
            window_rules: Vec::new(),
        }
    }
}
impl Binding {
    fn new(key: &str, action: &str) -> Self {
        Self {
            key: key.into(),
            action: action.into(),
            shift: false,
            modifier: true,
            description: None,
        }
    }
}

pub fn color(s: &str) -> Result<[f32; 4], String> {
    let hex = s.strip_prefix('#').ok_or("colour must start with #")?;
    if hex.len() != 6 || !hex.is_ascii() {
        return Err("colour must be #RRGGBB".into());
    }
    let n = u32::from_str_radix(hex, 16).map_err(|_| "invalid hex colour")?;
    Ok([
        ((n >> 16) & 255) as f32 / 255.,
        ((n >> 8) & 255) as f32 / 255.,
        (n & 255) as f32 / 255.,
        1.,
    ])
}

pub fn valid_action(action: &str) -> bool {
    match action {
        "rice-cycle"
        | "theme-cycle"
        | "pointer-release"
        | "terminal"
        | "draw"
        | "scratchpad-mark"
        | "scratchpad-toggle"
        | "screenshot-region"
        | "close"
        | "float"
        | "focus-next"
        | "focus-prev"
        | "ratio-down"
        | "ratio-up"
        | "reload"
        | "fullscreen"
        | "quit"
        | "volume-up"
        | "volume-down"
        | "mute"
        | "mic-mute"
        | "panels-toggle"
        | "shortcuts-toggle"
        | "menu-toggle"
        | "power-toggle"
        | "tray-toggle"
        | "wifi-toggle"
        | "launcher-toggle"
        | "notification-dismiss"
        | "notifications-clear"
        | "notifications-toggle-dnd"
        | "rules-apply" => true,
        _ if action.starts_with("rice ") => {
            &action[5..] == "default" || crate::rice::valid_name(&action[5..])
        }
        _ if action.starts_with("theme ") => !action[6..].trim().is_empty(),
        _ if action.starts_with("workspace ") || action.starts_with("send ") => action
            .split_once(' ')
            .and_then(|(_, n)| n.parse::<usize>().ok())
            .is_some_and(|n| (1..=10).contains(&n)),
        _ => action
            .strip_prefix("exec ")
            .is_some_and(|s| !s.trim().is_empty()),
    }
}

impl Config {
    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut value: toml::Value = text.parse::<toml::Value>().map_err(|e| e.to_string())?;
        let has_rice = value
            .get("rice")
            .and_then(|v| v.get("profile"))
            .and_then(|v| v.as_str())
            .is_some_and(|s| !s.is_empty());
        if let Some(name) = value
            .get("theme")
            .and_then(|v| v.get("preset"))
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty() && !has_rice)
        {
            let mut preset = crate::themes::patch(path, name)?;
            crate::themes::merge(&mut preset, value);
            value = preset;
        }
        let mut config: Self = value
            .try_into()
            .map_err(|e: toml::de::Error| e.to_string())?;
        if !config.rice.profile.is_empty() {
            config = crate::rice::load(&config, path, &config.rice.profile)?;
        }
        config.validate()?;
        Ok(config)
    }
    pub fn validate(&self) -> Result<(), String> {
        if !self.rice.profile.is_empty() && !crate::rice::valid_name(&self.rice.profile) {
            return Err("invalid rice.profile name".into());
        }
        crate::rules::validate(&self.window_rules, self.layout.workspaces)?;
        self.panels.validate()?;
        self.notifications.validate()?;
        self.shortcuts.validate()?;
        if !self.xwayland.scroll_factor.is_finite()
            || !(0.1..=10.0).contains(&self.xwayland.scroll_factor)
        {
            return Err("Xwayland scroll_factor must be between 0.1 and 10".into());
        }
        if !(100..=10000).contains(&self.audio.osd_duration_ms) {
            return Err("Audio OSD duration must be between 100 and 10000 ms".into());
        }
        self.menu.validate()?;
        self.tray.validate()?;
        self.wifi.validate()?;
        self.launcher.validate()?;
        if !self.display.refresh_hz.is_finite()
            || self.display.refresh_hz < 0.0
            || (self.display.refresh_hz > 0.0 && self.display.refresh_hz < 1.0)
            || self.display.refresh_hz > 1000.0
        {
            return Err("display.refresh_hz must be 0 (preferred) or between 1 and 1000 Hz".into());
        }
        if !self.theme.selection_opacity.is_finite()
            || !(0.0..=1.0).contains(&self.theme.selection_opacity)
        {
            return Err("selection_opacity must be 0..1".into());
        }
        if !["fill", "fit", "stretch", "center"].contains(&self.theme.wallpaper_mode.as_str()) {
            return Err("wallpaper_mode must be fill, fit, stretch or center".into());
        }
        if !self.theme.wallpaper_palette.is_empty() && self.theme.wallpaper_palette.len() != 2 {
            return Err("wallpaper_palette must be empty or [shadow, highlight]".into());
        }
        for entry in &self.theme.wallpaper_palette {
            color(entry)?;
        }
        for s in [
            &self.theme.background,
            &self.theme.active_border,
            &self.theme.inactive_border,
            &self.theme.selection,
        ] {
            color(s)?;
        }
        if !(0..=12).contains(&self.theme.border_width) || !(0..=100).contains(&self.layout.gap) {
            return Err("border_width must be 0..12; gap must be 0..100".into());
        }
        if !self.layout.master_ratio.is_finite() || !(0.2..=0.8).contains(&self.layout.master_ratio)
        {
            return Err("master_ratio must be between 0.2 and 0.8".into());
        }
        if !(1..=10).contains(&self.layout.workspaces) {
            return Err("workspaces must be 1..10".into());
        }
        if self.terminal.is_empty() || self.terminal[0].is_empty() {
            return Err("terminal command is empty".into());
        }
        if !["Alt", "Super", "Ctrl"].contains(&self.keyboard.modifier.as_str()) {
            return Err("modifier must be Alt, Super or Ctrl".into());
        }
        if !(100..=2000).contains(&self.keyboard.repeat_delay)
            || !(1..=100).contains(&self.keyboard.repeat_rate)
        {
            return Err("invalid keyboard repeat settings".into());
        }
        let mut seen = HashSet::new();
        for b in &self.bindings {
            if b.description
                .as_ref()
                .is_some_and(|s| s.len() > 240 || s.chars().any(char::is_control))
            {
                return Err("binding description too long or contains control characters".into());
            }
            let sym = xkb::keysym_from_name(&b.key, xkb::KEYSYM_CASE_INSENSITIVE);
            if sym.raw() == 0 {
                return Err(format!("unknown key: {}", b.key));
            }
            if !seen.insert((sym.raw(), b.shift, b.modifier)) {
                return Err(format!("duplicate binding: {}", b.key));
            }
            if !valid_action(&b.action) {
                return Err(format!("unknown action: {}", b.action));
            }
        }
        Ok(())
    }
    pub fn action(&self, mods: &ModifiersState, sym: Keysym) -> Option<String> {
        let modifier = match self.keyboard.modifier.as_str() {
            "Super" => mods.logo && !mods.alt && !mods.ctrl,
            "Ctrl" => mods.ctrl && !mods.alt && !mods.logo,
            _ => mods.alt && !mods.ctrl && !mods.logo,
        };
        self.bindings
            .iter()
            .find(|b| {
                (if b.modifier {
                    modifier
                } else {
                    !mods.alt && !mods.ctrl && !mods.logo
                }) && b.shift == mods.shift
                    && xkb::keysym_from_name(&b.key, xkb::KEYSYM_CASE_INSENSITIVE) == sym
            })
            .map(|b| b.action.clone())
    }
    pub fn wallpaper_path(&self, config_path: &Path) -> Option<PathBuf> {
        if self.theme.wallpaper.is_empty() {
            return None;
        }
        let p = PathBuf::from(&self.theme.wallpaper);
        Some(if p.is_absolute() {
            p
        } else {
            config_path.parent().unwrap_or(Path::new(".")).join(p)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn scratchpad_bindings_distinguish_shift_and_validate() {
        let mut config = Config::default();
        config.keyboard.modifier = "Super".into();
        let mut mods = ModifiersState {
            logo: true,
            ..Default::default()
        };
        let key = xkb::keysym_from_name("g", xkb::KEYSYM_CASE_INSENSITIVE);
        assert_eq!(
            config.action(&mods, key).as_deref(),
            Some("scratchpad-toggle")
        );
        mods.shift = true;
        assert_eq!(
            config.action(&mods, key).as_deref(),
            Some("scratchpad-mark")
        );
        mods.ctrl = true;
        assert_eq!(config.action(&mods, key), None);
        config.validate().unwrap();
    }

    #[test]
    fn appearance_ranges_and_wallpaper_palette_validate() {
        let mut config = Config::default();
        config.theme.wallpaper_palette = vec!["#211217".into(), "#a65b70".into()];
        config.theme.selection_opacity = 0.4;
        config.panels.workspace_padding = 9;
        config.validate().unwrap();
        config.theme.wallpaper_palette.pop();
        assert!(config.validate().unwrap_err().contains("wallpaper_palette"));
        config.theme.wallpaper_palette.push("invalid".into());
        assert!(config.validate().is_err());
        config.theme.wallpaper_palette.clear();
        for opacity in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
            config.theme.selection_opacity = opacity;
            assert!(config.validate().is_err());
        }
        config.theme.selection_opacity = 0.;
        config.panels.workspace_padding = 25;
        assert!(config.validate().is_err());
    }

    #[test]
    fn wifi_binding_and_settings_validate() {
        let mut config = Config::default();
        config.keyboard.modifier = "Super".into();
        let mods = ModifiersState {
            logo: true,
            ..Default::default()
        };
        assert_eq!(
            config.action(&mods, Keysym::period).as_deref(),
            Some("menu-toggle")
        );
        config.wifi.width = 0;
        assert!(config.validate().is_err());
    }
    #[test]
    fn ten_workspaces_and_f11_are_configurable_bindings() {
        let mut config = Config::default();
        config.keyboard.modifier = "Super".into();
        let mut mods = ModifiersState::default();
        let f11 = xkb::keysym_from_name("F11", 0);
        assert_eq!(config.action(&mods, f11).as_deref(), Some("fullscreen"));
        mods.logo = true;
        assert!(config.action(&mods, f11).is_none());
        assert_eq!(
            config
                .action(&mods, xkb::keysym_from_name("0", 0))
                .as_deref(),
            Some("workspace 10")
        );
        mods.shift = true;
        assert_eq!(
            config
                .action(&mods, xkb::keysym_from_name("0", 0))
                .as_deref(),
            Some("send 10")
        );
        assert_eq!(config.layout.workspaces, 10);
        config.validate().unwrap();
        config.layout.workspaces = 11;
        assert!(config.validate().is_err());
    }
    #[test]
    fn media_keys_work_without_modifier_and_normal_keys_are_forwarded() {
        let config = Config::default();
        let mut mods = ModifiersState::default();
        let mute = xkb::keysym_from_name("XF86AudioMute", 0);
        assert_eq!(config.action(&mods, mute).as_deref(), Some("mute"));
        assert!(config
            .action(&mods, xkb::keysym_from_name("q", 0))
            .is_none());
        mods.logo = true;
        assert!(config.action(&mods, mute).is_none());
        let super_config = Config {
            keyboard: Keyboard {
                modifier: "Super".into(),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(
            super_config
                .action(&mods, xkb::keysym_from_name("q", 0))
                .as_deref(),
            Some("close")
        );
    }
    #[test]
    fn rejects_broken_themes_and_duplicate_shortcuts() {
        let mut c = Config::default();
        c.validate().unwrap();
        c.theme.active_border = "#zz0000".into();
        assert!(c.validate().is_err());
        c.theme = Theme::default();
        c.bindings.push(c.bindings[0].clone());
        assert!(c.validate().is_err());
    }
    #[test]
    fn rejects_unknown_options_and_nonfinite_layout() {
        assert!(toml::from_str::<Config>("typo = true").is_err());
        let mut c = Config::default();
        c.layout.master_ratio = f64::NAN;
        assert!(c.validate().is_err());
    }
}
