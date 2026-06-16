//! Background workers: status polling, QR pairing, and scrcpy mirrors.
//! All state lives in `Shared` behind a mutex; workers repaint the UI on change.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use awb_core::adb::{self, Adb};
use awb_core::dnssd;
use awb_core::pairing;
use awb_core::qr::{PairingQr, QrModules};
use awb_core::scrcpy::Scrcpy;
use awb_core::wifi;
use eframe::egui::Context;

const PAIRING_TIMEOUT: Duration = Duration::from_secs(120);
const PAIRING_RETRY_TIMEOUT: Duration = Duration::from_secs(30);
const PAIRING_RETRY_DELAY: Duration = Duration::from_millis(800);

#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub available: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub serial: String,
    pub mirror_key: String,
    pub name: String,
    pub ready: bool,
    pub state: String,
    pub is_emulator: bool,
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub adb: ToolInfo,
    pub scrcpy: ToolInfo,
    pub devices: Vec<DeviceInfo>,
}

#[derive(Debug, Clone)]
pub enum PairingPhase {
    Qr { modules: QrModules },
    Connecting { label: String },
    Failed { message: String },
}

pub struct PairingSession {
    pub phase: PairingPhase,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct Shared {
    pub snapshot: Option<Snapshot>,
    pub refreshing: bool,
    pub logs: Vec<String>,
    pub pairing: Option<PairingSession>,
    pub mirrors: HashMap<String, Child>,
    pub starting_mirrors: HashSet<String>,
}

impl Shared {
    pub fn log(&mut self, line: impl AsRef<str>) {
        self.logs.push(format!("[{}] {}", clock(), line.as_ref()));

        let overflow = self.logs.len().saturating_sub(600);
        if overflow > 0 {
            self.logs.drain(..overflow);
        }
    }
}

fn clock() -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let local_offset = local_utc_offset_seconds();
    let day_seconds = ((now as i64 + local_offset).rem_euclid(86_400)) as u64;

    format!(
        "{:02}:{:02}:{:02}",
        day_seconds / 3600,
        (day_seconds / 60) % 60,
        day_seconds % 60
    )
}

fn local_utc_offset_seconds() -> i64 {
    use std::sync::OnceLock;

    static OFFSET: OnceLock<i64> = OnceLock::new();

    // `date +%z` once at startup avoids a chrono dependency for log timestamps.
    *OFFSET.get_or_init(|| {
        std::process::Command::new("date")
            .arg("+%z")
            .output()
            .ok()
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .and_then(|raw| {
                let raw = raw.trim();
                let (sign, digits) = raw.split_at(1);
                let hours: i64 = digits.get(0..2)?.parse().ok()?;
                let minutes: i64 = digits.get(2..4)?.parse().ok()?;
                let offset = hours * 3600 + minutes * 60;
                Some(if sign == "-" { -offset } else { offset })
            })
            .unwrap_or(0)
    })
}

pub fn refresh_status(shared: Arc<Mutex<Shared>>, ctx: Context) {
    {
        let mut state = shared.lock().unwrap();
        if state.refreshing {
            return;
        }
        state.refreshing = true;
    }
    ctx.request_repaint();

    thread::spawn(move || {
        let snapshot = collect_snapshot();

        let mut state = shared.lock().unwrap();
        state.reap_finished_mirrors();
        state.snapshot = Some(snapshot);
        state.refreshing = false;
        drop(state);
        ctx.request_repaint();
    });
}

