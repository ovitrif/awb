use std::collections::HashSet;
use std::time::{Duration, Instant};

use anyhow::{Result, bail};

use crate::adb::{self, Adb};
use crate::dnssd;
use crate::pairing;
use crate::qr::PairingQr;
use crate::wifi;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConnectedPhone {
    pub serial: String,
    pub display_name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PairingProgressKind {
    WaitingForQrScan,
    CompletingPairing,
    RestartingAdb,
    WaitingForConnectionEndpoint,
    Connecting,
    Verifying,
    ReadingScreen,
}

#[derive(Debug, Clone)]
pub struct PairingProgress {
    pub kind: PairingProgressKind,
    pub title: String,
    pub detail: String,
    pub deadline: Option<Instant>,
    pub attempt: Option<u32>,
    pub endpoint: Option<String>,
}

impl PairingProgress {
    pub fn new(
        kind: PairingProgressKind,
        title: impl Into<String>,
        detail: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            title: title.into(),
            detail: detail.into(),
            deadline: None,
            attempt: None,
            endpoint: None,
        }
    }

    pub fn deadline(mut self, deadline: Instant) -> Self {
        self.deadline = Some(deadline);
        self
    }

    pub fn attempt(mut self, attempt: u32) -> Self {
        self.attempt = Some(attempt);
        self
    }

    pub fn endpoint(mut self, endpoint: impl Into<String>) -> Self {
        self.endpoint = Some(endpoint.into());
        self
    }
}

