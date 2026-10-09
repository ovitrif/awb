use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::command_path::resolve_program;

const EMULATOR_LIST_TIMEOUT: Duration = Duration::from_secs(5);
const AVD_REMOVE_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Debug, Clone)]
pub struct Emulator {
    path: PathBuf,
}

impl Emulator {
    pub fn resolve(override_path: Option<PathBuf>) -> Result<Self> {
        Ok(Self {
            path: resolve_program("emulator", override_path)?,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn list_avds(&self) -> Result<Vec<String>> {
        self.list_avds_with_timeout(EMULATOR_LIST_TIMEOUT)
    }

    fn list_avds_with_timeout(&self, timeout: Duration) -> Result<Vec<String>> {
        let mut command = Command::new(&self.path);
        command
            .arg("-list-avds")
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let output = output_with_timeout(command, timeout, "emulator -list-avds")?;

        if !output.status.success() {
            bail!(
                "emulator -list-avds failed: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }

        Ok(parse_avds(&String::from_utf8_lossy(&output.stdout)))
    }

    pub fn launch(&self, name: &str) -> Result<Child> {
        self.launch_command(name)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("failed to launch AVD {name} with {}", self.path.display()))
    }

    fn launch_command(&self, name: &str) -> Command {
        let mut command = Command::new(&self.path);
        command.args(["-avd", name]);
        command
    }
}

pub fn remove_avd(name: &str) -> Result<()> {
    let avdmanager = resolve_program("avdmanager", None)?;
    remove_avd_with_program(&avdmanager, name)
}

fn remove_avd_with_program(avdmanager: &Path, name: &str) -> Result<()> {
    let mut command = Command::new(avdmanager);
    // avdmanager is a Java tool. An app launched from Finder or at login gets
    // launchd's bare environment, so point it at Android Studio's bundled
    // runtime when no JAVA_HOME is set and that runtime exists.
    if std::env::var_os("JAVA_HOME").is_none()
        && let Some(java_home) = bundled_java_home()
    {
        command.env("JAVA_HOME", java_home);
    }
    command
        .args(["delete", "avd", "--name", name])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = output_with_timeout(command, AVD_REMOVE_TIMEOUT, "avdmanager delete avd")?;
    if !output.status.success() {
        bail!(
            "could not delete AVD {name}: {}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    Ok(())
}

/// Android Studio's bundled Java runtime, in `/Applications` or
/// `~/Applications`.
fn bundled_java_home() -> Option<PathBuf> {
    let suffix = "Android Studio.app/Contents/jbr/Contents/Home";
    let mut roots = vec![PathBuf::from("/Applications")];
    if let Some(home) = std::env::var_os("HOME") {
        roots.push(PathBuf::from(home).join("Applications"));
    }
    roots
        .into_iter()
        .map(|root| root.join(suffix))
        .find(|java_home| java_home.join("bin/java").exists())
}

fn output_with_timeout(mut command: Command, timeout: Duration, action: &str) -> Result<Output> {
    let program = command.get_program().to_string_lossy().into_owned();
    let mut child = command
        .spawn()
        .with_context(|| format!("failed to run {program}"))?;
    let deadline = Instant::now() + timeout;

    loop {
        if child
            .try_wait()
            .with_context(|| format!("failed to poll {program}"))?
            .is_some()
        {
            return child
                .wait_with_output()
                .with_context(|| format!("failed to read {program} output"));
        }

        if Instant::now() >= deadline {
            let _ = child.kill();
            child
                .wait()
                .with_context(|| format!("failed to stop timed-out {program}"))?;
            bail!("{action} timed out after {} seconds", timeout.as_secs_f32());
        }

        thread::sleep(Duration::from_millis(25));
    }
}

fn parse_avds(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(ToString::to_string)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn removes_only_the_named_avd_and_reports_cli_errors() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let script = std::env::temp_dir().join(format!(
            "awb-avdmanager-remove-{}-{unique}.sh",
            std::process::id()
        ));
        fs::write(&script, "#!/bin/sh\n[ \"$#\" = 4 ] && [ \"$1\" = delete ] && [ \"$2\" = avd ] && [ \"$3\" = --name ] || exit 9\n[ \"$4\" = 'test avd' ] && exit 0\necho 'AVD is busy' >&2\nexit 1\n").unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let success = remove_avd_with_program(&script, "test avd");
        let failure = remove_avd_with_program(&script, "busy");
        fs::remove_file(script).unwrap();

        success.unwrap();
        let error = format!("{:#}", failure.unwrap_err());
        assert!(error.contains("could not delete AVD busy"));
        assert!(error.contains("AVD is busy"));
    }

    #[test]
    fn parses_avd_names() {
        assert_eq!(
            parse_avds("Pixel_10_Pro_XL\n\n Pixel_9a \nmedium_phone\n"),
            vec!["Pixel_10_Pro_XL", "Pixel_9a", "medium_phone"]
        );
    }

    #[test]
    fn builds_avd_launch_command() {
        let emulator = Emulator {
            path: PathBuf::from("/sdk/emulator/emulator"),
        };
        let command = emulator.launch_command("Pixel_9a");
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert_eq!(command.get_program(), "/sdk/emulator/emulator");
        assert_eq!(args, ["-avd", "Pixel_9a"]);
    }

    #[cfg(unix)]
    #[test]
    fn avd_listing_times_out() {
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::time::{SystemTime, UNIX_EPOCH};

        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let script = std::env::temp_dir().join(format!(
            "awb-emulator-timeout-{}-{unique}.sh",
            std::process::id()
        ));
        fs::write(&script, "#!/bin/sh\nwhile :; do :; done\n").unwrap();
        let mut permissions = fs::metadata(&script).unwrap().permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(&script, permissions).unwrap();

        let emulator = Emulator {
            path: script.clone(),
        };
        let error = emulator
            .list_avds_with_timeout(Duration::from_millis(50))
            .unwrap_err();
        fs::remove_file(script).unwrap();

        assert!(format!("{error:#}").contains("timed out"));
    }
}
