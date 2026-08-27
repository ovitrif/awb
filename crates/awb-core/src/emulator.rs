use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};

use crate::command_path::resolve_program;

const EMULATOR_LIST_TIMEOUT: Duration = Duration::from_secs(5);

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
        let output = output_with_timeout(command, timeout)?;

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

fn output_with_timeout(mut command: Command, timeout: Duration) -> Result<Output> {
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
            bail!(
                "emulator -list-avds timed out after {} seconds",
                timeout.as_secs_f32()
            );
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