#[derive(Debug, Clone)]
pub enum PairingEvent {
    Section {
        title: &'static str,
        lines: &'static [&'static str],
    },
    QrReady(PairingQr),
    Progress(PairingProgress),
    Status(String),
    Success(String),
    Warning(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlreadyConnectedChoice {
    Use(usize),
    KeepWaiting,
}

pub trait PairingFlowDelegate {
    fn on_event(&mut self, event: PairingEvent) -> Result<()>;
    fn sleep(&mut self, duration: Duration) -> Result<()>;

    fn choose_already_connected(
        &mut self,
        phones: &[ConnectedPhone],
    ) -> Result<AlreadyConnectedChoice>;

    fn manual_connect_endpoint(&mut self) -> Result<Option<String>>;
}

enum PairingWaitOutcome {
    PairingEndpoints(Vec<String>),
    AlreadyConnected(ConnectedPhone),
}

pub fn pair_and_connect<D>(adb: &Adb, timeout: Duration, delegate: &mut D) -> Result<ConnectedPhone>
where
    D: PairingFlowDelegate,
{
    wifi::ensure_pairing_wifi_ready()?;

    let baseline_services = adb::connect_services(&adb.mdns_services().unwrap_or_default());
    let baseline_devices = adb::ready_device_serials(&adb.devices().unwrap_or_default());
    let qr = PairingQr::generate();

    delegate.on_event(PairingEvent::Section {
        title: "Pair with QR code",
        lines: &[
            "On your Android phone, go to Developer options -> Wireless debugging.",
            "Tap \"Pair device with QR code\".",
            "Scan the QR code below.",
        ],
    })?;
    delegate.on_event(PairingEvent::QrReady(qr.clone()))?;

    let pairing_address = match wait_for_pairing_endpoint(adb, &qr.instance, timeout, delegate)? {
        PairingWaitOutcome::PairingEndpoints(pairing_addresses) => {
            pair_with_pairing_endpoints(adb, &pairing_addresses, &qr.secret, delegate)?
        }
        PairingWaitOutcome::AlreadyConnected(phone) => return Ok(phone),
    };

    delegate.on_event(PairingEvent::Status(
        "Looking for the wireless debugging connection endpoint...".to_string(),
    ))?;

    let device = connect_and_wait_for_device(
        adb,
        &pairing_address,
        &baseline_services,
        &baseline_devices,
        timeout,
        delegate,
    )?;

    Ok(ConnectedPhone {
        serial: device.serial.clone(),
        display_name: device.display_name(),
    })
}

fn wait_for_pairing_endpoint<D>(
    adb: &Adb,
    instance: &str,
    timeout: Duration,
    delegate: &mut D,
) -> Result<PairingWaitOutcome>
where
    D: PairingFlowDelegate,
{
    let deadline = Instant::now() + timeout;
    let mut reported_direct_check = false;
    let mut reported_device_check_error = false;
    let mut reported_adb_error = false;
    let mut reported_bonjour_error = false;

    loop {
        delegate.on_event(PairingEvent::Progress(
            PairingProgress::new(
                PairingProgressKind::WaitingForQrScan,
                "Waiting for QR scan",
                format!("Listening for pairing service `{instance}` via ADB mDNS and Bonjour."),
            )
            .deadline(deadline),
        ))?;

        let devices_result = adb.devices();
        let services_result = adb.mdns_services();
        let services = services_result.as_ref().map(Vec::as_slice).unwrap_or(&[]);

        match devices_result {
            Ok(mut devices) => {
                if disconnect_duplicate_wireless_aliases(adb, &devices, services, delegate)? {
                    devices = adb.devices().unwrap_or(devices);
                }

                let ready_phones = connected_phones_from_devices(devices, services);
                if !ready_phones.is_empty() {
                    match delegate.choose_already_connected(&ready_phones)? {
                        AlreadyConnectedChoice::Use(index) if index < ready_phones.len() => {
                            let phone = ready_phones[index].clone();
                            delegate.on_event(PairingEvent::Success(format!(
                                "ADB already sees {}; skipping QR scan.",
                                phone.display_name
                            )))?;
                            return Ok(PairingWaitOutcome::AlreadyConnected(phone));
                        }
                        _ => {}
                    }
                }
            }
            Err(error) if !reported_device_check_error => {
                delegate.on_event(PairingEvent::Warning(format!(
                    "could not check existing ADB devices: {error:#}"
                )))?;
                reported_device_check_error = true;
            }
            Err(_) => {}
        }

        if !reported_direct_check {
            delegate.on_event(PairingEvent::Status(
                "Also checking macOS Bonjour directly for the QR pairing service...".to_string(),
            ))?;
            reported_direct_check = true;
        }

        let discovery =
            pairing::discover_pairing_endpoint_candidates(adb, instance, Duration::from_secs(2));

        if !discovery.endpoints.is_empty() {
            if discovery.endpoints.len() == 1 {
                delegate.on_event(PairingEvent::Success(format!(
                    "Phone found at {}.",
                    discovery.endpoints[0]
                )))?;
            } else {
                delegate.on_event(PairingEvent::Success(format!(
                    "Phone found at candidate endpoint(s): {}.",
                    discovery.endpoints.join(", ")
                )))?;
            }
            return Ok(PairingWaitOutcome::PairingEndpoints(discovery.endpoints));
        }

        if let Some(error) = discovery.adb_error {
            if !reported_adb_error {
                delegate.on_event(PairingEvent::Warning(format!(
                    "adb mDNS lookup failed: {error}"
                )))?;
                reported_adb_error = true;
            }
        }

        if let Some(error) = discovery.bonjour_error {
            if !reported_bonjour_error {
                delegate.on_event(PairingEvent::Warning(format!(
                    "Bonjour pairing lookup failed: {error}"
                )))?;
                reported_bonjour_error = true;
            }
        }

        if Instant::now() >= deadline {
            bail!(
                "timed out waiting for the phone to advertise the QR pairing service `{instance}`"
            );
        }

        delegate.sleep(poll_delay(deadline, Duration::from_millis(500)))?;
    }
}

fn pair_with_pairing_endpoints<D>(
    adb: &Adb,
    endpoints: &[String],
    secret: &str,
    delegate: &mut D,
) -> Result<String>
where
    D: PairingFlowDelegate,
{
    let mut last_error = None;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut attempt = 0;
    let mut reset_adb_after_protocol_fault = false;

    delegate.on_event(PairingEvent::Success(
        "Phone found. Completing ADB pairing...".to_string(),
    ))?;

    'pairing: loop {
        let mut retryable_failure = false;

        for endpoint in endpoints {
            attempt += 1;
            delegate.on_event(PairingEvent::Progress(
                PairingProgress::new(
                    PairingProgressKind::CompletingPairing,
                    "Completing ADB pairing",
                    "QR scan detected; running `adb pair`.",
                )
                .deadline(deadline)
                .attempt(attempt)
                .endpoint(endpoint.clone()),
            ))?;
            delegate.on_event(PairingEvent::Status(format!("Pairing with {endpoint}...")))?;

            match adb.pair(endpoint, secret) {
                Ok(_) => {
                    delegate.on_event(PairingEvent::Success(format!(
                        "Pairing succeeded with {endpoint}."
                    )))?;
                    return Ok(endpoint.clone());
                }
                Err(error) => {
                    let message = format!("{error:#}");
                    delegate.on_event(PairingEvent::Warning(format!(
                        "Pairing endpoint {endpoint} failed: {message}"
                    )))?;
                    last_error = Some(message.clone());

                    if pairing_error_needs_adb_reset(&message) {
                        if reset_adb_after_protocol_fault {
                            last_error = Some(format!(
                                "{message}. ADB was restarted once and the pairing protocol still failed; start a new QR scan, or toggle Wireless debugging off and on before retrying."
                            ));
                            break 'pairing;
                        }

                        reset_adb_after_protocol_fault = true;
                        delegate.on_event(PairingEvent::Progress(
                            PairingProgress::new(
                                PairingProgressKind::RestartingAdb,
                                "Restarting ADB server",
                                "ADB reported a pairing protocol fault; resetting the local ADB server before retrying.",
                            )
                            .deadline(deadline)
                            .attempt(attempt)
                            .endpoint(endpoint.clone()),
                        ))?;
                        delegate.on_event(PairingEvent::Status(
                            "Restarting ADB server after pairing protocol fault...".to_string(),
                        ))?;

                        match adb.reset_server() {
                            Ok(()) => {
                                delegate.sleep(Duration::from_secs(1))?;
                                continue 'pairing;
                            }
                            Err(reset_error) => {
                                last_error = Some(format!(
                                    "{message}; additionally failed to restart ADB: {reset_error:#}"
                                ));
                                break 'pairing;
                            }
                        }
                    }

                    retryable_failure |= adb::pairing_error_is_retryable(&message);
                    delegate.on_event(PairingEvent::Progress(
                        PairingProgress::new(
                            PairingProgressKind::CompletingPairing,
                            "Pairing endpoint not ready",
                            format!("Retrying after: {}", compact_error(&message)),
                        )
                        .deadline(deadline)
                        .attempt(attempt)
                        .endpoint(endpoint.clone()),
                    ))?;
                }
            }
        }

        if !retryable_failure || Instant::now() >= deadline {
            break;
        }

        delegate.on_event(PairingEvent::Status(
            "Pairing endpoint is visible but not ready yet; retrying...".to_string(),
        ))?;
        delegate.on_event(PairingEvent::Progress(
            PairingProgress::new(
                PairingProgressKind::CompletingPairing,
                "Pairing with your phone... retrying",
                "ADB saw the phone, but the pairing socket was not ready yet.",
            )
            .deadline(deadline)
            .attempt(attempt),
        ))?;
        delegate.sleep(poll_delay(deadline, Duration::from_millis(800)))?;
    }

    bail!(
        "failed to pair with any discovered endpoint{}",
        last_error
            .map(|error| format!("; last error: {error}"))
            .unwrap_or_default()
    )
}

