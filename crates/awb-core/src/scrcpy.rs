use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::command_path::resolve_program;

pub const DEFAULT_WINDOW_WIDTH: u32 = 480;
pub const DEFAULT_WINDOW_HEIGHT: u32 = 1_071;

#[derive(Debug, Clone)]
pub struct Scrcpy {
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrcpyRunMode {
    Background,
    Foreground,
}

impl Scrcpy {
    pub fn resolve(override_path: Option<PathBuf>, skip_check: bool) -> Result<Self> {
        let path = if skip_check {
            override_path.unwrap_or_else(|| PathBuf::from("scrcpy"))
        } else {
            resolve_program("scrcpy", override_path)?
        };

        Ok(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn launch(&self, serial: &str, options: &ScrcpyOptions) -> Result<()> {
        let status = Command::new(&self.path)
            .args(default_args(serial, options))
            .status()
            .with_context(|| format!("failed to run {}", self.path.display()))?;

        if status.success() {
            return Ok(());
        }

        bail!("scrcpy exited with status {status}");
    }

    pub fn launch_background(&self, serial: &str, options: &ScrcpyOptions) -> Result<u32> {
        let child = self.spawn(serial, options, ScrcpyRunMode::Background)?;
        Ok(child.id())
    }

    pub fn spawn(
        &self,
        serial: &str,
        options: &ScrcpyOptions,
        mode: ScrcpyRunMode,
    ) -> Result<Child> {
        let mut command = Command::new(&self.path);
        command.args(default_args(serial, options));

        if mode == ScrcpyRunMode::Background {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null());
        }

        let child = command
            .spawn()
            .with_context(|| format!("failed to run {}", self.path.display()))?;
        Ok(child)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrcpyOptions {
    pub no_audio: bool,
    pub stay_awake: bool,
    pub borderless: bool,
    pub always_on_top: bool,
    pub window_title: String,
    pub window_width: u32,
    pub window_height: u32,
}

impl Default for ScrcpyOptions {
    fn default() -> Self {
        Self {
            no_audio: true,
            stay_awake: true,
            borderless: true,
            always_on_top: false,
            window_title: "Pixel 10 Pro".to_string(),
            window_width: DEFAULT_WINDOW_WIDTH,
            window_height: DEFAULT_WINDOW_HEIGHT,
        }
    }
}

pub fn default_args(serial: &str, options: &ScrcpyOptions) -> Vec<OsString> {
    let mut args = vec![OsString::from("-s"), OsString::from(serial)];

    if options.no_audio {
        args.push(OsString::from("--no-audio"));
    }

    if options.stay_awake {
        args.push(OsString::from("--stay-awake"));
    }

    if options.borderless {
        args.push(OsString::from("--window-borderless"));
    }

    if options.always_on_top {
        args.push(OsString::from("--always-on-top"));
    }

    if !options.window_title.is_empty() {
        args.push(OsString::from("--window-title"));
        args.push(OsString::from(&options.window_title));
    }

    if options.window_width > 0 {
        args.push(OsString::from("--window-width"));
        args.push(OsString::from(options.window_width.to_string()));
    }

    if options.window_height > 0 {
        args.push(OsString::from("--window-height"));
        args.push(OsString::from(options.window_height.to_string()));
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_raycast_equivalent_scrcpy_args() {
        let args = default_args("192.168.1.23:40233", &ScrcpyOptions::default());
        let args: Vec<_> = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert_eq!(
            args,
            vec![
                "-s",
                "192.168.1.23:40233",
                "--no-audio",
                "--stay-awake",
                "--window-borderless",
                "--window-title",
                "Pixel 10 Pro",
                "--window-width",
                "480",
                "--window-height",
                "1071",
            ]
        );
    }

    #[test]
    fn builds_plain_window_scrcpy_args() {
        let options = ScrcpyOptions {
            borderless: false,
            always_on_top: true,
            window_title: "Ovi Pixel".to_string(),
            ..ScrcpyOptions::default()
        };
        let args = default_args("device", &options);
        let args: Vec<_> = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert_eq!(
            args,
            vec![
                "-s",
                "device",
                "--no-audio",
                "--stay-awake",
                "--always-on-top",
                "--window-title",
                "Ovi Pixel",
                "--window-width",
                "480",
                "--window-height",
                "1071",
            ]
        );
    }

    #[test]
    fn zero_window_values_leave_scrcpy_window_defaults() {
        let options = ScrcpyOptions {
            window_width: 0,
            window_height: 0,
            ..ScrcpyOptions::default()
        };
        let args = default_args("device", &options);
        let args: Vec<_> = args
            .iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert!(!args.contains(&"--window-width".to_string()));
        assert!(!args.contains(&"--window-height".to_string()));
    }
}
