//! Complete appearance profiles, read or saved only on request. No resident profile index.
use crate::{config::Config, Ember};
use serde::{Deserialize, Serialize};
use std::{
    fs::{self, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};
const LIMIT: usize = 65536;
const BUILTINS: [&str; 3] = ["red-dusk", "midnight", "minimal"];
// Services, keybindings, workspace policies and notification delivery are deliberately outside a rice.
const FIELDS: [(&str, &[&str]); 8] = [
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
            "enabled",
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
            "hide_empty_workspaces",
            "hide_on_fullscreen",
            "poll_interval_secs",
            "items",
        ],
    ),
    ("layout", &["gap", "master_ratio"]),
    ("shortcuts", &["width", "font_size"]),
    (
        "notifications",
        &["width", "margin", "gap", "compact", "respect_panel_space"],
    ),
    ("menu", &["title", "width", "height"]),
    ("wifi", &["width", "font_size"]),
    ("launcher", &["width", "font_size", "rows"]),
];
#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    /// Named complete appearance profile, applied after the main config's appearance fields.
    pub profile: String,
}
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 48
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_')
        && !["default", "list", "save", "replace", "cycle"].contains(&name)
}
fn directory(config: &Path) -> PathBuf {
    config.parent().unwrap_or(Path::new(".")).join("rices")
}
fn absolute(path: &Path) -> Result<PathBuf, String> {
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(std::env::current_dir()
            .map_err(|e| e.to_string())?
            .join(path))
    }
}
fn locate(config: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_name(name) {
        return Err("rice name must be 1..48 letters, digits, underscores or hyphens; command names are reserved".into());
    }
    if BUILTINS.contains(&name) {
        return Ok(Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("config/rices")
            .join(format!("{name}.toml")));
    }
    Ok(directory(config).join(format!("{name}.toml")))
}
pub fn names(config: &Path) -> Vec<String> {
    let mut names = BUILTINS.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    if let Ok(entries) = fs::read_dir(directory(config)) {
        let mut custom = entries
            .take(256)
            .filter_map(Result::ok)
            .filter_map(|e| {
                let path = e.path();
                if path.extension().is_some_and(|e| e == "toml")
                    && e.file_type().is_ok_and(|f| f.is_file())
                {
                    path.file_stem()
                        .and_then(|s| s.to_str())
                        .filter(|s| valid_name(s) && !BUILTINS.contains(s))
                        .map(str::to_string)
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        custom.sort();
        names.extend(custom);
    }
    names
}
/// Full supported appearance, including defaults; no previous-profile state can bleed through.
pub fn appearance(config: &Config) -> Result<toml::Value, String> {
    let config = toml::Value::try_from(config).map_err(|e| e.to_string())?;
    let mut result = toml::Table::new();
    for (section, keys) in FIELDS {
        let mut values = toml::Table::new();
        for key in keys {
            if let Some(value) = config.get(section).and_then(|s| s.get(key)) {
                values.insert(key.to_string(), value.clone());
            }
        }
        result.insert(section.into(), toml::Value::Table(values));
    }
    Ok(toml::Value::Table(result))
}
fn read_profile(config: &Path, name: &str) -> Result<toml::Value, String> {
    let path = locate(config, name)?;
    let mut text = String::new();
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    if !file.metadata().map_err(|e| e.to_string())?.is_file() {
        return Err("rice profile must be a regular file".into());
    }
    file.take(LIMIT as u64 + 1)
        .read_to_string(&mut text)
        .map_err(|e| e.to_string())?;
    if text.len() > LIMIT {
        return Err("rice profile exceeds 64 KiB".into());
    }
    let mut document: toml::Value = text.parse::<toml::Value>().map_err(|e| e.to_string())?;
    let table = document
        .as_table_mut()
        .ok_or("rice profile must contain TOML tables")?;
    if table.remove("version").and_then(|v| v.as_integer()) != Some(1) {
        return Err("rice profile requires version = 1".into());
    }
    for (section, values) in table.iter() {
        let keys = FIELDS
            .iter()
            .find(|(s, _)| s == section)
            .map(|(_, keys)| *keys)
            .ok_or_else(|| format!("unsupported rice section: {section}"))?;
        for key in values
            .as_table()
            .ok_or("rice sections must be tables")?
            .keys()
        {
            if !keys.contains(&key.as_str()) {
                return Err(format!("unsupported rice field: {section}.{key}"));
            }
        }
    }
    // Resolve custom assets once. The renderer still retains only the active wallpaper.
    for (section, key) in [("theme", "wallpaper"), ("panels", "font")] {
        if let Some(value) = document.get_mut(section).and_then(|v| v.get_mut(key)) {
            if let Some(text) = value
                .as_str()
                .filter(|s| !s.is_empty() && !(section == "panels" && *s == "builtin"))
            {
                let asset = Path::new(text);
                if !asset.is_absolute() {
                    *value = toml::Value::String(
                        absolute(&path.parent().unwrap_or(Path::new(".")).join(asset))?
                            .to_string_lossy()
                            .into_owned(),
                    );
                }
            }
        }
    }
    let mut complete = appearance(&Config::default())?;
    crate::themes::merge(&mut complete, document);
    Ok(complete)
}
fn overlay(config: &Config, look: toml::Value) -> Result<Config, String> {
    let mut value = toml::Value::try_from(config).map_err(|e| e.to_string())?;
    crate::themes::merge(&mut value, look);
    let mut result: Config = value
        .try_into()
        .map_err(|e: toml::de::Error| e.to_string())?;
    result.theme.preset.clear();
    result.validate()?;
    Ok(result)
}
pub fn load(config: &Config, path: &Path, name: &str) -> Result<Config, String> {
    let mut result = overlay(config, read_profile(path, name)?)?;
    result.rice.profile = name.into();
    Ok(result)
}
fn validate_resources(config: &Config, path: &Path) -> Result<(), String> {
    crate::render::validate_wallpaper(config, path)?;
    crate::panels::validate_resources(&config.panels, path)?;
    crate::ui::validate_resources(config, path)
}
/// Save a complete snapshot using a new file, refusing to overwrite an existing profile.
pub fn save(config: &Config, path: &Path, name: &str) -> Result<PathBuf, String> {
    if !valid_name(name) || BUILTINS.contains(&name) {
        return Err("choose a custom rice name (1..48 letters, digits, underscores or hyphens); built-in and command names are reserved".into());
    }
    validate_resources(config, path)?;
    let mut look = appearance(config)?;
    // Snapshots keep working when saved beside configs with a different parent directory.
    for (section, key) in [("theme", "wallpaper"), ("panels", "font")] {
        let value = look.get_mut(section).and_then(|v| v.get_mut(key)).unwrap();
        let text = value.as_str().unwrap();
        if !text.is_empty() && !(section == "panels" && text == "builtin") {
            let asset = Path::new(text);
            if !asset.is_absolute() {
                *value = toml::Value::String(
                    absolute(&path.parent().unwrap_or(Path::new(".")).join(asset))?
                        .to_string_lossy()
                        .into_owned(),
                );
            }
        }
    }
    look.as_table_mut()
        .unwrap()
        .insert("version".into(), toml::Value::Integer(1));
    let text = toml::to_string_pretty(&look).map_err(|e| e.to_string())?;
    if text.len() > LIMIT {
        return Err("rice snapshot exceeds 64 KiB".into());
    }
    let dir = absolute(&directory(path))?;
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let target = dir.join(format!("{name}.toml"));
    let nonce = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let temporary = dir.join(format!(".rice-{}-{nonce}.tmp", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        file.write_all(text.as_bytes())
            .and_then(|()| file.sync_all())
            .map_err(|e| e.to_string())?;
        // link is atomic and fails if target already exists, including symlinks.
        fs::hard_link(&temporary, &target).map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                format!("rice {name} already exists; choose a new name or edit its TOML file")
            } else {
                e.to_string()
            }
        })?;
        Ok(())
    })();
    let _ = fs::remove_file(&temporary);
    result?;
    Ok(target)
}
impl Ember {
    pub fn select_rice(&mut self, name: &str) -> Result<(), String> {
        let next = if name == "default" {
            let startup = Config::load(&self.config_path)?;
            let mut result = overlay(&self.config, appearance(&startup)?)?;
            result.theme.preset = startup.theme.preset;
            result.rice = startup.rice;
            result
        } else {
            load(&self.config, &self.config_path, name)?
        };
        validate_resources(&next, &self.config_path)?;
        self.config = next;
        self.theme_generation += 1;
        self.relayout();
        crate::memory::request_reclaim();
        Ok(())
    }
    pub fn cycle_rice(&mut self) -> Result<(), String> {
        let names = names(&self.config_path);
        let current = if self.config.rice.profile.is_empty() {
            "red-dusk"
        } else {
            &self.config.rice.profile
        };
        let index = names
            .iter()
            .position(|name| name == current)
            .map_or(0, |i| (i + 1) % names.len());
        let mut first_error = None;
        for offset in 0..names.len() {
            match self.select_rice(&names[(index + offset) % names.len()]) {
                Ok(()) => return Ok(()),
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        Err(first_error.unwrap_or_else(|| "no rice profiles available".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn complete_profiles_are_deterministic_and_preserve_controls() {
        let mut config = Config::default();
        config.audio.enabled = false;
        config.notifications.do_not_disturb = true;
        config.launcher.enabled = false;
        config.layout.workspaces = 3;
        config.layout.default_floating = true;
        config.terminal = vec!["/bin/true".into()];
        config.keyboard.modifier = "Super".into();
        let path = Path::new("/tmp/ember-rice-unit/config.toml");
        let midnight = load(&config, path, "midnight").unwrap();
        let minimal = load(&midnight, path, "minimal").unwrap();
        let back = load(&minimal, path, "midnight").unwrap();
        assert_eq!(appearance(&midnight).unwrap(), appearance(&back).unwrap());
        assert_eq!(back.layout.workspaces, 3);
        assert!(back.layout.default_floating);
        assert!(!back.audio.enabled && !back.launcher.enabled && back.notifications.do_not_disturb);
        assert_eq!(back.keyboard.modifier, "Super");
        assert_eq!(back.terminal, config.terminal);
        assert_eq!(
            toml::Value::try_from(&back.bindings).unwrap(),
            toml::Value::try_from(&config.bindings).unwrap()
        );
        assert!(
            minimal.panels.items.is_empty()
                && !minimal.panels.enabled
                && minimal.theme.wallpaper.is_empty()
        );
    }
    #[test]
    fn profile_names_cannot_escape_the_profile_directory() {
        for name in [
            "../escape",
            "/tmp/evil",
            "a/b",
            "a.toml",
            "",
            "save",
            "default",
            "has space",
        ] {
            assert!(!valid_name(name), "{name}");
        }
        assert!(valid_name("my-red_2"));
    }
    #[test]
    fn snapshot_roundtrip_is_private_and_refuses_overwrite() {
        use std::os::unix::fs::PermissionsExt;
        let nonce = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let dir =
            std::env::temp_dir().join(format!("ember-rice-unit-{}-{nonce}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        let mut config = Config::default();
        config.panels.enabled = true;
        config.panels.items.push(crate::panels::Item {
            name: "clock".into(),
            modules: vec!["clock".into()],
            ..Default::default()
        });
        config.theme.wallpaper = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/red-dusk.png")
            .to_string_lossy()
            .into_owned();
        let saved = save(&config, &path, "mine").unwrap();
        let text = fs::read_to_string(&saved).unwrap();
        assert!(
            !text.contains("terminal =")
                && !text.contains("[audio]")
                && !text.contains("bindings")
                && !text.contains("[rice]")
        );
        assert_eq!(
            fs::metadata(&saved).unwrap().permissions().mode() & 0o777,
            0o600
        );
        assert_eq!(
            appearance(&load(&Config::default(), &path, "mine").unwrap()).unwrap(),
            appearance(&config).unwrap()
        );
        assert!(save(&config, &path, "mine")
            .unwrap_err()
            .contains("already exists"));
        assert_eq!(fs::read_to_string(saved).unwrap(), text);
        assert_eq!(fs::read_dir(dir.join("rices")).unwrap().count(), 1);
        fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn startup_rice_wins_over_appearance_and_does_not_recurse() {
        let path =
            std::env::temp_dir().join(format!("ember-rice-startup-{}.toml", std::process::id()));
        fs::write(&path,"[rice]\nprofile='minimal'\n[theme]\npreset='this-theme-is-ignored'\nbackground='not-a-colour'\n[panels]\nenabled=true\n[audio]\nenabled=false\n").unwrap();
        let config = Config::load(&path).unwrap();
        assert_eq!(config.theme.background, "#101010");
        assert!(config.theme.preset.is_empty() && !config.panels.enabled && !config.audio.enabled);
        fs::remove_file(path).unwrap();
    }
}