fn connect_and_wait_for_device<D>(
    adb: &Adb,
    pairing_address: &str,
    baseline_services: &HashSet<adb::MdnsService>,
    baseline_devices: &HashSet<String>,
    timeout: Duration,
    delegate: &mut D,
) -> Result<adb::AdbDevice>
where
    D: PairingFlowDelegate,
{
    let deadline = Instant::now() + timeout;
    let mut expected_serial = pairing_address.to_string();
    let mut announced_endpoints = HashSet::new();
    let mut reported_waiting_for_endpoint = false;
    let mut attempt = 0;
    let mut last_candidate_summary = String::new();
    let mut last_bonjour_check = None;
    let mut reported_connect_mdns_error = false;
    let mut reported_failed_connects = HashSet::new();

    loop {
        delegate.on_event(PairingEvent::Progress(
            PairingProgress::new(
                PairingProgressKind::WaitingForConnectionEndpoint,
                "Connection endpoint wait",
                "Checking `adb devices` and wireless debugging connect services.",
            )
            .deadline(deadline)
            .endpoint(pairing_address.to_string()),
        ))?;

        let mut ready_devices = adb.devices().unwrap_or_default();
        let services = match adb.mdns_services() {
            Ok(services) => services,
            Err(error) => {
                if !reported_connect_mdns_error {
                    delegate.on_event(PairingEvent::Warning(format!(
                        "adb mDNS connect lookup failed: {error:#}"
                    )))?;
                    reported_connect_mdns_error = true;
                }

                Vec::new()
            }
        };
        let candidates =
            adb::connect_service_candidates(&services, pairing_address, baseline_services);

        if disconnect_duplicate_wireless_aliases(adb, &ready_devices, &services, delegate)? {
            ready_devices = adb.devices().unwrap_or(ready_devices);
        }

        if let Some(device) = matching_ready_device_from_snapshot(
            &ready_devices,
            &expected_serial,
            baseline_devices,
            &services,
        ) {
            delegate.on_event(PairingEvent::Success(format!(
                "ADB device is ready: {}",
                device.display_name()
            )))?;
            return Ok(device);
        }

        let ready_device_count = ready_devices
            .iter()
            .filter(|device| device.state == adb::DeviceState::Device)
            .count();

        if ready_device_count > 0 {
            delegate.on_event(PairingEvent::Status(format!(
                "ADB sees {ready_device_count} ready device(s), but not the just-paired phone yet."
            )))?;
        }

        let candidate_summary = endpoint_summary(&candidates);

        for service in &candidates {
            if let Some(device) = adb::matching_ready_device_for_connect_service(
                &ready_devices,
                service,
                baseline_devices,
            ) {
                delegate.on_event(PairingEvent::Success(format!(
                    "ADB device is ready through mDNS: {}",
                    device.display_name()
                )))?;
                return Ok(device);
            }
        }

        if candidates.is_empty() && !reported_waiting_for_endpoint {
            delegate.on_event(PairingEvent::Status(
                "Waiting for the phone to advertise its connection endpoint...".to_string(),
            ))?;
            reported_waiting_for_endpoint = true;
        } else if !candidates.is_empty() && candidate_summary != last_candidate_summary {
            delegate.on_event(PairingEvent::Status(format!(
                "Connect endpoint candidate(s): {candidate_summary}"
            )))?;
            last_candidate_summary = candidate_summary;
        }

        if candidates.is_empty() && should_check_bonjour(last_bonjour_check) {
            last_bonjour_check = Some(Instant::now());

            if let Some(device) = try_direct_bonjour_connect(
                adb,
                pairing_address,
                baseline_devices,
                None,
                timeout,
                delegate,
            )? {
                return Ok(device);
            }
        }

        for service in candidates {
            if announced_endpoints.insert(service.address.clone()) {
                delegate.on_event(PairingEvent::Status(format!(
                    "Connecting to {}...",
                    service.address
                )))?;
            }

            attempt += 1;
            delegate.on_event(PairingEvent::Progress(
                PairingProgress::new(
                    PairingProgressKind::Connecting,
                    "Connecting to phone",
                    "Running `adb connect` and verifying the new device.",
                )
                .deadline(deadline)
                .attempt(attempt)
                .endpoint(service.address.clone()),
            ))?;
            delegate.on_event(PairingEvent::Status(format!(
                "Attempt {attempt}: adb connect {}",
                service.address
            )))?;

            match adb.connect(&service.address) {
                Ok(output) => {
                    expected_serial = adb::connect_serial_from_output(&output.combined_output())
                        .unwrap_or_else(|| service.address.clone());

                    delegate.on_event(PairingEvent::Progress(
                        PairingProgress::new(
                            PairingProgressKind::Verifying,
                            "Verifying ADB device",
                            "ADB connected; waiting for the device to become ready.",
                        )
                        .deadline(deadline)
                        .attempt(attempt)
                        .endpoint(expected_serial.clone()),
                    ))?;

                    let mut devices = adb.devices().unwrap_or_default();

                    if disconnect_duplicate_wireless_aliases(adb, &devices, &services, delegate)? {
                        devices = adb.devices().unwrap_or(devices);
                    }

                    if let Some(device) = matching_ready_device_from_snapshot(
                        &devices,
                        &expected_serial,
                        baseline_devices,
                        &services,
                    ) {
                        return Ok(device);
                    }
                }
                Err(error) => {
                    if reported_failed_connects.insert(service.address.clone()) {
                        delegate.on_event(PairingEvent::Warning(format!(
                            "ADB mDNS endpoint {} failed: {error:#}",
                            service.address
                        )))?;
                    }
                }
            }
        }

        if !reported_failed_connects.is_empty() {
            if let Some(device) = try_direct_bonjour_connect(
                adb,
                pairing_address,
                baseline_devices,
                None,
                timeout,
                delegate,
            )? {
                return Ok(device);
            }

            if let Some(device) =
                try_ui_hierarchy_connect(adb, baseline_devices, timeout, delegate)?
            {
                return Ok(device);
            }
        }

        if Instant::now() >= deadline {
            if let Some(device) =
                try_ui_hierarchy_connect(adb, baseline_devices, timeout, delegate)?
            {
                return Ok(device);
            }

            delegate.on_event(PairingEvent::Warning(
                "Automatic discovery timed out before finding a connectable wireless debugging endpoint."
                    .to_string(),
            ))?;

            if let Some(endpoint) = delegate.manual_connect_endpoint()? {
                return connect_to_endpoint(adb, &endpoint, baseline_devices, timeout, delegate);
            }

            bail!("paired, but no connectable endpoint was found");
        }

        delegate.sleep(poll_delay(deadline, Duration::from_secs(2)))?;
    }
}

