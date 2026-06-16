use std::net::{Ipv4Addr, Ipv6Addr};
use std::time::Duration;

use crate::adb::{self, Adb};
use crate::dnssd;

#[derive(Debug, Default)]
pub struct PairingEndpointDiscovery {
    pub endpoints: Vec<String>,
    pub adb_error: Option<String>,
    pub bonjour_error: Option<String>,
}

pub fn discover_pairing_endpoint_candidates(
    adb: &Adb,
    instance: &str,
    bonjour_timeout: Duration,
) -> PairingEndpointDiscovery {
    let mut discovery = PairingEndpointDiscovery::default();

    match adb.mdns_services() {
        Ok(services) => {
            for service in services
                .into_iter()
                .filter(|service| service.instance == instance && service.is_pairing_service())
            {
                push_endpoint(&mut discovery.endpoints, service.address);
            }
        }
        Err(error) => discovery.adb_error = Some(format!("{error:#}")),
    }

    match dnssd::discover_pairing_endpoint(instance, bonjour_timeout) {
        Ok(Some(endpoint)) => push_endpoint(&mut discovery.endpoints, endpoint),
        Ok(None) => {}
        Err(error) => discovery.bonjour_error = Some(format!("{error:#}")),
    }

    order_endpoints(&mut discovery.endpoints);
    discovery
}

fn push_endpoint(endpoints: &mut Vec<String>, endpoint: String) {
    let endpoint = endpoint.trim();

    if endpoint.is_empty() || endpoints.iter().any(|existing| existing == endpoint) {
        return;
    }

    endpoints.push(endpoint.to_string());
}

fn order_endpoints(endpoints: &mut [String]) {
    endpoints.sort_by_key(|endpoint| endpoint_preference(endpoint));
}

fn endpoint_preference(endpoint: &str) -> u8 {
    let host = adb::endpoint_host(endpoint)
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_string();

    if let Ok(ip) = host.parse::<Ipv4Addr>() {
        return if ip.is_link_local() || ip.is_loopback() {
            2
        } else {
            0
        };
    }

    if host.parse::<Ipv6Addr>().is_ok() {
        return 3;
    }

    1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_routable_ipv4_pairing_endpoints() {
        let mut endpoints = vec![
            "[fe80::1234]:37000".to_string(),
            "Android.local:37000".to_string(),
            "192.168.0.42:37000".to_string(),
            "169.254.1.42:37000".to_string(),
        ];

        order_endpoints(&mut endpoints);

        assert_eq!(endpoints[0], "192.168.0.42:37000");
        assert_eq!(endpoints[1], "Android.local:37000");
        assert_eq!(endpoints[2], "169.254.1.42:37000");
        assert_eq!(endpoints[3], "[fe80::1234]:37000");
    }
}
