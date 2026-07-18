use std::fs;
use std::path::PathBuf;

use awb_core::scrcpy::ScrcpyOptions;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    #[default]
    Auto,
    Day,
    Night,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub window_title: String,
    pub window_width: u32,
    pub window_height: u32,
    pub always_on_top: bool,
    /// Launch scrcpy as a borderless (plain) window with no title bar.
    pub borderless: bool,
    /// Auto-start mirroring when a physical phone connects (never emulators).
    pub auto_mirror: bool,
    /// Follow the system appearance or force the light/dark app palette.
    pub theme: ThemeMode,
}

impl Default for Settings {
    fn default() -> Self {
        let defaults = ScrcpyOptions::default();

        Self {
            window_title: defaults.window_title,
            window_width: defaults.window_width,
            window_height: defaults.window_height,
            always_on_top: true,
            borderless: true,
            auto_mirror: false,
            theme: ThemeMode::Auto,
        }
    }
}

impl Settings {
    pub fn scrcpy_options(&self) -> ScrcpyOptions {
        ScrcpyOptions {
            borderless: self.borderless,
            always_on_top: self.always_on_top,
            window_title: self.window_title.clone(),
            window_width: self.window_width,
            window_height: self.window_height,
            ..ScrcpyOptions::default()
        }
    }

    pub fn load() -> Self {
        config_path()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|raw| toml::from_str(&raw).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) {
        let Some(path) = config_path() else {
            return;
        };

        if let Some(dir) = path.parent() {
            let _ = fs::create_dir_all(dir);
        }

        if let Ok(raw) = toml::to_string_pretty(self) {
            let _ = fs::write(path, raw);
        }
    }
}

fn config_path() -> Option<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;

    Some(base.join("awb").join("config.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_automatic_theme() {
        assert_eq!(Settings::default().theme, ThemeMode::Auto);
    }

    #[test]
    fn old_configs_without_theme_migrate_to_auto() {
        let settings: Settings = toml::from_str(
            r#"
window_title = "Phone"
window_width = 480
window_height = 1071
always_on_top = true
borderless = true
auto_mirror = false
"#,
        )
        .expect("legacy settings should deserialize");

        assert_eq!(settings.theme, ThemeMode::Auto);
    }

    #[test]
    fn theme_modes_use_stable_lowercase_values() {
        let settings = Settings {
            theme: ThemeMode::Day,
            ..Settings::default()
        };

        let raw = toml::to_string(&settings).expect("settings serialize");
        assert!(raw.contains("theme = \"day\""));
    }
}