fn try_direct_bonjour_connect<D>(
    adb: &Adb,
    pairing_address: &str,
    baseline_devices: &HashSet<String>,
    skipped_endpoint: Option<&str>,
    timeout: Duration,
    delegate: &mut D,
) -> Result<Option<adb::AdbDevice>>
where
    D: PairingFlowDelegate,
{
    delegate.on_event(PairingEvent::Status(
        "Checking macOS Bonjour directly for wireless debugging endpoints...".to_string(),
    ))?;

    let endpoints = match dnssd::discover_connect_endpoints(pairing_address, Duration::from_secs(6))
    {
        Ok(endpoints) => endpoints,
        Err(error) => {
            delegate.on_event(PairingEvent::Warning(format!(
                "Bonjour connect lookup failed: {error:#}"
            )))?;
            return Ok(None);
        }
    };

    if endpoints.is_empty() {
        delegate.on_event(PairingEvent::Status(
            "No Bonjour connect endpoints found outside ADB.".to_string(),
        ))?;
        return Ok(None);
    }

    delegate.on_event(PairingEvent::Status(format!(
        "Bonjour endpoint candidate(s): {}",
        endpoints.join(", ")
    )))?;

    let verify_timeout = if timeout < Duration::from_secs(1) {
        Duration::from_secs(1)
    } else {
        timeout.min(Duration::from_secs(8))
    };

    for endpoint in endpoints {
        if skipped_endpoint == Some(endpoint.as_str()) {
            delegate.on_event(PairingEvent::Status(format!(
                "Skipping {endpoint}; ADB already tried it."
            )))?;
            continue;
        }

        match connect_to_endpoint(adb, &endpoint, baseline_devices, verify_timeout, delegate) {
            Ok(device) => return Ok(Some(device)),
            Err(error) => delegate.on_event(PairingEvent::Warning(format!(
                "Bonjour endpoint {endpoint} failed: {error:#}"
            )))?,
        }
    }

    Ok(None)
}

