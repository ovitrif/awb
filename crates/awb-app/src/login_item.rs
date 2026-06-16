//! "Open at Login" backed by macOS Login Items (System Events). Registering
//! the app here makes it appear under System Settings → General → Login Items.
//!
//! The first add/remove (or query) prompts the user to allow controlling
//! System Events under Privacy & Security → Automation.

#[cfg(target_os = "macos")]
mod imp {
    use std::process::Command;

    /// Display name for the login item AWB registers.
    const ITEM_NAME: &str = "awb";

    /// The enclosing `.app` bundle when present, otherwise the bare executable.
    /// Login Items shows whichever path is registered.
    fn app_path() -> Option<String> {
        let exe = std::env::current_exe().ok()?;
        let exe = exe.to_string_lossy().into_owned();

        match exe.find(".app/Contents/MacOS/") {
            Some(index) => Some(exe[..index + ".app".len()].to_string()),
            None => Some(exe),
        }
    }

    fn osascript(script: &str) -> Option<String> {
        let output = Command::new("osascript")
            .arg("-e")
            .arg(script)
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        Some(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    pub fn is_enabled() -> bool {
        let Some(path) = app_path() else {
            return false;
        };
        // `get the path of every login item` returns a comma-separated list.
        osascript("tell application \"System Events\" to get the path of every login item")
            .is_some_and(|paths| paths.split(',').any(|entry| entry.trim() == path))
    }

    pub fn set_enabled(enabled: bool) {
        let Some(path) = app_path() else {
            return;
        };

        // Remove only AWB's own entry, matched by the exact registered path, so
        // toggling never deletes an unrelated login item.
        let _ = osascript(&format!(
            "tell application \"System Events\" to delete (every login item whose path is \"{path}\")"
        ));

        if enabled {
            let _ = osascript(&format!(
                "tell application \"System Events\" to make login item at end with properties {{name:\"{ITEM_NAME}\", path:\"{path}\", hidden:true}}"
            ));
        }
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    pub fn is_enabled() -> bool {
        false
    }

    pub fn set_enabled(_enabled: bool) {}
}

pub use imp::{is_enabled, set_enabled};
