use std::io::Read;
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};

const PAIRING_SERVICE_TYPE: &str = "_adb-tls-pairing._tcp";
const CONNECT_SERVICE_TYPE: &str = "_adb-tls-connect._tcp";
const LOCAL_DOMAIN: &str = "local";

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedService {
    host: String,
    port: u16,
}

pub fn discover_pairing_endpoint(instance: &str, timeout: Duration) -> Result<Option<String>> {
    let Some(service) = resolve_instance(instance, PAIRING_SERVICE_TYPE, timeout)? else {
        return Ok(None);
    };

    let host = resolve_host_to_ipv4(&service.host, timeout)?
        .unwrap_or_else(|| normalize_dns_name(&service.host));

    Ok(Some(format!("{host}:{}", service.port)))
}

pub fn discover_connect_endpoints(pairing_address: &str, timeout: Duration) -> Result<Vec<String>> {
    let browse_timeout = timeout.min(Duration::from_secs(4));
    let resolve_timeout = timeout.min(Duration::from_secs(3));
    let instances = browse_connect_instances(browse_timeout)?;
    let pairing_host = endpoint_host(pairing_address);
    let mut endpoints = Vec::new();

    for instance in instances {
        let Some(service) = resolve_instance(&instance, CONNECT_SERVICE_TYPE, resolve_timeout)?
        else {
            continue;
        };

        let host = resolve_host_to_ipv4(&service.host, resolve_timeout)?
            .unwrap_or_else(|| normalize_dns_name(&service.host));
        let endpoint = format!("{host}:{}", service.port);

        if !endpoints.contains(&endpoint) {
            endpoints.push(endpoint);
        }
    }

    endpoints.sort_by_key(|endpoint| endpoint_host(endpoint) != pairing_host);
    Ok(endpoints)
}

fn browse_connect_instances(timeout: Duration) -> Result<Vec<String>> {
    let output = run_dns_sd_for(["-B", CONNECT_SERVICE_TYPE, LOCAL_DOMAIN], timeout)?;
    Ok(parse_browse_instances(&output))
}

fn resolve_instance(
    instance: &str,
    service_type: &str,
    timeout: Duration,
) -> Result<Option<ResolvedService>> {
    let output = run_dns_sd_for(["-L", instance, service_type, LOCAL_DOMAIN], timeout)?;
    Ok(parse_resolved_service(&output))
}

fn resolve_host_to_ipv4(host: &str, timeout: Duration) -> Result<Option<String>> {
    let output = run_dns_sd_for(["-G", "v4", host], timeout)?;
    Ok(parse_ipv4_address(&output))
}

fn run_dns_sd_for<const N: usize>(args: [&str; N], timeout: Duration) -> Result<String> {
    let mut child = Command::new("dns-sd")
        .args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to run dns-sd")?;

    let mut stdout = child
        .stdout
        .take()
        .context("failed to capture dns-sd stdout")?;

    let output_reader = thread::spawn(move || {
        let mut output = String::new();
        let _ = stdout.read_to_string(&mut output);
        output
    });

    thread::sleep(timeout);

    if child.try_wait()?.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }

    Ok(output_reader.join().unwrap_or_default())
}

fn parse_browse_instances(output: &str) -> Vec<String> {
    let mut instances = Vec::new();

    for line in output.lines() {
        let line = line.trim();

        if !line.contains(" Add ") || !line.contains(CONNECT_SERVICE_TYPE) {
            continue;
        }

        let Some((_before, after_type)) = line.split_once(CONNECT_SERVICE_TYPE) else {
            continue;
        };

        let instance = after_type
            .trim_start_matches('.')
            .trim()
            .trim_end_matches('.')
            .trim();

        if !instance.is_empty() && !instances.iter().any(|existing| existing == instance) {
            instances.push(instance.to_string());
        }
    }

    instances
}

fn parse_resolved_service(output: &str) -> Option<ResolvedService> {
    for line in output.lines() {
        let Some((_before, after)) = line.split_once(" can be reached at ") else {
            continue;
        };

        let target = after.split_whitespace().next()?;
        let (host, port) = target.rsplit_once(':')?;
        let port = port.parse::<u16>().ok()?;

        return Some(ResolvedService {
            host: normalize_dns_name(host),
            port,
        });
    }

    None
}

fn parse_ipv4_address(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find(|token| is_ipv4_address(token))
        .map(ToString::to_string)
}

fn normalize_dns_name(name: &str) -> String {
    name.trim()
        .trim_start_matches('[')
        .trim_end_matches(']')
        .trim_end_matches('.')
        .to_string()
}

fn endpoint_host(endpoint: &str) -> String {
    endpoint
        .rsplit_once(':')
        .map(|(host, _port)| normalize_dns_name(host))
        .unwrap_or_else(|| endpoint.to_string())
}

fn is_ipv4_address(value: &str) -> bool {
    let mut parts = value.split('.');

    if parts.clone().count() != 4 {
        return false;
    }

    parts.all(|part| part.parse::<u8>().is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_browse_instances() {
        let output = r#"
Browsing for _adb-tls-connect._tcp.local
DATE: ---Wed 06 May 2026---
10:44:01.123  Add        2  4 local. _adb-tls-connect._tcp. adb-5C020DLCH0007Q-tfPgZw
10:44:02.123  Rmv        2  4 local. _adb-tls-connect._tcp. adb-old
10:44:03.123  Add        2  4 local. _other._tcp. ignored
"#;

        assert_eq!(
            parse_browse_instances(output),
            vec!["adb-5C020DLCH0007Q-tfPgZw"]
        );
    }

    #[test]
    fn parses_resolved_service() {
        let output = r#"
Lookup adb-5C020DLCH0007Q-tfPgZw._adb-tls-connect._tcp.local
10:44:01.123  adb-5C020DLCH0007Q-tfPgZw._adb-tls-connect._tcp.local. can be reached at pixel.local.:37197 (interface 4)
"#;

        assert_eq!(
            parse_resolved_service(output),
            Some(ResolvedService {
                host: "pixel.local".to_string(),
                port: 37197
            })
        );
    }

    #[test]
    fn parses_ipv4_address() {
        let output = r#"
DATE: ---Wed 06 May 2026---
10:44:01.123  Add     2  4 pixel.local. 192.168.68.54  120
"#;

        assert_eq!(parse_ipv4_address(output).as_deref(), Some("192.168.68.54"));
    }
}
