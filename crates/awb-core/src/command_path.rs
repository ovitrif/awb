use std::env;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};

pub fn resolve_program(name: &str, override_path: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = override_path {
        if is_executable_file(&path) {
            return Ok(path);
        }

        bail!(
            "{} was not found or is not executable at {}",
            name,
            path.display()
        );
    }

    find_on_path(name).with_context(|| {
        format!("{name} was not found. Install it and make sure it is available on PATH.")
    })
}

fn find_on_path(name: &str) -> Result<PathBuf> {
    if let Some(path_var) = env::var_os("PATH") {
        for dir in env::split_paths(&path_var) {
            let candidate = dir.join(name);
            if is_executable_file(&candidate) {
                return Ok(candidate);
            }
        }
    }

    // GUI launches (Finder, Open-at-Login) don't inherit the shell PATH, so the
    // menu bar app would otherwise miss a Homebrew or Android SDK adb/scrcpy.
    for dir in standard_dirs() {
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            return Ok(candidate);
        }
    }

    bail!("{name} not found on PATH")
}

/// Common macOS install locations for adb and scrcpy, probed when PATH is the
/// minimal launchd environment rather than the user's shell.
fn standard_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![
        PathBuf::from("/opt/homebrew/bin"),
        PathBuf::from("/usr/local/bin"),
        PathBuf::from("/usr/bin"),
    ];

    if let Some(home) = env::var_os("HOME") {
        dirs.push(PathBuf::from(home).join("Library/Android/sdk/platform-tools"));
    }

    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Some(sdk) = env::var_os(var) {
            dirs.push(PathBuf::from(sdk).join("platform-tools"));
        }
    }

    dirs
}

fn is_executable_file(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        path.metadata()
            .map(|metadata| metadata.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    }

    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn standard_dirs_include_common_macos_locations() {
        let dirs = standard_dirs();
        assert!(dirs.contains(&PathBuf::from("/opt/homebrew/bin")));
        assert!(dirs.contains(&PathBuf::from("/usr/local/bin")));
    }
}
