//! "Open at Login" backed by macOS Login Items (System Events). Registering
//! the app here makes it appear under System Settings → General → Login Items.
//!
//! The first add/remove (or query) prompts the user to allow controlling
//! System Events under Privacy & Security → Automation.

#[cfg(target_os = "macos")]
mod imp {
    use std::process::Command;

    /// Matched case-insensitively against login item names, so it also matches
    /// `AWB` (the bundle) and `awb-app` (the bare binary).
    const MATCH: &str = "awb";

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
        osascript("tell application \"System Events\" to get the name of every login item")
            .map(|names| names.to_lowercase().contains(MATCH))
            .unwrap_or(false)
    }

    pub fn set_enabled(enabled: bool) {
        // Always clear stale entries first so we never stack duplicates.
        let _ = osascript(&format!(
            "tell application \"System Events\" to delete (every login item whose name contains \"{MATCH}\")"
        ));

        if enabled && let Some(path) = app_path() {
            let _ = osascript(&format!(
                "tell application \"System Events\" to make login item at end with properties {{name:\"{MATCH}\", path:\"{path}\", hidden:true}}"
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