fn try_ui_hierarchy_connect<D>(
    adb: &Adb,
    baseline_devices: &HashSet<String>,
    timeout: Duration,
    delegate: &mut D,
) -> Result<Option<adb::AdbDevice>>
where
    D: PairingFlowDelegate,
{
    let ready_devices =
        ui_hierarchy_candidate_devices(adb.devices().unwrap_or_default(), baseline_devices);

    if ready_devices.is_empty() {
        delegate.on_event(PairingEvent::Status(
            "No new ADB transport is available for screen parsing.".to_string(),
        ))?;
        return Ok(None);
    }

    delegate.on_event(PairingEvent::Status(
        "Trying to read the visible phone screen through ADB...".to_string(),
    ))?;
    let mut seen_endpoints = HashSet::new();
    let verify_timeout = if timeout < Duration::from_secs(1) {
        Duration::from_secs(1)
    } else {
        timeout.min(Duration::from_secs(8))
    };

    for device in ready_devices {
        delegate.on_event(PairingEvent::Progress(
            PairingProgress::new(
                PairingProgressKind::ReadingScreen,
                "Reading phone screen",
                format!(
                    "Looking for the visible Wireless debugging IP:port on {}.",
                    device.display_name()
                ),
            )
            .endpoint(device.serial.clone()),
        ))?;

        let hierarchy = match adb.dump_ui_hierarchy(&device.serial) {
            Ok(hierarchy) => hierarchy,
            Err(error) => {
                delegate.on_event(PairingEvent::Warning(format!(
                    "Could not read UI hierarchy from {}: {error:#}",
                    device.display_name()
                )))?;
                continue;
            }
        };

        let endpoints = extract_ipv4_endpoints(&hierarchy);

        if endpoints.is_empty() {
            delegate.on_event(PairingEvent::Status(format!(
                "No IP:port text found on {}.",
                device.display_name()
            )))?;
            continue;
        }

        delegate.on_event(PairingEvent::Status(format!(
            "Screen endpoint candidate(s): {}",
            endpoints.join(", ")
        )))?;

        for endpoint in endpoints {
            if !seen_endpoints.insert(endpoint.clone()) {
                continue;
            }

            match connect_to_endpoint(adb, &endpoint, baseline_devices, verify_timeout, delegate) {
                Ok(device) => return Ok(Some(device)),
                Err(error) => delegate.on_event(PairingEvent::Warning(format!(
                    "Screen endpoint {endpoint} failed: {error:#}"
                )))?,
            }
        }
    }

    Ok(None)
}