fn collect_snapshot() -> Snapshot {
    let adb_handle = Adb::resolve(None);

    let adb_info = match &adb_handle {
        Ok(adb) => match adb.version() {
            Ok(output) => ToolInfo {
                available: true,
                detail: output
                    .combined_output()
                    .lines()
                    .map(str::trim)
                    .find(|line| !line.is_empty())
                    .unwrap_or("available")
                    .to_string(),
            },
            Err(error) => ToolInfo {
                available: false,
                detail: format!("{error:#}"),
            },
        },
        Err(error) => ToolInfo {
            available: false,
            detail: format!("{error:#}"),
        },
    };

    let scrcpy_info = match Scrcpy::resolve(None, false) {
        Ok(scrcpy) => ToolInfo {
            available: true,
            detail: scrcpy.path().display().to_string(),
        },
        Err(_) => ToolInfo {
            available: false,
            detail: "Not found".to_string(),
        },
    };

    let devices = adb_handle
        .ok()
        .filter(|_| adb_info.available)
        .and_then(|adb| {
            let devices = adb.devices().ok()?;
            let services = adb.mdns_services().unwrap_or_default();

            Some(
                adb::dedupe_ready_devices(devices, &services)
                    .into_iter()
                    .map(|device| DeviceInfo {
                        mirror_key: mirror_key_for_device(&device, &services),
                        ready: device.state == adb::DeviceState::Device,
                        state: state_label(&device.state).to_string(),
                        is_emulator: is_emulator(&device),
                        name: device
                            .model
                            .as_deref()
                            .map(|model| model.replace('_', " "))
                            .unwrap_or_else(|| device.serial.clone()),
                        serial: device.serial,
                    })
                    .collect(),
            )
        })
        .unwrap_or_default();

    Snapshot {
        adb: adb_info,
        scrcpy: scrcpy_info,
        devices,
    }
}

fn is_emulator(device: &adb::AdbDevice) -> bool {
    device.serial.starts_with("emulator-")
        || device
            .model
            .as_deref()
            .is_some_and(|model| model.contains("sdk_gphone"))
}

fn state_label(state: &adb::DeviceState) -> &str {
    match state {
        adb::DeviceState::Device => "device",
        adb::DeviceState::Offline => "offline",
        adb::DeviceState::Unauthorized => "unauthorized",
        adb::DeviceState::Other(value) => value,
    }
}

fn mirror_key_for_device(device: &adb::AdbDevice, services: &[adb::MdnsService]) -> String {
    if let Some(service) = services
        .iter()
        .filter(|service| service.is_connect_service())
        .find(|service| adb::device_matches_connect_service(device, service))
    {
        return adb::connect_service_serial(service);
    }

    if adb::is_mdns_wireless_serial(&device.serial) {
        return adb::normalize_mdns_serial(&device.serial);
    }

    device.serial.clone()
}

impl Shared {
    pub fn reap_finished_mirrors(&mut self) {
        let mut finished = Vec::new();

        for (mirror_key, child) in self.mirrors.iter_mut() {
            if let Ok(Some(_)) = child.try_wait() {
                finished.push(mirror_key.clone());
            }
        }

        for mirror_key in finished {
            self.mirrors.remove(&mirror_key);
            self.log(format!("Mirror of {mirror_key} ended"));
        }
    }
}

pub fn start_mirror(
    shared: Arc<Mutex<Shared>>,
    ctx: Context,
    device: DeviceInfo,
    options: awb_core::scrcpy::ScrcpyOptions,
) {
    let mirror_key = device.mirror_key.clone();
    {
        let mut state = shared.lock().unwrap();
        if state.mirrors.contains_key(&mirror_key)
            || !state.starting_mirrors.insert(mirror_key.clone())
        {
            state.log(format!("Already mirroring {}", device.name));
            drop(state);
            ctx.request_repaint();
            return;
        }
    }

    thread::spawn(move || {
        let result = Scrcpy::resolve(None, false).and_then(|scrcpy| {
            let mut command = std::process::Command::new(scrcpy.path());
            command
                .args(awb_core::scrcpy::default_args(&device.serial, &options))
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());
            command
                .spawn()
                .map_err(|error| anyhow::anyhow!("failed to start scrcpy: {error}"))
        });

        let mut state = shared.lock().unwrap();
        let start_still_wanted = state.starting_mirrors.remove(&mirror_key);
        match result {
            Ok(mut child) if !start_still_wanted => {
                let _ = child.kill();
                let _ = child.wait();
                state.log(format!("Cancelled mirror start for {}", device.name));
            }
            Ok(mut child) if state.mirrors.contains_key(&mirror_key) => {
                // Another start won the race (e.g. a rapid double-click). Drop-
                // ping a Child does not kill scrcpy, so stop this extra process
                // rather than replacing — and losing track of — the tracked one.
                let _ = child.kill();
                let _ = child.wait();
                state.log(format!("Already mirroring {}", device.name));
            }
            Ok(mut child) => {
                state.log(format!("Mirroring {} (pid {})", device.name, child.id()));

                if let Some(out) = child.stdout.take() {
                    spawn_log_pump(shared.clone(), ctx.clone(), Box::new(BufReader::new(out)));
                }
                if let Some(err) = child.stderr.take() {
                    spawn_log_pump(shared.clone(), ctx.clone(), Box::new(BufReader::new(err)));
                }

                state.mirrors.insert(mirror_key, child);
            }
            Err(error) if start_still_wanted => state.log(format!("Mirror failed: {error:#}")),
            Err(_) => {}
        }
        drop(state);
        ctx.request_repaint();
    });
}

