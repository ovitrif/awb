use std::fs;
use std::path::PathBuf;

use awb_core::scrcpy::ScrcpyOptions;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub window_title: String,
    pub window_width: u32,
    pub window_height: u32,
    pub always_on_top: bool,
    pub plain_window: bool,
    /// Auto-start mirroring when a physical phone connects (never emulators).
    pub auto_mirror: bool,
}

impl Default for Settings {
    fn default() -> Self {
        let defaults = ScrcpyOptions::default();

        Self {
            window_title: defaults.window_title,
            window_width: defaults.window_width,
            window_height: defaults.window_height,
            always_on_top: true,
            plain_window: false,
            auto_mirror: false,
        }
    }
}

impl Settings {
    pub fn scrcpy_options(&self) -> ScrcpyOptions {
        ScrcpyOptions {
            borderless: !self.plain_window,
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
