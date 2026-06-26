use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};

use anyhow::{Context, Result, bail};

use crate::adb::Adb;
use crate::command_path::{path_env_with_tool_dirs, resolve_program};

pub const DEFAULT_WINDOW_WIDTH: u32 = 480;
pub const DEFAULT_WINDOW_HEIGHT: u32 = 1_071;
const RECOMMENDED_SCRCPY_VERSION: ScrcpyVersion = ScrcpyVersion::new(4, 0, 0);

#[derive(Debug, Clone)]
pub struct Scrcpy {
    path: PathBuf,
    adb: Option<Adb>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrcpyRunMode {
    Background,
    Foreground,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScrcpyDiagnostics {
    pub version_line: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct ScrcpyVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

impl ScrcpyVersion {
    const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }
}

impl std::fmt::Display for ScrcpyVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.patch == 0 {
            write!(f, "{}.{}", self.major, self.minor)
        } else {
            write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
        }
    }
}

impl Scrcpy {
    pub fn resolve(override_path: Option<PathBuf>, skip_check: bool) -> Result<Self> {
        Self::resolve_with_adb(override_path, skip_check, None)
    }

    pub fn resolve_with_adb(
        override_path: Option<PathBuf>,
        skip_check: bool,
        adb: Option<Adb>,
    ) -> Result<Self> {
        let path = if skip_check {
            override_path.unwrap_or_else(|| PathBuf::from("scrcpy"))
        } else {
            resolve_program("scrcpy", override_path)?
        };

        Ok(Self {
            path,
            adb: adb.or_else(|| Adb::resolve(None).ok()),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn version_line(&self) -> Result<String> {
        let output = self
            .metadata_command("--version")
            .output()
            .with_context(|| format!("failed to run {} --version", self.path.display()))?;
        let output = command_output("scrcpy --version", output)?;

        Ok(output
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty())
            .unwrap_or(&output)
            .to_string())
    }

    pub fn diagnostics(&self, options: &ScrcpyOptions) -> ScrcpyDiagnostics {
        let (version_line, version_error) = match self.version_line() {
            Ok(line) => (Some(line), None),
            Err(error) => (None, Some(error)),
        };
        let help = self.help_text();
        let mut warnings = Vec::new();

        match version_line.as_deref().and_then(parse_scrcpy_version) {
            Some(version) if version < RECOMMENDED_SCRCPY_VERSION => warnings.push(format!(
                "scrcpy {version} detected at {}. AWB works best with scrcpy {RECOMMENDED_SCRCPY_VERSION} or newer; run `brew upgrade scrcpy` if mirroring fails.",
                self.path.display()
            )),
            Some(_) => {}
            None => {
                if let Some(error) = version_error {
                    warnings.push(format!(
                        "Could not read scrcpy version from {}: {error:#}. If mirroring fails, run `brew upgrade scrcpy` and try again.",
                        self.path.display()
                    ));
                }
            }
        }

        match help {
            Ok(help) => {
                for flag in compatibility_flags(options) {
                    if !help.contains(flag) {
                        warnings.push(format!(
                            "scrcpy at {} does not advertise `{flag}` in `scrcpy --help`; update it with `brew upgrade scrcpy` or disable the related AWB setting.",
                            self.path.display()
                        ));
                    }
                }
            }
            Err(error) => warnings.push(format!(
                "Could not inspect scrcpy options from {}: {error:#}. If mirroring fails, run `brew upgrade scrcpy` and try again.",
                self.path.display()
            )),
        }

        ScrcpyDiagnostics {
            version_line,
            warnings,
        }
    }

    pub fn launch(&self, serial: &str, options: &ScrcpyOptions) -> Result<()> {
        self.ensure_adb_server()?;

        let status = self
            .command(serial, options)
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
        self.spawn_configured(serial, options, |command| {
            if mode == ScrcpyRunMode::Background {
                command
                    .stdin(Stdio::null())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null());
            }
        })
    }

    pub fn spawn_piped(&self, serial: &str, options: &ScrcpyOptions) -> Result<Child> {
        self.spawn_configured(serial, options, |command| {
            command
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
        })
    }

    fn spawn_configured(
        &self,
        serial: &str,
        options: &ScrcpyOptions,
        configure: impl FnOnce(&mut Command),
    ) -> Result<Child> {
        self.ensure_adb_server()?;

        let mut command = self.command(serial, options);
        configure(&mut command);

        command
            .spawn()
            .with_context(|| format!("failed to run {}", self.path.display()))
    }

    fn command(&self, serial: &str, options: &ScrcpyOptions) -> Command {
        let mut command = Command::new(&self.path);
        command.args(default_args(serial, options));
        self.configure_adb_env(&mut command);
        command
    }

    fn metadata_command(&self, arg: &str) -> Command {
        let mut command = Command::new(&self.path);
        command.arg(arg);
        self.configure_adb_env(&mut command);
        command
    }

    fn configure_adb_env(&self, command: &mut Command) {
        let adb_dir = self
            .adb
            .as_ref()
            .and_then(|adb| adb.path().parent())
            .map(Path::to_path_buf);

        if let Some(adb) = &self.adb {
            command.env("ADB", adb.path());
        }

        if let Some(path) = path_env_with_tool_dirs(adb_dir) {
            command.env("PATH", path);
        }
    }

    fn ensure_adb_server(&self) -> Result<()> {
        let Some(adb) = &self.adb else {
            return Ok(());
        };

        match adb.start_server() {
            Ok(()) => Ok(()),
            Err(start_error) => adb.reset_server().with_context(|| {
                format!("ADB start-server failed ({start_error:#}); reset also failed")
            }),
        }
    }

    fn help_text(&self) -> Result<String> {
        let output = self
            .metadata_command("--help")
            .output()
            .with_context(|| format!("failed to run {} --help", self.path.display()))?;

        command_output("scrcpy --help", output)
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

fn command_output(command: &str, output: std::process::Output) -> Result<String> {
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let text = match (stdout.is_empty(), stderr.is_empty()) {
        (false, true) => stdout,
        (true, false) => stderr,
        (false, false) => format!("{stdout}\n{stderr}"),
        (true, true) => String::new(),
    };

    if output.status.success() {
        Ok(text)
    } else {
        bail!("{command} exited with status {}: {text}", output.status)
    }
}

fn compatibility_flags(options: &ScrcpyOptions) -> Vec<&'static str> {
    let mut flags = vec!["--no-audio", "--stay-awake"];

    if options.borderless {
        flags.push("--window-borderless");
    }

    if options.always_on_top {
        flags.push("--always-on-top");
    }

    if !options.window_title.is_empty() {
        flags.push("--window-title");
    }

    if options.window_width > 0 {
        flags.push("--window-width");
    }

    if options.window_height > 0 {
        flags.push("--window-height");
    }

    flags
}

fn parse_scrcpy_version(line: &str) -> Option<ScrcpyVersion> {
    line.split_whitespace()
        .filter_map(parse_version_token)
        .next()
}

fn parse_version_token(token: &str) -> Option<ScrcpyVersion> {
    let token = token.trim_start_matches('v');
    if !token
        .chars()
        .next()
        .is_some_and(|char| char.is_ascii_digit())
    {
        return None;
    }

    let mut parts = token.split('.');
    let major = parse_number_prefix(parts.next()?)?;
    let minor = parse_number_prefix(parts.next()?)?;
    let patch = parts.next().and_then(parse_number_prefix).unwrap_or(0);

    Some(ScrcpyVersion::new(major, minor, patch))
}

fn parse_number_prefix(value: &str) -> Option<u16> {
    let digits: String = value
        .chars()
        .take_while(|char| char.is_ascii_digit())
        .collect();

    if digits.is_empty() {
        return None;
    }

    digits.parse().ok()
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

    #[test]
    fn command_exports_resolved_adb_to_scrcpy() {
        let adb_path = PathBuf::from("/custom/android/platform-tools/adb");
        let scrcpy = Scrcpy {
            path: PathBuf::from("/usr/local/bin/scrcpy"),
            adb: Some(Adb::from_resolved_path(adb_path.clone())),
        };

        let command = scrcpy.command("device", &ScrcpyOptions::default());
        let envs: Vec<_> = command.get_envs().collect();
        let adb_env = envs
            .iter()
            .find(|(name, _)| *name == "ADB")
            .and_then(|(_, value)| *value)
            .expect("ADB should be set");
        let path_env = envs
            .iter()
            .find(|(name, _)| *name == "PATH")
            .and_then(|(_, value)| *value)
            .expect("PATH should be set");
        let path_dirs: Vec<_> = std::env::split_paths(path_env).collect();

        assert_eq!(adb_env, adb_path.as_os_str());
        assert_eq!(path_dirs.first().map(PathBuf::as_path), adb_path.parent());
    }

    #[test]
    fn parses_scrcpy_versions() {
        assert_eq!(
            parse_scrcpy_version("scrcpy 4.0 <https://github.com/Genymobile/scrcpy>"),
            Some(ScrcpyVersion::new(4, 0, 0))
        );
        assert_eq!(
            parse_scrcpy_version("scrcpy 3.3.4"),
            Some(ScrcpyVersion::new(3, 3, 4))
        );
    }

    #[test]
    fn compatibility_flags_follow_enabled_options() {
        let default_flags = compatibility_flags(&ScrcpyOptions::default());
        assert!(default_flags.contains(&"--window-borderless"));
        assert!(!default_flags.contains(&"--always-on-top"));

        let custom_flags = compatibility_flags(&ScrcpyOptions {
            borderless: false,
            always_on_top: true,
            window_title: String::new(),
            window_width: 0,
            window_height: 0,
            ..ScrcpyOptions::default()
        });

        assert!(!custom_flags.contains(&"--window-borderless"));
        assert!(custom_flags.contains(&"--always-on-top"));
        assert!(!custom_flags.contains(&"--window-title"));
        assert!(!custom_flags.contains(&"--window-width"));
        assert!(!custom_flags.contains(&"--window-height"));
    }
}