fn spawn_log_pump(shared: Arc<Mutex<Shared>>, ctx: Context, reader: Box<dyn BufRead + Send>) {
    thread::spawn(move || {
        for line in reader.lines().map_while(Result::ok) {
            if line.trim().is_empty() {
                continue;
            }

            shared.lock().unwrap().log(line);
            ctx.request_repaint();
        }
    });
}

pub fn stop_mirror(shared: &Arc<Mutex<Shared>>, serial: &str) {
    let mut state = shared.lock().unwrap();

    if let Some(mut child) = state.mirrors.remove(serial) {
        let _ = child.kill();
        let _ = child.wait();
        state.log(format!("Stopped mirror of {serial}"));
    } else if state.starting_mirrors.remove(serial) {
        state.log(format!("Cancelled mirror start for {serial}"));
    }
}

pub fn stop_all_mirrors(shared: &Arc<Mutex<Shared>>) {
    let mut state = shared.lock().unwrap();
    let serials: Vec<String> = state.mirrors.keys().cloned().collect();

    for serial in serials {
        if let Some(mut child) = state.mirrors.remove(&serial) {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
    state.starting_mirrors.clear();
}

pub fn start_pairing(shared: Arc<Mutex<Shared>>, ctx: Context) {
    let cancel = Arc::new(AtomicBool::new(false));

    {
        let mut state = shared.lock().unwrap();

        if let Some(session) = &state.pairing {
            session.cancel.store(true, Ordering::Relaxed);
        }

        if let Err(error) = wifi::ensure_pairing_wifi_ready() {
            let message = format!("{error:#}");
            state.pairing = Some(PairingSession {
                phase: PairingPhase::Failed {
                    message: message.clone(),
                },
                cancel: cancel.clone(),
            });
            state.log(format!("Pairing blocked: {message}"));
            drop(state);
            ctx.request_repaint();
            return;
        }

        let qr = PairingQr::generate();
        let modules = match qr.modules() {
            Ok(modules) => modules,
            Err(error) => {
                state.log(format!("QR generation failed: {error:#}"));
                return;
            }
        };

        state.pairing = Some(PairingSession {
            phase: PairingPhase::Qr { modules },
            cancel: cancel.clone(),
        });
        state.log(format!("Pairing started ({})", qr.instance));
        drop(state);

        let shared = shared.clone();
        let ctx = ctx.clone();
        thread::spawn(move || pairing_worker(shared, ctx, qr, cancel));
    }

    ctx.request_repaint();
}

pub fn cancel_pairing(shared: &Arc<Mutex<Shared>>) {
    let mut state = shared.lock().unwrap();

    if let Some(session) = state.pairing.take() {
        session.cancel.store(true, Ordering::Relaxed);
        state.log("Pairing cancelled");
    }
}

fn pairing_worker(
    shared: Arc<Mutex<Shared>>,
    ctx: Context,
    qr: PairingQr,
    cancel: Arc<AtomicBool>,
) {
    let result = run_pairing(&shared, &ctx, &qr, &cancel);

    if cancel.load(Ordering::Relaxed) {
        return;
    }

    let mut state = shared.lock().unwrap();
    match result {
        Ok(device_name) => {
            state.log(format!("Paired and connected to {device_name}"));
            state.pairing = None;
            drop(state);
            refresh_status(shared, ctx.clone());
        }
        Err(error) => {
            state.log(format!("Pairing failed: {error:#}"));
            set_phase(
                &mut state,
                PairingPhase::Failed {
                    message: format!("{error:#}"),
                },
            );
        }
    }
    ctx.request_repaint();
}

fn set_phase(state: &mut Shared, phase: PairingPhase) {
    if let Some(session) = &mut state.pairing {
        session.phase = phase;
    }
}

fn run_pairing(
    shared: &Arc<Mutex<Shared>>,
    ctx: &Context,
    qr: &PairingQr,
    cancel: &Arc<AtomicBool>,
) -> anyhow::Result<String> {
    let adb = Adb::resolve(None)?;
    let baseline_services = adb::connect_services(&adb.mdns_services().unwrap_or_default());
    let baseline_devices = adb::ready_device_serials(&adb.devices().unwrap_or_default());

    let pairing_endpoints = wait_for_pairing_endpoints(&adb, qr, cancel)?;
    ensure_not_cancelled(cancel)?;

    let pairing_endpoint =
        pair_with_pairing_endpoints(shared, ctx, &adb, pairing_endpoints, qr, cancel)?;
    ensure_not_cancelled(cancel)?;

    update_phase(
        shared,
        ctx,
        PairingPhase::Connecting {
            label: "Connecting…".to_string(),
        },
    );

    let device = connect_after_pairing(
        &adb,
        &pairing_endpoint,
        &baseline_services,
        &baseline_devices,
        cancel,
    )?;

    Ok(device.display_name())
}

fn update_phase(shared: &Arc<Mutex<Shared>>, ctx: &Context, phase: PairingPhase) {
    set_phase(&mut shared.lock().unwrap(), phase);
    ctx.request_repaint();
}

fn ensure_not_cancelled(cancel: &Arc<AtomicBool>) -> anyhow::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        anyhow::bail!("cancelled");
    }

    Ok(())
}