fn connect_to_endpoint<D>(
    adb: &Adb,
    endpoint: &str,
    baseline_devices: &HashSet<String>,
    timeout: Duration,
    delegate: &mut D,
) -> Result<adb::AdbDevice>
where
    D: PairingFlowDelegate,
{
    delegate.on_event(PairingEvent::Status(format!("Connecting to {endpoint}...")))?;
    let output = adb.connect(endpoint)?;
    let expected_serial = adb::connect_serial_from_output(&output.combined_output())
        .unwrap_or_else(|| endpoint.to_string());

    delegate.on_event(PairingEvent::Status(
        "Verifying the device is ready...".to_string(),
    ))?;
    wait_for_ready_device(adb, &expected_serial, baseline_devices, timeout, delegate)
}

fn wait_for_ready_device<D>(
    adb: &Adb,
    expected_serial: &str,
    baseline_devices: &HashSet<String>,
    timeout: Duration,
    delegate: &mut D,
) -> Result<adb::AdbDevice>
where
    D: PairingFlowDelegate,
{
    let deadline = Instant::now() + timeout;
    delegate.on_event(PairingEvent::Status(
        "Waiting for adb devices...".to_string(),
    ))?;

    loop {
        delegate.on_event(PairingEvent::Progress(
            PairingProgress::new(
                PairingProgressKind::Verifying,
                "ADB device wait",
                "Waiting for the connected device to appear in `adb devices`.",
            )
            .deadline(deadline)
            .endpoint(expected_serial.to_string()),
        ))?;

        let mut devices = adb.devices().unwrap_or_default();
        let services = adb.mdns_services().unwrap_or_default();

        if disconnect_duplicate_wireless_aliases(adb, &devices, &services, delegate)? {
            devices = adb.devices().unwrap_or(devices);
        }

        if let Some(device) = matching_ready_device_from_snapshot(
            &devices,
            expected_serial,
            baseline_devices,
            &services,
        ) {
            delegate.on_event(PairingEvent::Success(format!(
                "ADB device is ready: {}",
                device.display_name()
            )))?;
            return Ok(device);
        }

        if Instant::now() >= deadline {
            bail!("timed out waiting for {expected_serial} to appear in adb devices");
        }

        delegate.sleep(poll_delay(deadline, Duration::from_secs(2)))?;
    }
}

