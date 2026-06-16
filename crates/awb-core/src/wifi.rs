use anyhow::Result;
#[cfg(target_os = "macos")]
use anyhow::{Context, bail};
#[cfg(target_os = "macos")]
use std::process::Command;

#[cfg(target_os = "macos")]
#[derive(Debug, Clone, PartialEq, Eq)]
struct MacWifiState {
    device: String,
    power_on: bool,
    network: Option<String>,
}

pub fn ensure_pairing_wifi_ready() -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        ensure_macos_pairing_wifi_ready()
    }

    #[cfg(not(target_os = "macos"))]
    {
        Ok(())
    }
}

#[cfg(target_os = "macos")]
fn ensure_macos_pairing_wifi_ready() -> Result<()> {
    let state = macos_wifi_state()?;

    if !state.power_on {
        bail!(
            "Wi-Fi is off on this Mac. QR pairing needs the Mac and phone on the same Wi-Fi network. Turn Wi-Fi on, join the phone's network, then try again."
        );
    }

    if state.network.is_none() {
        bail!(
            "Wi-Fi is not connected on this Mac ({}). QR pairing needs the Mac and phone on the same Wi-Fi network. Join the phone's Wi-Fi network, then try again.",
            state.device
        );
    }

    Ok(())
}

#[cfg(target_os = "macos")]
fn macos_wifi_state() -> Result<MacWifiState> {
    let ports = command_stdout("networksetup", ["-listallhardwareports"])?;
    let device = parse_wifi_device(&ports)
        .context("could not find a Wi-Fi interface on this Mac for QR pairing")?;
    let power = command_stdout("networksetup", ["-getairportpower", &device])?;
    let network = command_stdout("networksetup", ["-getairportnetwork", &device])?;

    Ok(MacWifiState {
        device,
        power_on: parse_airport_power(&power),
        network: parse_airport_network(&network),
    })
}

#[cfg(target_os = "macos")]
fn command_stdout<const N: usize>(program: &str, args: [&str; N]) -> Result<String> {
    let output = Command::new(program)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {program}"))?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();

    if output.status.success() {
        return Ok(stdout);
    }

    let detail = if stderr.is_empty() { stdout } else { stderr };
    bail!("{program} failed: {detail}");
}

#[cfg(any(target_os = "macos", test))]
fn parse_wifi_device(output: &str) -> Option<String> {
    let mut saw_wifi_port = false;

    for line in output.lines().map(str::trim) {
        if let Some(port) = line.strip_prefix("Hardware Port: ") {
            saw_wifi_port = matches!(port, "Wi-Fi" | "AirPort");
            continue;
        }

        if saw_wifi_port {
            if let Some(device) = line.strip_prefix("Device: ") {
                let device = device.trim();
                if !device.is_empty() {
                    return Some(device.to_string());
                }
            }
        }
    }

    None
}

#[cfg(any(target_os = "macos", test))]
fn parse_airport_power(output: &str) -> bool {
    output
        .rsplit_once(':')
        .map(|(_label, value)| value.trim().eq_ignore_ascii_case("on"))
        .unwrap_or(false)
}

#[cfg(any(target_os = "macos", test))]
fn parse_airport_network(output: &str) -> Option<String> {
    let network = output
        .strip_prefix("Current Wi-Fi Network: ")
        .map(str::trim)?;

    if network.is_empty() {
        None
    } else {
        Some(network.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_wifi_device_from_networksetup_ports() {
        let output = r#"
Hardware Port: AX88179A
Device: en11
Ethernet Address: 9c:69:d3:22:c1:a3

Hardware Port: Wi-Fi
Device: en0
Ethernet Address: f4:d4:88:8e:e3:ea
"#;

        assert_eq!(parse_wifi_device(output).as_deref(), Some("en0"));
    }

    #[test]
    fn parses_airport_power() {
        assert!(parse_airport_power("Wi-Fi Power (en0): On"));
        assert!(!parse_airport_power("Wi-Fi Power (en0): Off"));
    }

    #[test]
    fn parses_current_network() {
        assert_eq!(
            parse_airport_network("Current Wi-Fi Network: Synonym").as_deref(),
            Some("Synonym")
        );
        assert_eq!(
            parse_airport_network("You are not associated with an AirPort network.").as_deref(),
            None
        );
    }
}