fn wait_for_pairing_endpoints(
    adb: &Adb,
    qr: &PairingQr,
    cancel: &Arc<AtomicBool>,
) -> anyhow::Result<Vec<String>> {
    let deadline = Instant::now() + PAIRING_TIMEOUT;

    loop {
        ensure_not_cancelled(cancel)?;

        let discovery = pairing::discover_pairing_endpoint_candidates(
            adb,
            &qr.instance,
            Duration::from_secs(2),
        );
        if !discovery.endpoints.is_empty() {
            return Ok(discovery.endpoints);
        }

        if Instant::now() >= deadline {
            anyhow::bail!("no QR scan detected");
        }

        thread::sleep(Duration::from_millis(400));
    }
}

fn pair_with_pairing_endpoints(
    shared: &Arc<Mutex<Shared>>,
    ctx: &Context,
    adb: &Adb,
    endpoints: Vec<String>,
    qr: &PairingQr,
    cancel: &Arc<AtomicBool>,
) -> anyhow::Result<String> {
    let mut last_error = None;
    let deadline = Instant::now() + PAIRING_RETRY_TIMEOUT;

    loop {
        ensure_not_cancelled(cancel)?;
        update_phase(
            shared,
            ctx,
            PairingPhase::Connecting {
                label: "Pairing with your phone...".to_string(),
            },
        );

        let mut retryable_failure = false;

        for endpoint in endpoints.clone() {
            ensure_not_cancelled(cancel)?;

            shared
                .lock()
                .unwrap()
                .log(format!("Pairing with endpoint {endpoint}"));

            match adb.pair(&endpoint, &qr.secret) {
                Ok(_) => return Ok(endpoint),
                Err(error) => {
                    let message = format!("{error:#}");
                    retryable_failure |= adb::pairing_error_is_retryable(&message);
                    shared
                        .lock()
                        .unwrap()
                        .log(format!("Pairing endpoint {endpoint} failed: {message}"));
                    last_error = Some(message);
                }
            }
        }

        if !retryable_failure || Instant::now() >= deadline {
            break;
        }

        shared
            .lock()
            .unwrap()
            .log("Pairing endpoint is visible but not ready yet; retrying...");
        update_phase(
            shared,
            ctx,
            PairingPhase::Connecting {
                label: "Pairing with your phone... retrying".to_string(),
            },
        );
        sleep_or_cancel(cancel, PAIRING_RETRY_DELAY)?;
    }

    anyhow::bail!(
        "failed to pair with any discovered endpoint{}",
        last_error
            .map(|error| format!("; last error: {error}"))
            .unwrap_or_default()
    )
}

fn sleep_or_cancel(cancel: &Arc<AtomicBool>, duration: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + duration;

    while Instant::now() < deadline {
        ensure_not_cancelled(cancel)?;
        thread::sleep(
            deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(100)),
        );
    }

    Ok(())
}