fn matching_ready_device_from_snapshot(
    devices: &[adb::AdbDevice],
    expected_serial: &str,
    baseline_devices: &HashSet<String>,
    services: &[adb::MdnsService],
) -> Option<adb::AdbDevice> {
    if let Some(device) = adb::matching_ready_device(devices, expected_serial, baseline_devices) {
        return Some(device);
    }

    let expected_host = adb::endpoint_host(expected_serial);

    services
        .iter()
        .filter(|service| {
            service.is_connect_service() && adb::endpoint_host(&service.address) == expected_host
        })
        .find_map(|service| {
            adb::matching_ready_device_for_connect_service(devices, service, baseline_devices)
        })
}

fn connected_phones_from_devices(
    devices: Vec<adb::AdbDevice>,
    services: &[adb::MdnsService],
) -> Vec<ConnectedPhone> {
    adb::dedupe_ready_devices(devices, services)
        .into_iter()
        .filter(|device| device.state == adb::DeviceState::Device)
        .map(|device| ConnectedPhone {
            serial: device.serial.clone(),
            display_name: device.display_name(),
        })
        .collect()
}

fn disconnect_duplicate_wireless_aliases<D>(
    adb: &Adb,
    devices: &[adb::AdbDevice],
    services: &[adb::MdnsService],
    delegate: &mut D,
) -> Result<bool>
where
    D: PairingFlowDelegate,
{
    let aliases = adb::duplicate_wireless_endpoint_aliases(devices, services);

    for alias in &aliases {
        delegate.on_event(PairingEvent::Status(format!(
            "Removing duplicate ADB transport {alias}..."
        )))?;

        match adb.disconnect(alias) {
            Ok(_) => delegate.on_event(PairingEvent::Success(format!(
                "Removed duplicate ADB transport {alias}."
            )))?,
            Err(error) => delegate.on_event(PairingEvent::Warning(format!(
                "could not remove duplicate ADB transport {alias}: {error:#}"
            )))?,
        }
    }

    Ok(!aliases.is_empty())
}

fn ui_hierarchy_candidate_devices(
    devices: Vec<adb::AdbDevice>,
    baseline_devices: &HashSet<String>,
) -> Vec<adb::AdbDevice> {
    devices
        .into_iter()
        .filter(|device| device.state == adb::DeviceState::Device)
        .filter(|device| !baseline_devices.contains(&device.serial))
        .collect()
}

