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
    link_active: bool,
    ipv4_address: Option<String>,
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

    if !wifi_connected(
        state.network.as_deref(),
        state.link_active,
        state.ipv4_address.as_deref(),
    ) {
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
    let interface = command_stdout("ifconfig", [&device]).unwrap_or_default();
    let link_active = parse_ifconfig_link_active(&interface);
    let ipconfig_address = command_stdout("ipconfig", ["getifaddr", &device])
        .ok()
        .and_then(|output| parse_ipconfig_ipv4_address(&output));
    let ipv4_address = parse_ifconfig_ipv4_address(&interface).or(ipconfig_address);
    let power_on = command_stdout("networksetup", ["-getairportpower", &device])
        .map(|output| parse_airport_power(&output))
        .unwrap_or(link_active)
        || link_active;
    let network = command_stdout("networksetup", ["-getairportnetwork", &device])
        .ok()
        .and_then(|output| parse_airport_network(&output));

    Ok(MacWifiState {
        device,
        power_on,
        network,
        link_active,
        ipv4_address,
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

#[cfg(any(target_os = "macos", test))]
fn parse_ifconfig_link_active(output: &str) -> bool {
    output
        .lines()
        .map(str::trim)
        .any(|line| line.eq_ignore_ascii_case("status: active"))
}

#[cfg(any(target_os = "macos", test))]
fn parse_ifconfig_ipv4_address(output: &str) -> Option<String> {
    output.lines().map(str::trim).find_map(|line| {
        line.strip_prefix("inet ")
            .and_then(|rest| rest.split_whitespace().next())
            .and_then(usable_ipv4_address)
    })
}

#[cfg(any(target_os = "macos", test))]
fn parse_ipconfig_ipv4_address(output: &str) -> Option<String> {
    output.lines().map(str::trim).find_map(usable_ipv4_address)
}

#[cfg(any(target_os = "macos", test))]
fn usable_ipv4_address(address: &str) -> Option<String> {
    let address = address.trim();

    if is_usable_ipv4_address(address) {
        Some(address.to_string())
    } else {
        None
    }
}

#[cfg(any(target_os = "macos", test))]
fn is_usable_ipv4_address(address: &str) -> bool {
    let mut parts = address.split('.');
    let Some(first) = parts.next().and_then(|part| part.parse::<u8>().ok()) else {
        return false;
    };
    let Some(second) = parts.next().and_then(|part| part.parse::<u8>().ok()) else {
        return false;
    };
    let Some(_third) = parts.next().and_then(|part| part.parse::<u8>().ok()) else {
        return false;
    };
    let Some(_fourth) = parts.next().and_then(|part| part.parse::<u8>().ok()) else {
        return false;
    };

    if parts.next().is_some() {
        return false;
    }

    first != 0 && first != 127 && first < 224 && !(first == 169 && second == 254)
}

#[cfg(any(target_os = "macos", test))]
fn wifi_connected(network: Option<&str>, link_active: bool, ipv4_address: Option<&str>) -> bool {
    network.is_some() || (link_active && ipv4_address.map(is_usable_ipv4_address).unwrap_or(false))
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

    #[test]
    fn parses_active_ifconfig_wifi_link() {
        let output = r#"
en0: flags=8863<UP,BROADCAST,SMART,RUNNING,SIMPLEX,MULTICAST> mtu 1500
	inet6 fe80::8f2:48e4:ce41:f191%en0 prefixlen 64 secured scopeid 0xe
	inet 192.168.68.59 netmask 0xfffffc00 broadcast 192.168.71.255
	status: active
"#;

        assert!(parse_ifconfig_link_active(output));
        assert_eq!(
            parse_ifconfig_ipv4_address(output).as_deref(),
            Some("192.168.68.59")
        );
    }

    #[test]
    fn ignores_self_assigned_ifconfig_ipv4_address() {
        let output = r#"
en0: flags=8863<UP,BROADCAST,SMART,RUNNING,SIMPLEX,MULTICAST> mtu 1500
	inet 169.254.7.11 netmask 0xffff0000 broadcast 169.254.255.255
	status: active
"#;

        assert_eq!(parse_ifconfig_ipv4_address(output), None);
    }

    #[test]
    fn parses_ipconfig_ipv4_address() {
        assert_eq!(
            parse_ipconfig_ipv4_address("192.168.68.59\n").as_deref(),
            Some("192.168.68.59")
        );
        assert_eq!(parse_ipconfig_ipv4_address("169.254.7.11\n"), None);
    }

    #[test]
    fn treats_active_wifi_with_usable_ipv4_as_connected_without_ssid() {
        assert!(wifi_connected(None, true, Some("192.168.68.59")));
    }

    #[test]
    fn treats_inactive_wifi_without_ssid_as_disconnected() {
        assert!(!wifi_connected(None, false, None));
        assert!(!wifi_connected(None, true, Some("169.254.7.11")));
        assert!(wifi_connected(Some("Synonym"), false, None));
    }
}
