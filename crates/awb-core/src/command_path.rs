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
    let path_var = env::var_os("PATH").context("PATH is not set")?;

    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if is_executable_file(&candidate) {
            return Ok(candidate);
        }
    }

    bail!("{name} not found on PATH")
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
