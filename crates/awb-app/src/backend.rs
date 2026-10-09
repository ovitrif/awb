//! Background workers: status polling, QR pairing, and scrcpy mirrors.
//! All state lives in `Shared` behind a mutex; workers repaint the UI on change.

use std::collections::{HashMap, HashSet};
use std::io::{BufRead, BufReader};
use std::process::Child;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime};

use awb_core::adb::{self, Adb};
use awb_core::emulator::{self, Emulator};
use awb_core::pairing_flow::{
    self, AlreadyConnectedChoice, PairingEvent, PairingFlowDelegate, PairingProgressKind,
};
use awb_core::qr::{PairingQr, QrModules};
use awb_core::scrcpy::Scrcpy;
use awb_core::wifi;
use eframe::egui::Context;

use crate::mock;

const PAIRING_TIMEOUT: Duration = Duration::from_secs(120);
static ADB_WORK_LOCK: Mutex<()> = Mutex::new(());

pub use awb_core::pairing_flow::PairingProgress;

#[derive(Debug, Clone)]
pub struct ToolInfo {
    pub available: bool,
    pub detail: String,
    pub warnings: Vec<String>,
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
pub struct AvdInfo {
    pub name: String,
    pub device: Option<DeviceInfo>,
}

impl AvdInfo {
    pub fn status(&self, launching: bool) -> &str {
        match &self.device {
            Some(device) if device.ready => "Running",
            _ if launching => "Starting…",
            Some(device) => &device.state,
            None => "Stopped",
        }
    }

    pub fn can_launch(&self, launching: bool) -> bool {
        self.device.is_none() && !launching
    }
}

#[derive(Debug, Clone)]
pub struct Snapshot {
    pub adb: ToolInfo,
    pub emulator: ToolInfo,
    pub scrcpy: ToolInfo,
    pub devices: Vec<DeviceInfo>,
    pub avds: Vec<AvdInfo>,
}

#[derive(Debug, Clone)]
pub enum PairingPhase {
    Qr {
        modules: QrModules,
        progress: PairingProgress,
    },
    Connecting {
        progress: PairingProgress,
    },
    Failed {
        message: String,
    },
    /// Pairing succeeded; the page shows this briefly before returning to the
    /// device list, so the way back reads as a step back.
    Paired {
        device_name: String,
        at: Instant,
    },
}

pub struct PairingSession {
    pub phase: PairingPhase,
    pub cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct Shared {
    pub snapshot: Option<Snapshot>,
    pub device_connected: bool,
    pub refreshing: bool,
    pub logs: Vec<String>,
    pub logged_tool_warnings: HashSet<String>,
    pub pairing: Option<PairingSession>,
    pub mirrors: HashMap<String, Child>,
    pub starting_mirrors: HashSet<String>,
    pub starting_avds: HashSet<String>,
    pub deleting_avds: HashSet<String>,
    /// Phones "paired" by the mock pairing flow, listed with the real ones.
    pub mock_devices: Vec<DeviceInfo>,
}

impl Shared {
    pub fn log(&mut self, line: impl AsRef<str>) {
        self.logs.push(format!("[{}] {}", clock(), line.as_ref()));

        let overflow = self.logs.len().saturating_sub(600);
        if overflow > 0 {
            self.logs.drain(..overflow);
        }
    }

    fn log_tool_warnings(&mut self, warnings: &[String]) {
        for warning in warnings {
            if self.logged_tool_warnings.insert(warning.clone()) {
                self.log(warning);
            }
        }
    }

    fn pairing_busy(&self) -> bool {
        matches!(
            self.pairing.as_ref().map(|session| &session.phase),
            Some(PairingPhase::Qr { .. } | PairingPhase::Connecting { .. })
        )
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
        if state.pairing_busy() {
            return;
        }
        state.refreshing = true;
    }
    ctx.request_repaint();