fn connect_after_pairing(
    adb: &Adb,
    pairing_endpoint: &str,
    baseline_services: &std::collections::HashSet<adb::MdnsService>,
    baseline_devices: &std::collections::HashSet<String>,
    cancel: &Arc<AtomicBool>,
) -> anyhow::Result<adb::AdbDevice> {
    let deadline = Instant::now() + Duration::from_secs(60);

    loop {
        ensure_not_cancelled(cancel)?;

        let devices = adb.devices().unwrap_or_default();
        let services = adb.mdns_services().unwrap_or_default();

        if let Some(device) =
            adb::matching_ready_device(&devices, pairing_endpoint, baseline_devices)
        {
            return Ok(device);
        }

        let mut endpoints: Vec<String> =
            adb::connect_service_candidates(&services, pairing_endpoint, baseline_services)
                .into_iter()
                .map(|service| service.address)
                .collect();

        // ADB can surface only a stale connect candidate while macOS Bonjour
        // sees the live endpoint, so always merge Bonjour results rather than
        // falling back to them only when ADB returned nothing. Keep the
        // Bonjour set on the pairing host so another advertising phone cannot
        // satisfy this pairing flow.
        if let Ok(found) =
            dnssd::discover_connect_endpoints(pairing_endpoint, Duration::from_secs(2))
        {
            for endpoint in bonjour_connect_candidates_for_pairing(found, pairing_endpoint) {
                if !endpoints.contains(&endpoint) {
                    endpoints.push(endpoint);
                }
            }
        }

        // Retry every candidate each pass: a phone's endpoint can refuse the
        // first connect and accept a few seconds later, so we must not skip a
        // candidate permanently before the deadline.
        for endpoint in endpoints {
            ensure_not_cancelled(cancel)?;

            if let Ok(output) = adb.connect(&endpoint) {
                let serial =
                    adb::connect_serial_from_output(&output.combined_output()).unwrap_or(endpoint);
                let devices = adb.devices().unwrap_or_default();

                if let Some(device) =
                    adb::matching_ready_device(&devices, &serial, baseline_devices)
                {
                    return Ok(device);
                }
            }
        }

        if Instant::now() >= deadline {
            anyhow::bail!("paired, but no connectable endpoint was found");
        }

        thread::sleep(Duration::from_millis(500));
    }
}

fn bonjour_connect_candidates_for_pairing(
    endpoints: Vec<String>,
    pairing_endpoint: &str,
) -> Vec<String> {
    let pairing_host = adb::endpoint_host(pairing_endpoint);

    endpoints
        .into_iter()
        .filter(|endpoint| adb::endpoint_host(endpoint) == pairing_host)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mirror_key_collapses_endpoint_and_mdns_aliases() {
        let service = mdns_connect_service("adb-5C020DLCH0007Q-tfPgZw", "192.168.68.59:36375");
        let endpoint_device = adb_device("192.168.68.59:36375");
        let mdns_device = adb_device("adb-5C020DLCH0007Q-tfPgZw._adb-tls-connect._tcp");

        assert_eq!(
            mirror_key_for_device(&endpoint_device, std::slice::from_ref(&service)),
            "adb-5C020DLCH0007Q-tfPgZw._adb-tls-connect._tcp"
        );
        assert_eq!(
            mirror_key_for_device(&mdns_device, &[service]),
            "adb-5C020DLCH0007Q-tfPgZw._adb-tls-connect._tcp"
        );
    }

    #[test]
    fn mirror_key_normalizes_mdns_serial_without_service_snapshot() {
        let device = adb_device("adb-5C020DLCH0007Q-tfPgZw._adb-tls-connect._tcp.local");

        assert_eq!(
            mirror_key_for_device(&device, &[]),
            "adb-5C020DLCH0007Q-tfPgZw._adb-tls-connect._tcp"
        );
    }

    #[test]
    fn bonjour_connect_candidates_keep_pairing_host_only() {
        assert_eq!(
            bonjour_connect_candidates_for_pairing(
                vec![
                    "192.168.68.54:37197".to_string(),
                    "192.168.68.99:37197".to_string(),
                ],
                "192.168.68.54:40713",
            ),
            vec!["192.168.68.54:37197"]
        );
    }

    fn mdns_connect_service(instance: &str, address: &str) -> adb::MdnsService {
        adb::MdnsService {
            instance: instance.to_string(),
            service_type: "_adb-tls-connect._tcp".to_string(),
            address: address.to_string(),
        }
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