fn should_check_bonjour(last_check: Option<Instant>) -> bool {
    match last_check {
        Some(last_check) => last_check.elapsed() >= Duration::from_secs(10),
        None => true,
    }
}

fn extract_ipv4_endpoints(input: &str) -> Vec<String> {
    let mut endpoints = Vec::new();

    for token in input.split(|character: char| {
        !(character.is_ascii_digit() || character == '.' || character == ':')
    }) {
        let token = token.trim_matches('.');

        if is_ipv4_endpoint(token) && !endpoints.iter().any(|endpoint| endpoint == token) {
            endpoints.push(token.to_string());
        }
    }

    endpoints
}

fn is_ipv4_endpoint(endpoint: &str) -> bool {
    let Some((host, port)) = endpoint.rsplit_once(':') else {
        return false;
    };

    if !matches!(port.parse::<u16>(), Ok(port) if port > 0) {
        return false;
    }

    let mut host_parts = host.split('.');

    host_parts.clone().count() == 4 && host_parts.all(|part| part.parse::<u8>().is_ok())
}

fn endpoint_summary(services: &[adb::MdnsService]) -> String {
    if services.is_empty() {
        return "none".to_string();
    }

    services
        .iter()
        .map(|service| service.address.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

fn pairing_error_needs_adb_reset(message: &str) -> bool {
    let message = message.to_ascii_lowercase();

    message.contains("protocol fault")
        && (message.contains("couldn't read status")
            || message.contains("could not read status")
            || message.contains("no status"))
}

fn compact_error(message: &str) -> String {
    let message = message
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or("unknown error");

    const MAX_LEN: usize = 96;
    if message.chars().count() <= MAX_LEN {
        return message.to_string();
    }

    format!("{}...", message.chars().take(MAX_LEN).collect::<String>())
}

fn remaining_until(deadline: Instant) -> Duration {
    deadline.saturating_duration_since(Instant::now())
}

fn poll_delay(deadline: Instant, max_delay: Duration) -> Duration {
    remaining_until(deadline).min(max_delay)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_ipv4_endpoints_from_screen_text() {
        let hierarchy = r#"
            <node text="192.168.68.54:37197" />
            <node text="adb-ignored.local:37197" />
            <node text="999.168.68.54:37197" />
            <node text="192.168.68.54:37197" />
        "#;

        assert_eq!(
            extract_ipv4_endpoints(hierarchy),
            vec!["192.168.68.54:37197"]
        );
    }

    #[test]
    fn ui_hierarchy_candidates_skip_baseline_devices() {
        let mut baseline_devices = HashSet::new();
        baseline_devices.insert("already-ready".to_string());
        let devices = vec![adb_device("already-ready"), adb_device("new-ready")];

        let candidates = ui_hierarchy_candidate_devices(devices, &baseline_devices);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].serial, "new-ready");
    }

    #[test]
    fn validates_strict_ipv4_endpoint_shape() {
        assert!(is_ipv4_endpoint("192.168.68.54:37197"));
        assert!(!is_ipv4_endpoint("localhost:5555"));
        assert!(!is_ipv4_endpoint("192.168.68.54:0"));
        assert!(!is_ipv4_endpoint("999.168.68.54:37197"));
        assert!(!is_ipv4_endpoint("192.168.68.54:notaport"));
    }

    #[test]
    fn protocol_fault_pairing_errors_trigger_adb_reset() {
        assert!(pairing_error_needs_adb_reset(
            "adb pair failed: error: protocol fault (couldn't read status message): Undefined error: 0"
        ));
        assert!(pairing_error_needs_adb_reset(
            "error: protocol fault (no status)"
        ));
        assert!(!pairing_error_needs_adb_reset("connection refused"));
    }

    fn adb_device(serial: &str) -> adb::AdbDevice {
        adb::AdbDevice {
            serial: serial.to_string(),
            state: adb::DeviceState::Device,
            product: None,
            model: Some("Pixel_10_Pro".to_string()),
            device: None,
            transport_id: None,
        }
    }
}