    thread::spawn(move || {
        let mut snapshot = collect_snapshot();
        let mut state = shared.lock().unwrap();
        snapshot.devices.extend(state.mock_devices.iter().cloned());
        let device_connected = snapshot.devices.iter().any(|device| device.ready);

        state.reap_finished_mirrors();
        state.log_tool_warnings(&snapshot.scrcpy.warnings);
        state.snapshot = Some(snapshot);
        state.device_connected = device_connected;
        state.refreshing = false;
        drop(state);
        ctx.request_repaint();
    });
}

/// Refresh only the state needed by the menu bar icon while the popover is
/// hidden. The full status refresh also probes scrcpy and mDNS, so keeping the
/// background path to `adb devices` avoids unnecessary process churn.
pub fn refresh_connection_state(shared: Arc<Mutex<Shared>>, ctx: Context) {
    {
        let mut state = shared.lock().unwrap();
        if state.refreshing || state.pairing_busy() {
            return;
        }
        state.refreshing = true;
    }

    thread::spawn(move || {
        let device_connected = {
            let _adb_work = ADB_WORK_LOCK.lock().unwrap();
            Adb::resolve(None)
                .ok()
                .and_then(|adb| adb.devices().ok())
                .is_some_and(|devices| has_ready_adb_device(&devices))
        };

        let mut state = shared.lock().unwrap();
        state.device_connected = device_connected;
        state.refreshing = false;
        drop(state);
        ctx.request_repaint();
    });
}

fn collect_snapshot() -> Snapshot {
    let emulator_handle = Emulator::resolve(None);
    let (emulator_info, avds) = match &emulator_handle {
        Ok(emulator) => match emulator.list_avds() {
            Ok(avds) => (
                ToolInfo {
                    available: true,
                    detail: emulator.path().display().to_string(),
                    warnings: Vec::new(),
                },
                avds,
            ),
            Err(error) => (
                ToolInfo {
                    available: false,
                    detail: format!("{error:#}"),
                    warnings: Vec::new(),
                },
                Vec::new(),
            ),
        },
        Err(error) => (
            ToolInfo {
                available: false,
                detail: format!("{error:#}"),
                warnings: Vec::new(),
            },
            Vec::new(),
        ),
    };

    let _adb_work = ADB_WORK_LOCK.lock().unwrap();
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
                warnings: Vec::new(),
            },
            Err(error) => ToolInfo {
                available: false,
                detail: format!("{error:#}"),
                warnings: Vec::new(),
            },
        },
        Err(error) => ToolInfo {
            available: false,
            detail: format!("{error:#}"),
            warnings: Vec::new(),
        },
    };

    let scrcpy_info = match Scrcpy::resolve(None, false) {
        Ok(scrcpy) => {
            let diagnostics = scrcpy.diagnostics(&Default::default());
            let detail = match diagnostics.version_line {
                Some(version) if !version.is_empty() => {
                    format!("{} ({})", version, scrcpy.path().display())
                }
                _ => scrcpy.path().display().to_string(),
            };

            ToolInfo {
                available: true,
                detail,
                warnings: diagnostics.warnings,
            }
        }
        Err(_) => ToolInfo {
            available: false,
            detail: "Not found".to_string(),
            warnings: Vec::new(),
        },
    };

    let (devices, running_avds) = adb_handle
        .as_ref()
        .ok()
        .filter(|_| adb_info.available)
        .and_then(|adb| {
            let devices = adb.devices().ok()?;
            let running_avds = devices
                .iter()
                .filter(|device| device.serial.starts_with("emulator-"))
                .filter_map(|device| {
                    adb.emulator_avd_name(&device.serial)
                        .ok()
                        .map(|name| (device.serial.clone(), name))
                })
                .collect::<HashMap<_, _>>();
            let services = adb.mdns_services().unwrap_or_default();
            let devices: Vec<_> = adb::dedupe_ready_devices(devices, &services)
                .into_iter()
                .map(|device| DeviceInfo {
                    name: running_avds
                        .get(&device.serial)
                        .map(|name| name.replace('_', " "))
                        .or_else(|| device.model.as_deref().map(|model| model.replace('_', " ")))
                        .unwrap_or_else(|| device.serial.clone()),
                    mirror_key: mirror_key_for_device(&device, &services),
                    ready: device.state == adb::DeviceState::Device,
                    state: state_label(&device.state).to_string(),
                    is_emulator: is_emulator(&device),
                    serial: device.serial,
                })
                .collect();

            Some((devices, running_avds))
        })
        .unwrap_or_default();
    let avds = avd_rows(avds, &devices, &running_avds);

    Snapshot {
        adb: adb_info,
        emulator: emulator_info,
        scrcpy: scrcpy_info,
        devices,
        avds,
    }
}

