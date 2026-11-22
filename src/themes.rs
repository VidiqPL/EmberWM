//! Small TOML appearance presets; loaded only on selection/reload, never a theme daemon.
use crate::{config::Config, Ember};
use std::path::{Path, PathBuf};
const BUILTINS: [&str; 5] = ["red", "midnight", "forest", "mono", "light"];
const KEYS: [(&str, &[&str]); 3] = [
    (
        "theme",
        &[
            "background",
            "active_border",
            "inactive_border",
            "selection",
            "selection_opacity",
            "border_width",
            "wallpaper",
            "wallpaper_mode",
            "wallpaper_palette",
        ],
    ),
    (
        "panels",
        &[
            "font",
            "font_size",
            "height",
            "margin",
            "padding",
            "spacing",
            "workspace_padding",
            "background",
            "foreground",
            "accent",
            "border_color",
            "border_width",
            "opacity",
            "items",
        ],
    ),
    ("layout", &["gap"]),
];
pub fn merge(base: &mut toml::Value, patch: toml::Value) {
    if let (Some(a), Some(b)) = (base.as_table_mut(), patch.as_table()) {
        for (k, v) in b {
            if let Some(old) = a.get_mut(k) {
                merge(old, v.clone());
            } else {
                a.insert(k.clone(), v.clone());
            }
        }
    } else {
        *base = patch;
    }
}
fn preset_path(config: &Path, name: &str) -> PathBuf {
    let path = PathBuf::from(name);
    if path.is_absolute() {
        path
    } else {
        config
            .parent()
            .unwrap_or(Path::new("."))
            .join("themes")
            .join(if path.extension().is_some() {
                path
            } else {
                path.with_extension("toml")
            })
    }
}
pub fn patch(config: &Path, name: &str) -> Result<toml::Value, String> {
    if !BUILTINS.contains(&name) {
        let path = preset_path(config, name);
        if std::fs::metadata(&path).map_err(|e| e.to_string())?.len() > 65536 {
            return Err("theme preset exceeds 64 KiB".into());
        }
        let mut patch: toml::Value = std::fs::read_to_string(&path)
            .map_err(|e| format!("{}: {e}", path.display()))?
            .parse::<toml::Value>()
            .map_err(|e| e.to_string())?;
        validate_patch(&patch)?;
        // Asset paths in a preset are relative to that preset's directory.
        for (section, key) in [("theme", "wallpaper"), ("panels", "font")] {
            if let Some(value) = patch.get_mut(section).and_then(|v| v.get_mut(key)) {
                if let Some(text) = value.as_str().filter(|s| !s.is_empty() && *s != "builtin") {
                    let p = Path::new(text);
                    if !p.is_absolute() {
                        *value = toml::Value::String(
                            path.parent()
                                .unwrap()
                                .join(p)
                                .to_string_lossy()
                                .into_owned(),
                        );
                    }
                }
            }
        }
        return Ok(patch);
    }
    let (background, active, inactive, selection, panel, fg) = match name {
        "midnight" => (
            "#101526", "#7aa2f7", "#303952", "#bb9af7", "#181f33", "#c0caf5",
        ),
        "forest" => (
            "#111b16", "#8ec07c", "#344a3b", "#b8bb26", "#1b2a21", "#e1ead8",
        ),
        "mono" => (
            "#101010", "#dddddd", "#444444", "#aaaaaa", "#1b1b1b", "#eeeeee",
        ),
        "light" => (
            "#f0ece7", "#a33651", "#c5b9b4", "#b63b5d", "#e5ddd6", "#302530",
        ),
        _ => (
            "#23181a", "#793b48", "#23181a", "#b35c70", "#201517", "#d4c5c8",
        ),
    };
    // Colour presets leave wallpaper, framing and its palette intact. Complete
    // rice profiles still control whether an image is present (e.g. minimal).
    let (accent, border, width) = if name == "red" {
        ("#b86b7c", "#60313c", 1)
    } else {
        (active, inactive, 2)
    };
    format!("[theme]\nbackground='{background}'\nactive_border='{active}'\ninactive_border='{inactive}'\nselection='{selection}'\nborder_width={width}\n[panels]\nbackground='{panel}'\nforeground='{fg}'\naccent='{accent}'\nborder_color='{border}'\n").parse::<toml::Value>().map_err(|e|e.to_string())
}
fn validate_patch(patch: &toml::Value) -> Result<(), String> {
    let sections = patch
        .as_table()
        .ok_or("theme preset must contain TOML tables")?;
    for (section, value) in sections {
        let keys = KEYS
            .iter()
            .find(|(s, _)| s == section)
            .map(|(_, keys)| *keys)
            .ok_or_else(|| format!("unsupported theme section: {section}"))?;
        for key in value
            .as_table()
            .ok_or("theme sections must be tables")?
            .keys()
        {
            if !keys.contains(&key.as_str()) {
                return Err(format!("unsupported theme field: {section}.{key}"));
            }
        }
    }
    Ok(())
}
pub fn names(config: &Path) -> Vec<String> {
    let mut names = BUILTINS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if let Ok(entries) = std::fs::read_dir(config.parent().unwrap_or(Path::new(".")).join("themes"))
    {
        let mut custom = entries
            .take(128)
            .filter_map(Result::ok)
            .filter_map(|e| {
                let p = e.path();
                if p.extension().is_some_and(|e| e == "toml")
                    && e.file_type().is_ok_and(|f| f.is_file())
                {
                    p.file_stem().and_then(|s| s.to_str()).map(str::to_string)
                } else {
                    None
                }
            })
            .filter(|n| !BUILTINS.contains(&n.as_str()))
            .collect::<Vec<_>>();
        custom.sort();
        names.extend(custom);
    }
    names
}
pub fn apply(config: &Config, path: &Path, name: &str) -> Result<Config, String> {
    let mut value = toml::Value::try_from(config).map_err(|e| e.to_string())?;
    merge(&mut value, patch(path, name)?);
    let mut result: Config = value
        .try_into()
        .map_err(|e: toml::de::Error| e.to_string())?;
    result.theme.preset = name.into();
    result.validate()?;
    crate::render::validate_wallpaper(&result, path)?;
    crate::panels::validate_resources(&result.panels, path)?;
    crate::ui::validate_resources(&result, path)?;
    Ok(result)
}
impl Ember {
    pub fn select_theme(&mut self, name: &str) -> Result<(), String> {
        let config = if name == "default" {
            let config = Config::load(&self.config_path)?;
            crate::render::validate_wallpaper(&config, &self.config_path)?;
            crate::panels::validate_resources(&config.panels, &self.config_path)?;
            crate::ui::validate_resources(&config, &self.config_path)?;
            config
        } else {
            apply(&self.config, &self.config_path, name)?
        };
        // Only appearance changes. Running apps, bindings and service settings remain the user's.
        self.config.theme = config.theme;
        self.config.panels = config.panels;
        self.config.layout.gap = config.layout.gap;
        self.theme_generation += 1;
        self.relayout();
        crate::memory::request_reclaim();
        Ok(())
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn builtins_validate_and_keep_bindings_and_services() {
        let mut config = Config::default();
        config.theme.wallpaper = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/abbey-oakwood.jpg")
            .to_string_lossy()
            .into_owned();
        config.theme.wallpaper_mode = "fit".into();
        config.theme.wallpaper_palette = vec!["#1b0b0e".into(), "#b3696a".into()];
        let path = Path::new("/tmp/ember-theme-test/config.toml");
        for name in BUILTINS {
            let preset = apply(&config, path, name).unwrap();
            assert_eq!(preset.bindings.len(), config.bindings.len());
            assert_eq!(preset.audio.enabled, config.audio.enabled);
            assert_eq!(preset.theme.wallpaper, config.theme.wallpaper);
            assert_eq!(preset.theme.wallpaper_mode, config.theme.wallpaper_mode);
            assert_eq!(
                preset.theme.wallpaper_palette,
                config.theme.wallpaper_palette
            );
        }
        assert!(validate_patch(&"[audio]\nenabled=false".parse().unwrap()).is_err());
        assert!(validate_patch(&"[theme]\npreset='x'".parse().unwrap()).is_err());
    }
    #[test]
    fn startup_overrides_are_applied_after_preset() {
        let mut base = patch(Path::new("/tmp/config.toml"), "forest").unwrap();
        merge(
            &mut base,
            "[theme]\nactive_border='#ffffff'".parse().unwrap(),
        );
        let c: Config = base.try_into().unwrap();
        assert_eq!(c.theme.background, "#111b16");
        assert_eq!(c.theme.active_border, "#ffffff");
    }
}