fn avd_rows(
    names: Vec<String>,
    devices: &[DeviceInfo],
    running_avds: &HashMap<String, String>,
) -> Vec<AvdInfo> {
    let mut avds: Vec<_> = names
        .into_iter()
        .map(|name| AvdInfo { name, device: None })
        .collect();

    for device in devices.iter().filter(|device| device.is_emulator) {
        let name = running_avds.get(&device.serial);
        if let Some(avd) = avds.iter_mut().find(|avd| Some(&avd.name) == name) {
            // Multiple instances of the same AVD still occupy one row. Prefer
            // a ready instance if another is offline.
            if avd.device.is_none() || device.ready {
                avd.device = Some(device.clone());
            }
        } else {
            // Keep externally started emulators visible even if AVD discovery
            // or the name lookup is unavailable.
            avds.push(AvdInfo {
                name: name.cloned().unwrap_or_else(|| device.name.clone()),
                device: Some(device.clone()),
            });
        }
    }

    avds
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

fn has_ready_adb_device(devices: &[adb::AdbDevice]) -> bool {
    devices
        .iter()
        .any(|device| device.state == adb::DeviceState::Device)
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
        let result = {
            let _adb_work = ADB_WORK_LOCK.lock().unwrap();
            Adb::resolve(None).and_then(|adb| {
                Scrcpy::resolve_with_adb(None, false, Some(adb)).and_then(|scrcpy| {
                    let diagnostics = scrcpy.diagnostics(&options);
                    scrcpy
                        .spawn_piped(&device.serial, &options)
                        .map(|child| (child, diagnostics))
                })
            })
        };

        let mut state = shared.lock().unwrap();
        let start_still_wanted = state.starting_mirrors.remove(&mirror_key);
        match result {
            Ok((mut child, _)) if !start_still_wanted => {
                let _ = child.kill();
                let _ = child.wait();
                state.log(format!("Cancelled mirror start for {}", device.name));
            }
            Ok((mut child, _)) if state.mirrors.contains_key(&mirror_key) => {
                // Another start won the race (e.g. a rapid double-click). Drop-
                // ping a Child does not kill scrcpy, so stop this extra process
                // rather than replacing — and losing track of — the tracked one.
                let _ = child.kill();
                let _ = child.wait();
                state.log(format!("Already mirroring {}", device.name));
            }
            Ok((mut child, diagnostics)) => {
                state.log_tool_warnings(&diagnostics.warnings);
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

pub fn start_avd(shared: Arc<Mutex<Shared>>, ctx: Context, name: String) {
    {
        let mut state = shared.lock().unwrap();
        if state.deleting_avds.contains(&name) {
            return;
        }
        if state.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .avds
                .iter()
                .any(|avd| avd.name == name && !avd.can_launch(false))
        }) {
            state.log(format!("AVD {name} is already running"));
            return;
        }
        if !state.starting_avds.insert(name.clone()) {
            state.log(format!("Already starting AVD {name}"));
            drop(state);
            ctx.request_repaint();
            return;
        }
    }

    thread::spawn(move || {
        match Emulator::resolve(None).and_then(|emulator| emulator.launch(&name)) {
            Ok(mut child) => {
                {
                    let mut state = shared.lock().unwrap();
                    state.log(format!("Starting AVD {name} (pid {})", child.id()));
                    if let Some(out) = child.stdout.take() {
                        spawn_log_pump(shared.clone(), ctx.clone(), Box::new(BufReader::new(out)));
                    }
                    if let Some(err) = child.stderr.take() {
                        spawn_log_pump(shared.clone(), ctx.clone(), Box::new(BufReader::new(err)));
                    }
                }
                ctx.request_repaint();
                refresh_status(shared.clone(), ctx.clone());

                let status = child.wait();
                let mut state = shared.lock().unwrap();
                state.starting_avds.remove(&name);
                match status {
                    Ok(status) if !status.success() => {
                        state.log(format!("AVD {name} exited with status {status}"));
                    }
                    Err(error) => state.log(format!("Could not wait for AVD {name}: {error}")),
                    _ => {}
                }
                drop(state);
                ctx.request_repaint();
                refresh_status(shared, ctx);
            }
            Err(error) => {
                let mut state = shared.lock().unwrap();
                state.starting_avds.remove(&name);
                state.log(format!("Could not start AVD {name}: {error:#}"));
                drop(state);
                ctx.request_repaint();
            }
        }
    });
}

pub fn delete_avd(shared: Arc<Mutex<Shared>>, ctx: Context, name: String) {
    {
        let mut state = shared.lock().unwrap();
        let stopped = state.snapshot.as_ref().is_some_and(|snapshot| {
            snapshot
                .avds
                .iter()
                .any(|avd| avd.name == name && avd.device.is_none())
        });
        if !stopped
            || state.starting_avds.contains(&name)
            || !state.deleting_avds.insert(name.clone())
        {
            return;
        }
    }
    ctx.request_repaint();

    thread::spawn(move || {
        let result = (|| -> anyhow::Result<()> {
            let _adb_work = ADB_WORK_LOCK.lock().unwrap();
            let adb = Adb::resolve(None)?;
            let devices = adb.devices()?;
            let active_names = devices
                .iter()
                .filter(|device| is_emulator(device))
                .map(|device| adb.emulator_avd_name(&device.serial))
                .collect::<anyhow::Result<Vec<_>>>()?;
            anyhow::ensure!(
                !active_names.contains(&name),
                "AVD {name} is running; stop it before deleting"
            );
            emulator::remove_avd(&name)
        })();

        let mut state = shared.lock().unwrap();
        state.deleting_avds.remove(&name);
        match result {
            Ok(()) => {
                if let Some(snapshot) = &mut state.snapshot {
                    snapshot.avds.retain(|avd| avd.name != name);
                }
                state.log(format!("Deleted AVD {name}"));
            }
            Err(error) => state.log(format!("Could not delete AVD {name}: {error:#}")),
        }
        drop(state);
        ctx.request_repaint();
        refresh_status(shared, ctx);
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

        if mock::pairing().is_none()
            && let Err(error) = wifi::ensure_pairing_wifi_ready()
        {
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

        state.pairing = Some(PairingSession {
            phase: PairingPhase::Connecting {
                progress: PairingProgress::new(
                    PairingProgressKind::WaitingForQrScan,
                    "Preparing QR",
                    "Generating a new ADB pairing QR code.",
                ),
            },
            cancel: cancel.clone(),
        });
        state.log("Pairing started");
        drop(state);

        let shared = shared.clone();
        let ctx = ctx.clone();
        thread::spawn(move || pairing_worker(shared, ctx, cancel));
    }

    ctx.request_repaint();
}

pub fn cancel_pairing(shared: &Arc<Mutex<Shared>>) {
    let mut state = shared.lock().unwrap();

    if let Some(session) = state.pairing.take() {
        session.cancel.store(true, Ordering::Relaxed);
        if !matches!(session.phase, PairingPhase::Paired { .. }) {
            state.log("Pairing cancelled");
        }
    }
}

fn pairing_worker(shared: Arc<Mutex<Shared>>, ctx: Context, cancel: Arc<AtomicBool>) {
    let result = match mock::pairing() {
        Some(mock) => run_mock_pairing(&shared, &ctx, &cancel, mock),
        None => run_pairing(&shared, &ctx, &cancel),
    };

    if cancel.load(Ordering::Relaxed) {
        return;
    }

    let mut state = shared.lock().unwrap();
    if !pairing_session_owns_cancel(&state, &cancel) {
        return;
    }

    match result {
        Ok(device_name) => {
            state.log(format!("Paired and connected to {device_name}"));
            set_phase(
                &mut state,
                PairingPhase::Paired {
                    device_name,
                    at: Instant::now(),
                },
            );
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

fn pairing_session_owns_cancel(state: &Shared, cancel: &Arc<AtomicBool>) -> bool {
    state
        .pairing
        .as_ref()
        .is_some_and(|session| Arc::ptr_eq(&session.cancel, cancel))
}

fn set_phase(state: &mut Shared, phase: PairingPhase) {
    if let Some(session) = &mut state.pairing {
        session.phase = phase;
    }
}

fn run_pairing(
    shared: &Arc<Mutex<Shared>>,
    ctx: &Context,
    cancel: &Arc<AtomicBool>,
) -> anyhow::Result<String> {
    let _adb_work = ADB_WORK_LOCK.lock().unwrap();
    let adb = Adb::resolve(None)?;
    let mut delegate = AppPairingDelegate {
        shared,
        ctx,
        cancel,
    };
    let phone = pairing_flow::pair_and_connect(&adb, PAIRING_TIMEOUT, &mut delegate)?;

    Ok(phone.display_name)
}

/// Feeds the app's pairing delegate a scripted pairing: the same events a real
/// pairing produces, with a simulated scan instead of mDNS and adb.
fn run_mock_pairing(
    shared: &Arc<Mutex<Shared>>,
    ctx: &Context,
    cancel: &Arc<AtomicBool>,
    mock: mock::MockPairing,
) -> anyhow::Result<String> {
    let mut delegate = AppPairingDelegate {
        shared,
        ctx,
        cancel,
    };
    delegate.on_event(PairingEvent::QrReady(PairingQr::with_instance(
        "awb-mock".to_string(),
    )))?;
    delegate.sleep(mock.scan_after)?;

    let steps = [
        (
            PairingProgressKind::CompletingPairing,
            "Pairing",
            "The phone scanned the code; completing pairing.",
        ),
        (
            PairingProgressKind::Connecting,
            "Connecting",
            "Opening a wireless debugging connection.",
        ),
        (
            PairingProgressKind::Verifying,
            "Verifying",
            "Checking that the phone answers over adb.",
        ),
    ];
    for (kind, title, detail) in steps {
        delegate.on_event(PairingEvent::Progress(
            PairingProgress::new(kind, title, detail).endpoint(mock::PHONE_ENDPOINT),
        ))?;
        delegate.sleep(Duration::from_millis(900))?;
    }

    match mock.outcome {
        mock::Outcome::Failure => {
            anyhow::bail!("Mock pairing failed: the phone rejected the pairing code.")
        }
        mock::Outcome::Success => {
            let mut state = shared.lock().unwrap();
            if !state
                .mock_devices
                .iter()
                .any(|device| device.serial == mock::PHONE_ENDPOINT)
            {
                state.mock_devices.push(DeviceInfo {
                    serial: mock::PHONE_ENDPOINT.to_string(),
                    mirror_key: mock::PHONE_ENDPOINT.to_string(),
                    name: mock::PHONE_NAME.to_string(),
                    ready: true,
                    state: "device".to_string(),
                    is_emulator: false,
                });
            }
            Ok(mock::PHONE_NAME.to_string())
        }
    }
}

struct AppPairingDelegate<'a> {
    shared: &'a Arc<Mutex<Shared>>,
    ctx: &'a Context,
    cancel: &'a Arc<AtomicBool>,
}

impl PairingFlowDelegate for AppPairingDelegate<'_> {
    fn on_event(&mut self, event: PairingEvent) -> anyhow::Result<()> {
        ensure_not_cancelled(self.cancel)?;

        match event {
            PairingEvent::Section { title, lines } => {
                self.shared
                    .lock()
                    .unwrap()
                    .log(format!("{title}: {}", lines.join(" ")));
            }
            PairingEvent::QrReady(qr) => {
                let modules = qr.modules()?;
                update_phase(
                    self.shared,
                    self.ctx,
                    self.cancel,
                    PairingPhase::Qr {
                        modules,
                        progress: PairingProgress::new(
                            PairingProgressKind::WaitingForQrScan,
                            "Waiting for QR scan",
                            "Listening for the phone's pairing service.",
                        )
                        .deadline(Instant::now() + PAIRING_TIMEOUT),
                    },
                );
                self.shared
                    .lock()
                    .unwrap()
                    .log(format!("Pairing QR ready ({})", qr.instance));
            }
            PairingEvent::Progress(progress) => {
                if progress.kind == PairingProgressKind::WaitingForQrScan {
                    update_qr_progress(self.shared, self.ctx, self.cancel, progress);
                } else {
                    update_phase(
                        self.shared,
                        self.ctx,
                        self.cancel,
                        PairingPhase::Connecting { progress },
                    );
                }
            }
            PairingEvent::Status(message) => self.shared.lock().unwrap().log(message),
            PairingEvent::Success(message) => self.shared.lock().unwrap().log(message),
            PairingEvent::Warning(message) => self
                .shared
                .lock()
                .unwrap()
                .log(format!("Warning: {message}")),
        }

        Ok(())
    }

    fn sleep(&mut self, duration: Duration) -> anyhow::Result<()> {
        sleep_or_cancel(self.cancel, duration)
    }

    fn choose_already_connected(
        &mut self,
        _phones: &[pairing_flow::ConnectedPhone],
    ) -> anyhow::Result<AlreadyConnectedChoice> {
        Ok(AlreadyConnectedChoice::KeepWaiting)
    }

    fn manual_connect_endpoint(&mut self) -> anyhow::Result<Option<String>> {
        self.shared.lock().unwrap().log(
            "Automatic connection discovery timed out; manual endpoint entry is only available in the CLI.",
        );
        Ok(None)
    }
}

fn update_phase(
    shared: &Arc<Mutex<Shared>>,
    ctx: &Context,
    cancel: &Arc<AtomicBool>,
    phase: PairingPhase,
) {
    let mut state = shared.lock().unwrap();
    if !pairing_session_owns_cancel(&state, cancel) {
        return;
    }

    set_phase(&mut state, phase);
    drop(state);
    ctx.request_repaint();
}

fn update_qr_progress(
    shared: &Arc<Mutex<Shared>>,
    ctx: &Context,
    cancel: &Arc<AtomicBool>,
    progress: PairingProgress,
) {
    let mut state = shared.lock().unwrap();
    if !pairing_session_owns_cancel(&state, cancel) {
        return;
    }

    if let Some(PairingSession {
        phase: PairingPhase::Qr {
            progress: current, ..
        },
        ..
    }) = &mut state.pairing
    {
        *current = progress;
    }

    drop(state);
    ctx.request_repaint();
}

fn ensure_not_cancelled(cancel: &Arc<AtomicBool>) -> anyhow::Result<()> {
    if cancel.load(Ordering::Relaxed) {
        anyhow::bail!("cancelled");
    }

    Ok(())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deletion_rejects_running_starting_and_already_deleting_avds() {
        let tool = ToolInfo {
            available: true,
            detail: String::new(),
            warnings: vec![],
        };
        for (device, starting, deleting) in [
            (Some(device_info("emulator-5554", true, true)), false, false),
            (
                Some(device_info("emulator-5554", true, false)),
                false,
                false,
            ),
            (None, true, false),
            (None, false, true),
        ] {
            let shared = Arc::new(Mutex::new(Shared {
                snapshot: Some(Snapshot {
                    adb: tool.clone(),
                    emulator: tool.clone(),
                    scrcpy: tool.clone(),
                    devices: vec![],
                    avds: vec![AvdInfo {
                        name: "Pixel_9a".to_string(),
                        device,
                    }],
                }),
                starting_avds: if starting {
                    HashSet::from(["Pixel_9a".to_string()])
                } else {
                    HashSet::new()
                },
                deleting_avds: if deleting {
                    HashSet::from(["Pixel_9a".to_string()])
                } else {
                    HashSet::new()
                },
                ..Default::default()
            }));
            delete_avd(shared.clone(), Context::default(), "Pixel_9a".to_string());
            assert_eq!(
                shared.lock().unwrap().deleting_avds.contains("Pixel_9a"),
                deleting
            );
            if deleting {
                start_avd(shared.clone(), Context::default(), "Pixel_9a".to_string());
                assert!(shared.lock().unwrap().starting_avds.is_empty());
            }
        }
    }

    #[test]
    fn avd_keeps_its_row_through_launch_running_and_shutdown() {
        let names = vec!["Pixel_9a".to_string(), "Pixel_10_Pro_XL".to_string()];
        let running_avds = HashMap::from([("emulator-5554".to_string(), "Pixel_9a".to_string())]);
        let phone = device_info("phone", false, true);
        let emulator = device_info("emulator-5554", true, true);

        for (devices, launching, expected_status, can_launch) in [
            (vec![phone.clone()], false, "Stopped", true),
            (vec![phone.clone()], true, "Starting…", false),
            (
                vec![phone.clone(), device_info("emulator-5554", true, false)],
                true,
                "Starting…",
                false,
            ),
            (
                vec![phone.clone(), emulator.clone()],
                true,
                "Running",
                false,
            ),
            (vec![phone.clone(), emulator], false, "Running", false),
            (vec![phone], false, "Stopped", true),
        ] {
            let rows = avd_rows(names.clone(), &devices, &running_avds);
            assert_eq!(
                rows.iter().map(|row| &row.name).collect::<Vec<_>>(),
                names.iter().collect::<Vec<_>>()
            );
            assert_eq!(rows[0].status(launching), expected_status);
            assert_eq!(rows[0].can_launch(launching), can_launch);
            assert_eq!(rows[1].status(false), "Stopped");
        }
    }

    #[test]
    fn external_emulators_stay_visible_without_avd_discovery() {
        let running = device_info("emulator-5554", true, true);
        let offline = device_info("emulator-5556", true, false);
        let rows = avd_rows(
            vec![],
            &[device_info("phone", false, true), running, offline],
            &HashMap::from([("emulator-5554".to_string(), "Pixel_9a".to_string())]),
        );

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].name, "Pixel_9a");
        assert_eq!(rows[0].status(false), "Running");
        assert_eq!(rows[1].name, "emulator-5556");
        assert_eq!(rows[1].status(false), "offline");
        assert!(rows.iter().all(|row| !row.can_launch(false)));
    }

    #[test]
    fn multiple_instances_of_an_avd_use_one_running_row() {
        let rows = avd_rows(
            vec!["Pixel_9a".to_string()],
            &[
                device_info("emulator-5554", true, true),
                device_info("emulator-5556", true, false),
            ],
            &HashMap::from([
                ("emulator-5554".to_string(), "Pixel_9a".to_string()),
                ("emulator-5556".to_string(), "Pixel_9a".to_string()),
            ]),
        );

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status(false), "Running");
        assert!(!rows[0].can_launch(false));
    }

    fn device_info(serial: &str, is_emulator: bool, ready: bool) -> DeviceInfo {
        DeviceInfo {
            serial: serial.to_string(),
            mirror_key: serial.to_string(),
            name: serial.to_string(),
            ready,
            state: if ready { "device" } else { "offline" }.to_string(),
            is_emulator,
        }
    }

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
    fn pairing_session_ownership_uses_cancel_token_identity() {
        let active_cancel = Arc::new(AtomicBool::new(false));
        let stale_cancel = Arc::new(AtomicBool::new(false));
        let state = Shared {
            pairing: Some(PairingSession {
                phase: PairingPhase::Connecting {
                    progress: PairingProgress::new(
                        PairingProgressKind::CompletingPairing,
                        "Pairing",
                        "Testing active session",
                    ),
                },
                cancel: active_cancel.clone(),
            }),
            ..Shared::default()
        };

        assert!(pairing_session_owns_cancel(&state, &active_cancel));
        assert!(!pairing_session_owns_cancel(&state, &stale_cancel));
    }

    #[test]
    fn menu_connection_state_uses_ready_adb_devices() {
        let ready = adb_device("adb-ready._adb-tls-connect._tcp");
        let mut offline = adb_device("adb-offline._adb-tls-connect._tcp");
        offline.state = adb::DeviceState::Offline;

        assert!(!has_ready_adb_device(&[]));
        assert!(!has_ready_adb_device(std::slice::from_ref(&offline)));
        assert!(has_ready_adb_device(&[offline, ready]));
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
