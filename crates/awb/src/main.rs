mod ui;

use std::collections::HashSet;
use std::env;
use std::io;
use std::path::PathBuf;
use std::process::{Child, Command as ProcessCommand, ExitCode, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result, bail};
use awb_core::adb::{self, Adb};
use awb_core::dnssd;
use awb_core::qr::PairingQr;
use awb_core::scrcpy::{
    DEFAULT_WINDOW_HEIGHT, DEFAULT_WINDOW_WIDTH, Scrcpy, ScrcpyOptions, ScrcpyRunMode,
};
use clap::{CommandFactory, Parser, Subcommand, ValueEnum};
use clap_complete::Shell;
use serde::Serialize;

const BINARY_NAME: &str = "awb";
const TRAY_BINARY_NAME: &str = "awb-tray";

#[derive(Debug, Parser)]
#[command(
    name = "awb",
    version,
    about = "awb (Android Wireless Bridge): interactive QR pairing for Android wireless debugging."
)]
struct Args {
    #[command(subcommand)]
    command: Option<CliCommand>,

    #[arg(long, value_name = "PATH", help = "Path to adb")]
    adb: Option<PathBuf>,

    #[arg(long, value_name = "PATH", help = "Path to scrcpy")]
    scrcpy: Option<PathBuf>,

    #[arg(
        long,
        default_value_t = 60,
        value_name = "SECONDS",
        help = "How long to wait for phone pairing and connection discovery"
    )]
    timeout: u64,

    #[arg(long, help = "Skip checking for scrcpy before launching it")]
    no_scrcpy_check: bool,

    #[arg(long, help = "Kill and restart the local ADB server before pairing")]
    reset_adb: bool,

    #[arg(
        long,
        conflicts_with_all = ["background", "foreground", "stable"],
        help = "Connect or pair a phone without starting scrcpy or showing the scrcpy menu"
    )]
    connect_only: bool,

    #[arg(
        long,
        conflicts_with = "foreground",
        help = "Start scrcpy without waiting for it and skip the menu"
    )]
    background: bool,

    #[arg(
        long,
        conflicts_with = "background",
        help = "Start scrcpy and wait until it exits, then skip the menu"
    )]
    foreground: bool,

    #[arg(
        long,
        help = "Use scrcpy's regular decorated window instead of a borderless Pixel-style window"
    )]
    plain_window: bool,

    #[arg(long, help = "Keep the scrcpy window above other windows")]
    always_on_top: bool,

    #[arg(
        long,
        default_value = "Pixel 10 Pro",
        value_name = "TEXT",
        help = "Window title passed to scrcpy"
    )]
    window_title: String,

    #[arg(
        long,
        default_value_t = DEFAULT_WINDOW_WIDTH,
        value_name = "POINTS",
        help = "Initial scrcpy window width; use 0 for scrcpy's automatic size"
    )]
    window_width: u32,

    #[arg(
        long,
        default_value_t = DEFAULT_WINDOW_HEIGHT,
        value_name = "POINTS",
        help = "Initial scrcpy window height; use 0 for scrcpy's automatic size"
    )]
    window_height: u32,

    #[arg(
        long,
        help = "Keep supervising wireless ADB and reconnect when the transport drops"
    )]
    watch: bool,

    #[arg(
        long,
        help = "Convenience mode: start scrcpy, watch reconnect, keep awake and Wi-Fi diagnostics"
    )]
    stable: bool,

    #[arg(
        long,
        default_value_t = 5,
        value_name = "SECONDS",
        help = "Seconds between ADB keepalive checks in watch mode"
    )]
    keepalive_interval: u64,

    #[arg(
        long,
        default_value_t = 2,
        value_name = "COUNT",
        help = "Consecutive failed keepalives before reconnecting"
    )]
    keepalive_failures: u8,

    #[arg(
        long,
        help = "Ask Android to keep the screen awake even when not launching scrcpy"
    )]
    keep_screen_awake: bool,

    #[arg(
        long,
        value_name = "SERIAL",
        help = "Use a specific connected ADB device serial instead of prompting"
    )]
    device_serial: Option<String>,

    #[arg(long, help = "Print Wi-Fi status after connecting and when it changes")]
    wifi_doctor: bool,
}

#[derive(Debug, Clone, Subcommand)]
enum CliCommand {
    #[command(about = "Print local ADB, scrcpy, and connected device status")]
    Status(StatusArgs),

    #[command(about = "Kill and restart the local ADB server")]
    ResetAdb,

    #[command(about = "Print shell completions")]
    Completions(CompletionArgs),

    #[command(about = "Launch the awb menu bar app")]
    Tray,
}

#[derive(Debug, Clone, clap::Args)]
struct StatusArgs {
    #[arg(long, help = "Print status as JSON for desktop UI and automation")]
    json: bool,
}

#[derive(Debug, Clone, clap::Args)]
struct CompletionArgs {
    #[arg(value_enum, default_value_t = CompletionShell::Zsh)]
    shell: CompletionShell,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum CompletionShell {
    Zsh,
}

#[derive(Debug, Clone)]
struct ConnectedPhone {
    serial: String,
    display_name: String,
}

enum StartupDeviceChoice {
    Connected(ConnectedPhone),
    PairNew,
    Close,
}

enum PairingWaitOutcome {
    PairingEndpoint(String),
    AlreadyConnected(ConnectedPhone),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ScrcpyLaunchMode {
    Menu,
    Background,
    Foreground,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConnectedAction {
    StartBackground,
    StartForeground,
    Close,
}

impl ConnectedAction {
    fn starts_scrcpy(self) -> bool {
        matches!(self, Self::StartBackground | Self::StartForeground)
    }
}

#[derive(Debug, Serialize)]
struct StatusSnapshot {
    awb_version: &'static str,
    adb: AdbStatus,
    scrcpy: ToolStatus,
    devices: Vec<DeviceStatus>,
}

#[derive(Debug, Serialize)]
struct AdbStatus {
    path: Option<String>,
    available: bool,
    version: Option<String>,
    mdns_available: Option<bool>,
    error: Option<String>,
    mdns_error: Option<String>,
    devices_error: Option<String>,
}

#[derive(Debug, Serialize)]
struct ToolStatus {
    path: Option<String>,
    available: bool,
    error: Option<String>,
}

#[derive(Debug, Serialize)]
struct DeviceStatus {
    serial: String,
    state: String,
    display_name: String,
    product: Option<String>,
    model: Option<String>,
    device: Option<String>,
    transport_id: Option<String>,
}

impl Args {
    fn scrcpy_launch_mode(&self) -> ScrcpyLaunchMode {
        if self.background {
            ScrcpyLaunchMode::Background
        } else if self.foreground {
            ScrcpyLaunchMode::Foreground
        } else if self.stable {
            ScrcpyLaunchMode::Background
        } else {
            ScrcpyLaunchMode::Menu
        }
    }

    fn scrcpy_options(&self) -> ScrcpyOptions {
        ScrcpyOptions {
            borderless: !self.plain_window,
            always_on_top: self.always_on_top,
            window_title: self.window_title.clone(),
            window_width: self.window_width,
            window_height: self.window_height,
            ..ScrcpyOptions::default()
        }
    }

    fn watch_enabled(&self) -> bool {
        self.watch || self.stable
    }

    fn keep_screen_awake_enabled(&self) -> bool {
        self.keep_screen_awake || self.stable
    }

    fn wifi_doctor_enabled(&self) -> bool {
        self.wifi_doctor || self.stable
    }

    fn keepalive_interval(&self) -> Duration {
        Duration::from_secs(self.keepalive_interval.max(1))
    }

    fn keepalive_failures(&self) -> u8 {
        self.keepalive_failures.max(1)
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) if ui::is_cancelled(&error) => ExitCode::SUCCESS,
        Err(error) => {
            ui::error(format!("{error:#}"));
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args = Args::parse();

    if let Some(command) = args.command.as_ref() {
        return handle_cli_command(command, &args);
    }

    let timeout = Duration::from_secs(args.timeout);

    ui::title("awb", "Android Wireless Bridge");
    ui::status("Checking ADB...");
    let adb = Adb::resolve(args.adb.clone())?;

    if args.reset_adb {
        reset_adb_server(&adb)?;
        adb.version()
            .context("ADB still did not answer after restarting the server")?;
    } else {
        ensure_adb_ready(&adb)?;
    }

    ensure_adb_mdns_ready(&adb)?;

    let phone = if let Some(serial) = args.device_serial.as_deref() {
        connected_phone_for_serial(&adb, serial)?
    } else if args.connect_only {
        match connect_only_phone(&adb, timeout)? {
            Some(phone) => phone,
            None => return Ok(()),
        }
    } else {
        match startup_device_choice(&adb)? {
            StartupDeviceChoice::Connected(phone) => phone,
            StartupDeviceChoice::PairNew => retrying_pairing_flow(&adb, timeout)?,
            StartupDeviceChoice::Close => return Ok(()),
        }
    };

    ui::success(format!("Connected to {}", phone.display_name));
    let action = connected_phone_action(&args)?;
    prepare_connected_phone(&adb, &phone, &args, action);
    handle_connected_phone(&adb, &phone, &args, action)
}

fn ensure_adb_ready(adb: &Adb) -> Result<()> {
    match adb.version() {
        Ok(_) => Ok(()),
        Err(error) => {
            ui::warn(format!("ADB did not answer cleanly: {error:#}"));
            reset_adb_server(adb)?;
            adb.version()
                .context("ADB still did not answer after restarting the server")?;
            Ok(())
        }
    }
}

fn ensure_adb_mdns_ready(adb: &Adb) -> Result<()> {
    ui::status("Checking ADB mDNS...");
    match check_adb_mdns(adb) {
        Ok(()) => Ok(()),
        Err(error) if adb::error_is_timeout(&error) => {
            ui::warn(format!(
                "ADB mDNS check timed out: {error:#}. Restarting ADB server once..."
            ));
            reset_adb_server(adb)?;

            match check_adb_mdns(adb) {
                Ok(()) => Ok(()),
                Err(error) if adb::error_is_timeout(&error) => {
                    ui::warn(format!(
                        "ADB mDNS still timed out after restart: {error:#}. Continuing with manual and Bonjour fallbacks."
                    ));
                    Ok(())
                }
                Err(error) => {
                    ui::warn(format!(
                        "could not run adb mdns check after restart: {error:#}"
                    ));
                    Ok(())
                }
            }
        }
        Err(error) => {
            ui::warn(format!("could not run adb mdns check: {error:#}"));
            Ok(())
        }
    }
}

fn check_adb_mdns(adb: &Adb) -> Result<()> {
    match adb.mdns_check() {
        Ok(output) if output.status.success() => Ok(()),
        Ok(output) => {
            ui::warn(format!(
                "adb mdns check reported a problem: {}",
                output.combined_output()
            ));
            Ok(())
        }
        Err(error) => Err(error),
    }
}

fn handle_cli_command(command: &CliCommand, args: &Args) -> Result<()> {
    match command {
        CliCommand::Status(status_args) => print_status(args, status_args),
        CliCommand::ResetAdb => {
            let adb = Adb::resolve(args.adb.clone())?;
            reset_adb_server(&adb)
        }
        CliCommand::Completions(args) => print_completions(args),
        CliCommand::Tray => launch_tray(),
    }
}

fn launch_tray() -> Result<()> {
    let tray_path = env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(TRAY_BINARY_NAME)))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from(TRAY_BINARY_NAME));

    let child = ProcessCommand::new(&tray_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .with_context(|| {
            format!(
                "could not launch {}. Install it next to awb or put it on PATH",
                tray_path.display()
            )
        })?;

    ui::success(format!(
        "Launched the awb menu bar app (pid {}).",
        child.id()
    ));
    Ok(())
}

fn print_status(args: &Args, status_args: &StatusArgs) -> Result<()> {
    let snapshot = collect_status(args);

    if status_args.json {
        println!("{}", serde_json::to_string_pretty(&snapshot)?);
        return Ok(());
    }

    ui::title("awb", "Local status");
    print_tool_status(
        "ADB",
        &snapshot.adb.path,
        snapshot.adb.available,
        &snapshot.adb.error,
    );

    if let Some(mdns_available) = snapshot.adb.mdns_available {
        if mdns_available {
            ui::success("ADB mDNS is available.");
        } else if let Some(error) = &snapshot.adb.mdns_error {
            ui::warn(format!("ADB mDNS is unavailable: {error}"));
        }
    }

    print_tool_status(
        "scrcpy",
        &snapshot.scrcpy.path,
        snapshot.scrcpy.available,
        &snapshot.scrcpy.error,
    );

    match snapshot.devices.as_slice() {
        [] if snapshot.adb.devices_error.is_none() => ui::status("No connected ADB devices."),
        [] => {}
        devices => {
            ui::status("Connected ADB devices:");
            for device in devices {
                println!("  {} ({})", device.display_name, device.state);
            }
        }
    }

    if let Some(error) = &snapshot.adb.devices_error {
        ui::warn(format!("could not list devices: {error}"));
    }

    Ok(())
}

fn print_tool_status(name: &str, path: &Option<String>, available: bool, error: &Option<String>) {
    match (available, path, error) {
        (true, Some(path), _) => ui::success(format!("{name}: {path}")),
        (true, None, _) => ui::success(format!("{name}: available")),
        (false, _, Some(error)) => ui::warn(format!("{name}: {error}")),
        (false, _, None) => ui::warn(format!("{name}: unavailable")),
    }
}

fn collect_status(args: &Args) -> StatusSnapshot {
    let mut adb_status = AdbStatus {
        path: None,
        available: false,
        version: None,
        mdns_available: None,
        error: None,
        mdns_error: None,
        devices_error: None,
    };
    let mut devices = Vec::new();

    match Adb::resolve(args.adb.clone()) {
        Ok(adb) => {
            adb_status.path = Some(adb.path().display().to_string());

            match adb.version() {
                Ok(output) => {
                    adb_status.available = true;
                    adb_status.version = first_non_empty_line(&output.combined_output());
                }
                Err(error) => adb_status.error = Some(format!("{error:#}")),
            }

            if adb_status.available {
                let (mdns_available, mdns_error) = collect_adb_mdns_status(&adb);
                adb_status.mdns_available = mdns_available;
                adb_status.mdns_error = mdns_error;

                match adb.devices() {
                    Ok(adb_devices) => {
                        devices = adb_devices.into_iter().map(DeviceStatus::from).collect();
                    }
                    Err(error) => adb_status.devices_error = Some(format!("{error:#}")),
                }
            }
        }
        Err(error) => adb_status.error = Some(format!("{error:#}")),
    }

    StatusSnapshot {
        awb_version: env!("CARGO_PKG_VERSION"),
        adb: adb_status,
        scrcpy: collect_scrcpy_status(args),
        devices,
    }
}

fn collect_adb_mdns_status(adb: &Adb) -> (Option<bool>, Option<String>) {
    match adb.mdns_check() {
        Ok(output) if output.status.success() => (Some(true), None),
        Ok(output) => (
            Some(false),
            Some(fallback_status_message(&output.combined_output())),
        ),
        Err(error) => (Some(false), Some(format!("{error:#}"))),
    }
}

fn collect_scrcpy_status(args: &Args) -> ToolStatus {
    match Scrcpy::resolve(args.scrcpy.clone(), args.no_scrcpy_check) {
        Ok(scrcpy) => ToolStatus {
            path: Some(scrcpy.path().display().to_string()),
            available: true,
            error: None,
        },
        Err(error) => ToolStatus {
            path: None,
            available: false,
            error: Some(format!("{error:#}")),
        },
    }
}

fn first_non_empty_line(value: &str) -> Option<String> {
    value
        .lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(ToString::to_string)
}

fn fallback_status_message(output: &str) -> String {
    let output = output.trim();

    if output.is_empty() {
        "command exited without output".to_string()
    } else {
        output.to_string()
    }
}

impl From<adb::AdbDevice> for DeviceStatus {
    fn from(device: adb::AdbDevice) -> Self {
        let state = match &device.state {
            adb::DeviceState::Device => "device",
            adb::DeviceState::Offline => "offline",
            adb::DeviceState::Unauthorized => "unauthorized",
            adb::DeviceState::Other(value) => value,
        }
        .to_string();

        Self {
            serial: device.serial.clone(),
            state,
            display_name: device.display_name(),
            product: device.product,
            model: device.model,
            device: device.device,
            transport_id: device.transport_id,
        }
    }
}

fn print_completions(args: &CompletionArgs) -> Result<()> {
    let mut command = Args::command().bin_name(BINARY_NAME);
    let mut stdout = io::stdout();

    match args.shell {
        CompletionShell::Zsh => {
            clap_complete::generate(Shell::Zsh, &mut command, BINARY_NAME, &mut stdout);
        }
    }

    Ok(())
}

fn prepare_connected_phone(
    adb: &Adb,
    phone: &ConnectedPhone,
    args: &Args,
    action: ConnectedAction,
) {
    if should_request_stay_awake(args, action) {
        match adb.stay_awake(&phone.serial) {
            Ok(()) => ui::success("Requested Android stay-awake mode."),
            Err(error) => ui::warn(format!(
                "could not enable Android stay-awake mode: {error:#}"
            )),
        }
    }

    if args.wifi_doctor_enabled() {
        let mut last_wifi_status = None;
        report_wifi_status(adb, &phone.serial, &mut last_wifi_status);
    }
}

fn handle_connected_phone(
    adb: &Adb,
    phone: &ConnectedPhone,
    args: &Args,
    action: ConnectedAction,
) -> Result<()> {
    if args.watch_enabled() {
        return match action {
            ConnectedAction::StartBackground => {
                watch_connected_phone(adb, phone, args, Some(ScrcpyRunMode::Background))
            }
            ConnectedAction::StartForeground => {
                watch_connected_phone(adb, phone, args, Some(ScrcpyRunMode::Foreground))
            }
            ConnectedAction::Close => Ok(()),
        };
    }

    match action {
        ConnectedAction::StartBackground => start_scrcpy_background(phone, args),
        ConnectedAction::StartForeground => start_scrcpy_foreground(phone, args),
        ConnectedAction::Close => Ok(()),
    }
}

fn should_request_stay_awake(args: &Args, action: ConnectedAction) -> bool {
    args.keep_screen_awake_enabled() || action.starts_scrcpy()
}

fn connected_phone_action(args: &Args) -> Result<ConnectedAction> {
    if args.connect_only {
        return Ok(ConnectedAction::Close);
    }

    match args.scrcpy_launch_mode() {
        ScrcpyLaunchMode::Background => return Ok(ConnectedAction::StartBackground),
        ScrcpyLaunchMode::Foreground => return Ok(ConnectedAction::StartForeground),
        ScrcpyLaunchMode::Menu => {}
    }

    let background_label = if args.watch_enabled() {
        "Start scrcpy and keep watching ADB"
    } else {
        "Start scrcpy and close awb"
    };

    match ui::menu_with_default(
        &[background_label, "Start scrcpy and wait", "Close"],
        1,
        Duration::from_secs(5),
    )? {
        1 => Ok(ConnectedAction::StartBackground),
        2 => Ok(ConnectedAction::StartForeground),
        3 => Ok(ConnectedAction::Close),
        _ => unreachable!("ui::menu only returns a valid option"),
    }
}

fn connect_only_phone(adb: &Adb, timeout: Duration) -> Result<Option<ConnectedPhone>> {
    ui::status("Checking for already-connected phones...");
    let ready_phones = wait_for_startup_connected_phones(adb, Duration::from_secs(2))?;

    match ready_phones.len() {
        0 => retrying_pairing_flow(adb, timeout).map(Some),
        1 => {
            let phone = ready_phones[0].clone();
            ui::success(format!(
                "ADB is already connected to {}.",
                phone.display_name
            ));
            Ok(Some(phone))
        }
        count => {
            ui::success(format!("ADB is already connected to {count} devices."));
            Ok(None)
        }
    }
}

fn start_scrcpy_background(phone: &ConnectedPhone, args: &Args) -> Result<()> {
    let scrcpy = resolve_scrcpy(args)?;
    let pid = scrcpy.launch_background(&phone.serial, &args.scrcpy_options())?;
    ui::success(format!("Started scrcpy (pid {pid}); awb can close now."));
    Ok(())
}

fn start_scrcpy_foreground(phone: &ConnectedPhone, args: &Args) -> Result<()> {
    let scrcpy = resolve_scrcpy(args)?;
    scrcpy.launch(&phone.serial, &args.scrcpy_options())
}

fn watch_connected_phone(
    adb: &Adb,
    phone: &ConnectedPhone,
    args: &Args,
    scrcpy_mode: Option<ScrcpyRunMode>,
) -> Result<()> {
    ui::section(
        "Watch mode",
        [
            "Sending ADB keepalives to detect stale wireless transports.",
            "When the device drops, awb tries adb reconnect and mDNS endpoints.",
            "Press either ⌃ + C, ESC, C or X to stop watching.",
        ],
    );

    let scrcpy = if scrcpy_mode.is_some() {
        Some(resolve_scrcpy(args)?)
    } else {
        None
    };
    let scrcpy_options = args.scrcpy_options();
    let mut serial = phone.serial.clone();
    let mut child = match (&scrcpy, scrcpy_mode) {
        (Some(scrcpy), Some(mode)) => Some(spawn_supervised_scrcpy(
            scrcpy,
            &serial,
            &scrcpy_options,
            mode,
        )?),
        _ => None,
    };
    let mut failed_keepalives = 0;
    let mut last_wifi_status = None;

    loop {
        if let Some(scrcpy_child) = child.as_mut() {
            if let Some(status) = scrcpy_child
                .try_wait()
                .context("failed to check scrcpy status")?
            {
                ui::warn(format!(
                    "scrcpy exited with status {status}; it will restart when ADB is ready."
                ));
                child = None;
            }
        }

        match adb.keepalive(&serial) {
            Ok(()) => {
                if failed_keepalives > 0 {
                    ui::success("ADB keepalive recovered.");
                }

                failed_keepalives = 0;

                if args.wifi_doctor_enabled() {
                    report_wifi_status(adb, &serial, &mut last_wifi_status);
                }

                if child.is_none() {
                    if let (Some(scrcpy), Some(mode)) = (&scrcpy, scrcpy_mode) {
                        child = Some(spawn_supervised_scrcpy(
                            scrcpy,
                            &serial,
                            &scrcpy_options,
                            mode,
                        )?);
                    }
                }
            }
            Err(error) => {
                failed_keepalives += 1;
                ui::warn(format!(
                    "ADB keepalive failed ({failed_keepalives}/{}): {error:#}",
                    args.keepalive_failures()
                ));

                if failed_keepalives >= args.keepalive_failures() {
                    match reconnect_watched_phone(adb, &serial) {
                        Ok(reconnected) => {
                            serial = reconnected.serial;
                            failed_keepalives = 0;
                            last_wifi_status = None;
                            ui::success(format!(
                                "Watch mode reconnected to {}",
                                reconnected.display_name
                            ));
                        }
                        Err(reconnect_error) => {
                            ui::warn(format!("automatic reconnect failed: {reconnect_error:#}"));
                        }
                    }
                }
            }
        }

        ui::sleep_or_cancel(args.keepalive_interval())?;
    }
}

fn spawn_supervised_scrcpy(
    scrcpy: &Scrcpy,
    serial: &str,
    options: &ScrcpyOptions,
    mode: ScrcpyRunMode,
) -> Result<Child> {
    let child = scrcpy.spawn(serial, options, mode)?;
    ui::success(format!("Started supervised scrcpy (pid {}).", child.id()));
    Ok(child)
}

fn reconnect_watched_phone(adb: &Adb, current_serial: &str) -> Result<ConnectedPhone> {
    ui::status("Trying to reconnect wireless ADB...");
    let _ = adb.reconnect_offline();

    if let Some(phone) = ready_phone_matching(adb, current_serial)? {
        return Ok(phone);
    }

    let timeout = Duration::from_secs(10);
    let baseline_devices = HashSet::new();

    if is_plausible_endpoint(current_serial) {
        if let Ok(device) = connect_to_endpoint(adb, current_serial, &baseline_devices, timeout) {
            return Ok(ConnectedPhone {
                serial: device.serial.clone(),
                display_name: device.display_name(),
            });
        }
    }

    for endpoint in reconnect_endpoints(adb, current_serial) {
        match connect_to_endpoint(adb, &endpoint, &baseline_devices, timeout) {
            Ok(device) => {
                return Ok(ConnectedPhone {
                    serial: device.serial.clone(),
                    display_name: device.display_name(),
                });
            }
            Err(error) => ui::warn(format!("reconnect endpoint {endpoint} failed: {error:#}")),
        }
    }

    bail!("no reconnectable wireless debugging endpoint was found")
}

fn ready_phone_matching(adb: &Adb, expected_serial: &str) -> Result<Option<ConnectedPhone>> {
    let baseline_devices = HashSet::new();
    let devices = adb.devices()?;

    Ok(
        adb::matching_ready_device(&devices, expected_serial, &baseline_devices).map(|device| {
            ConnectedPhone {
                serial: device.serial.clone(),
                display_name: device.display_name(),
            }
        }),
    )
}

fn connected_phone_for_serial(adb: &Adb, serial: &str) -> Result<ConnectedPhone> {
    ready_phone_matching(adb, serial)?.with_context(|| {
        format!("ADB device {serial} is not connected or is not in the ready device state")
    })
}

fn reconnect_endpoints(adb: &Adb, current_serial: &str) -> Vec<String> {
    let host = adb::endpoint_host(current_serial);
    let services = adb.mdns_services().unwrap_or_default();
    let connect_endpoints: Vec<String> = services
        .iter()
        .filter(|service| service.is_connect_service())
        .map(|service| service.address.clone())
        .collect();

    let same_host: Vec<String> = connect_endpoints
        .iter()
        .filter(|endpoint| adb::endpoint_host(endpoint) == host)
        .cloned()
        .collect();

    if same_host.is_empty() {
        connect_endpoints
    } else {
        same_host
    }
}

fn report_wifi_status(adb: &Adb, serial: &str, last_status: &mut Option<String>) {
    match adb.wifi_status(serial) {
        Ok(status) if last_status.as_deref() != Some(status.as_str()) => {
            ui::status(format!("Wi-Fi: {status}"));
            *last_status = Some(status);
        }
        Ok(_) => {}
        Err(error) => ui::warn(format!("could not read Wi-Fi status: {error:#}")),
    }
}

fn resolve_scrcpy(args: &Args) -> Result<Scrcpy> {
    Scrcpy::resolve(args.scrcpy.clone(), args.no_scrcpy_check)
        .context("scrcpy was not found. Install scrcpy, then try again")
}

fn startup_device_choice(adb: &Adb) -> Result<StartupDeviceChoice> {
    ui::status("Checking for already-connected phones...");
    match startup_device_choice_once(adb) {
        Ok(choice) => Ok(choice),
        Err(error) if adb::error_is_timeout(&error) => {
            ui::warn(format!(
                "ADB device listing timed out: {error:#}. Restarting ADB server once..."
            ));
            reset_adb_server(adb)?;
            startup_device_choice_once(adb)
                .context("ADB devices still did not answer after restarting the server")
        }
        Err(error) => Err(error),
    }
}

fn startup_device_choice_once(adb: &Adb) -> Result<StartupDeviceChoice> {
    let ready_phones = wait_for_startup_connected_phones(adb, Duration::from_secs(2))?;

    match ready_phones.len() {
        0 => Ok(StartupDeviceChoice::PairNew),
        1 => {
            let phone = ready_phones[0].clone();
            ui::success(format!(
                "ADB is already connected to {}.",
                phone.display_name
            ));
            Ok(StartupDeviceChoice::Connected(phone))
        }
        _ => {
            ui::status("ADB is already connected to multiple devices.");

            let mut options: Vec<String> = ready_phones
                .iter()
                .map(|phone| format!("Use {}", phone.display_name))
                .collect();
            options.push("Pair a new phone".to_string());
            options.push("Close".to_string());

            let option_refs: Vec<&str> = options.iter().map(String::as_str).collect();
            let selected = ui::menu(&option_refs)?;

            if selected <= ready_phones.len() {
                return Ok(StartupDeviceChoice::Connected(
                    ready_phones[selected - 1].clone(),
                ));
            }

            if selected == ready_phones.len() + 1 {
                return Ok(StartupDeviceChoice::PairNew);
            }

            Ok(StartupDeviceChoice::Close)
        }
    }
}

fn wait_for_startup_connected_phones(adb: &Adb, timeout: Duration) -> Result<Vec<ConnectedPhone>> {
    let deadline = Instant::now() + timeout;

    loop {
        let ready_phones = ready_connected_phones(adb)?;

        if !ready_phones.is_empty() || Instant::now() >= deadline {
            return Ok(ready_phones);
        }

        thread::sleep(Duration::from_millis(250));
    }
}

fn ready_connected_phones(adb: &Adb) -> Result<Vec<ConnectedPhone>> {
    let mut devices = adb.devices()?;
    let services = if devices
        .iter()
        .any(|device| device.state == adb::DeviceState::Device)
    {
        adb.mdns_services().unwrap_or_default()
    } else {
        Vec::new()
    };

    if disconnect_duplicate_wireless_aliases(adb, &devices, &services) {
        devices = adb.devices().unwrap_or(devices);
    }

    Ok(connected_phones_from_devices(devices, &services))
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

fn disconnect_duplicate_wireless_aliases(
    adb: &Adb,
    devices: &[adb::AdbDevice],
    services: &[adb::MdnsService],
) -> bool {
    let aliases = adb::duplicate_wireless_endpoint_aliases(devices, services);

    for alias in &aliases {
        ui::status(format!("Removing duplicate ADB transport {alias}..."));

        match adb.disconnect(alias) {
            Ok(_) => ui::success(format!("Removed duplicate ADB transport {alias}.")),
            Err(error) => ui::warn(format!(
                "could not remove duplicate ADB transport {alias}: {error:#}"
            )),
        }
    }

    !aliases.is_empty()
}

fn reset_adb_server(adb: &Adb) -> Result<()> {
    ui::status("Resetting local ADB server...");
    adb.reset_server()?;
    ui::success("ADB server restarted.");
    Ok(())
}

fn retrying_pairing_flow(adb: &Adb, timeout: Duration) -> Result<ConnectedPhone> {
    loop {
        match pair_and_connect(adb, timeout) {
            Ok(phone) => return Ok(phone),
            Err(error) => {
                if ui::is_cancelled(&error) {
                    ui::success("Pairing cancelled.");
                } else {
                    ui::error(format!("{error:#}"));
                }

                match ui::menu(&[
                    "Retry QR pairing",
                    "Enter phone IP:port manually",
                    "Reset ADB server and retry",
                    "Pair with pairing code",
                    "Close",
                ])? {
                    1 => continue,
                    2 => match manual_connect_flow(adb, timeout) {
                        Ok(phone) => return Ok(phone),
                        Err(error) if ui::is_cancelled(&error) => return Err(error),
                        Err(error) => {
                            ui::error(format!("{error:#}"));
                            continue;
                        }
                    },
                    3 => {
                        reset_adb_server(adb)?;
                        continue;
                    }
                    4 => match pairing_code_flow(adb, timeout) {
                        Ok(phone) => return Ok(phone),
                        Err(error) if ui::is_cancelled(&error) => return Err(error),
                        Err(error) => {
                            ui::error(format!("{error:#}"));
                            continue;
                        }
                    },
                    5 => return Err(ui::cancelled()),
                    _ => unreachable!("ui::menu only returns a valid option"),
                }
            }
        }
    }
}

fn manual_connect_flow(adb: &Adb, timeout: Duration) -> Result<ConnectedPhone> {
    let baseline_devices = adb::ready_device_serials(&adb.devices().unwrap_or_default());
    let device = manual_connect_device(adb, &baseline_devices, timeout)?;

    Ok(ConnectedPhone {
        serial: device.serial.clone(),
        display_name: device.display_name(),
    })
}

fn manual_connect_device(
    adb: &Adb,
    baseline_devices: &HashSet<String>,
    timeout: Duration,
) -> Result<adb::AdbDevice> {
    ui::section(
        "Manual connection",
        [
            "On your Android phone, go back to the main Wireless debugging screen.",
            "Copy the value shown as \"IP address & Port\".",
        ],
    );

    let endpoint = prompt_endpoint("Enter phone IP:port")?;
    connect_to_endpoint(adb, &endpoint, baseline_devices, timeout)
}

fn pairing_code_flow(adb: &Adb, timeout: Duration) -> Result<ConnectedPhone> {
    ui::section(
        "Pair with pairing code",
        [
            "On your Android phone, go to Developer options -> Wireless debugging.",
            "Tap \"Pair device with pairing code\".",
            "Enter the pairing IP:port and pairing code shown on the phone.",
        ],
    );

    let pairing_endpoint = prompt_endpoint("Enter pairing IP:port")?;
    let pairing_code = ui::prompt_required("Enter pairing code")?;

    ui::status(format!("Pairing with {pairing_endpoint}..."));
    adb.pair(&pairing_endpoint, &pairing_code)?;

    ui::success("Pairing succeeded.");
    ui::section(
        "Connect paired phone",
        [
            "Close the pairing-code dialog on the phone.",
            "On the main Wireless debugging screen, copy \"IP address & Port\".",
        ],
    );

    let connect_endpoint = prompt_endpoint("Enter phone IP:port")?;
    let baseline_devices = adb::ready_device_serials(&adb.devices().unwrap_or_default());
    let device = connect_to_endpoint(adb, &connect_endpoint, &baseline_devices, timeout)?;

    Ok(ConnectedPhone {
        serial: device.serial.clone(),
        display_name: device.display_name(),
    })
}

fn prompt_endpoint(label: &str) -> Result<String> {
    loop {
        let endpoint = ui::prompt_required(label)?;

        if is_plausible_endpoint(&endpoint) {
            return Ok(endpoint);
        }

        ui::warn("Use the full value shown on the phone, for example 192.168.68.54:37123.");
    }
}

fn is_plausible_endpoint(endpoint: &str) -> bool {
    let endpoint = endpoint.trim();

    if endpoint.is_empty() || endpoint.contains(char::is_whitespace) {
        return false;
    }

    let Some((_host, port)) = endpoint.rsplit_once(':') else {
        return false;
    };

    port.parse::<u16>().is_ok()
}

fn connect_to_endpoint(
    adb: &Adb,
    endpoint: &str,
    baseline_devices: &HashSet<String>,
    timeout: Duration,
) -> Result<adb::AdbDevice> {
    ui::status(format!("Connecting to {endpoint}..."));
    let output = adb.connect(endpoint)?;
    let expected_serial = adb::connect_serial_from_output(&output.combined_output())
        .unwrap_or_else(|| endpoint.to_string());

    ui::status("Verifying the device is ready...");
    wait_for_ready_device(adb, &expected_serial, baseline_devices, timeout)
}

fn wait_for_ready_device(
    adb: &Adb,
    expected_serial: &str,
    baseline_devices: &HashSet<String>,
    timeout: Duration,
) -> Result<adb::AdbDevice> {
    let deadline = Instant::now() + timeout;
    ui::status("Waiting for adb devices...");
    ui::status(ui::CANCEL_HINT);
    let mut countdown = ui::Countdown::new("ADB device wait");

    loop {
        countdown.tick(remaining_until(deadline))?;

        let mut devices = adb.devices().unwrap_or_default();
        let services = adb.mdns_services().unwrap_or_default();

        if disconnect_duplicate_wireless_aliases(adb, &devices, &services) {
            devices = adb.devices().unwrap_or(devices);
        }

        if let Some(device) = matching_ready_device_from_snapshot(
            &devices,
            expected_serial,
            baseline_devices,
            &services,
        ) {
            countdown.finish();
            ui::success(format!("ADB device is ready: {}", device.display_name()));
            return Ok(device);
        }

        if Instant::now() >= deadline {
            countdown.finish();
            bail!("timed out waiting for {expected_serial} to appear in adb devices");
        }

        if let Err(error) = ui::sleep_or_cancel(poll_delay(deadline, Duration::from_secs(2))) {
            countdown.finish();
            return Err(error);
        }
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

fn pair_and_connect(adb: &Adb, timeout: Duration) -> Result<ConnectedPhone> {
    let baseline_services = adb::connect_services(&adb.mdns_services().unwrap_or_default());
    let baseline_devices = adb::ready_device_serials(&adb.devices().unwrap_or_default());
    let qr = PairingQr::generate();

    ui::section(
        "Pair with QR code",
        [
            "On your Android phone, go to Developer options -> Wireless debugging.",
            "Tap \"Pair device with QR code\".",
            "Scan the QR code below.",
        ],
    );
    ui::print_qr(&qr.render_terminal()?);
    ui::blank_line();
    ui::status(ui::CANCEL_HINT);

    let pairing_address = match wait_for_pairing_endpoint(adb, &qr.instance, timeout)? {
        PairingWaitOutcome::PairingEndpoint(pairing_address) => pairing_address,
        PairingWaitOutcome::AlreadyConnected(phone) => return Ok(phone),
    };

    ui::success("Phone found. Completing ADB pairing...");
    adb.pair(&pairing_address, &qr.secret)?;

    ui::status("Looking for the wireless debugging connection endpoint...");
    let device = connect_and_wait_for_device(
        adb,
        &pairing_address,
        &baseline_services,
        &baseline_devices,
        timeout,
    )?;

    Ok(ConnectedPhone {
        serial: device.serial.clone(),
        display_name: device.display_name(),
    })
}

fn wait_for_pairing_endpoint(
    adb: &Adb,
    instance: &str,
    timeout: Duration,
) -> Result<PairingWaitOutcome> {
    let deadline = Instant::now() + timeout;
    let mut countdown = ui::Countdown::new("Waiting for QR scan");
    let mut reported_direct_check = false;
    let mut reported_device_check_error = false;
    let mut reported_adb_error = false;
    let mut reported_bonjour_error = false;
    let mut offered_multiple_existing_devices = false;

    loop {
        countdown.tick(remaining_until(deadline))?;

        let devices_result = adb.devices();
        let services_result = adb.mdns_services();
        let services = services_result.as_ref().map(Vec::as_slice).unwrap_or(&[]);

        match devices_result {
            Ok(mut devices) => {
                if disconnect_duplicate_wireless_aliases(adb, &devices, services) {
                    devices = adb.devices().unwrap_or(devices);
                }

                let ready_phones = connected_phones_from_devices(devices, services);

                if !ready_phones.is_empty() {
                    countdown.finish();
                }

                if let Some(phone) = already_connected_phone_choice(
                    ready_phones,
                    &mut offered_multiple_existing_devices,
                )? {
                    countdown.finish();
                    return Ok(PairingWaitOutcome::AlreadyConnected(phone));
                }
            }
            Err(error) if !reported_device_check_error => {
                countdown.finish();
                ui::warn(format!("could not check existing ADB devices: {error:#}"));
                reported_device_check_error = true;
            }
            Err(_) => {}
        }

        match services_result {
            Ok(services) => {
                if let Some(service) = services
                    .into_iter()
                    .find(|service| service.instance == instance && service.is_pairing_service())
                {
                    countdown.finish();
                    return Ok(PairingWaitOutcome::PairingEndpoint(service.address));
                }
            }
            Err(error) if !reported_adb_error => {
                countdown.finish();
                ui::warn(format!("adb mDNS lookup failed: {error:#}"));
                reported_adb_error = true;
            }
            Err(_) => {}
        }

        if !reported_direct_check {
            countdown.finish();
            ui::status("Also checking macOS Bonjour directly for the QR pairing service...");
            reported_direct_check = true;
        }

        match dnssd::discover_pairing_endpoint(instance, Duration::from_secs(2)) {
            Ok(Some(endpoint)) => {
                countdown.finish();
                ui::success("Phone found through macOS Bonjour.");
                return Ok(PairingWaitOutcome::PairingEndpoint(endpoint));
            }
            Ok(None) => {}
            Err(error) if !reported_bonjour_error => {
                countdown.finish();
                ui::warn(format!("Bonjour pairing lookup failed: {error:#}"));
                reported_bonjour_error = true;
            }
            Err(_) => {}
        }

        if Instant::now() >= deadline {
            countdown.finish();
            bail!(
                "timed out waiting for the phone to advertise the QR pairing service `{instance}`"
            );
        }

        if let Err(error) = ui::sleep_or_cancel(poll_delay(deadline, Duration::from_millis(500))) {
            countdown.finish();
            return Err(error);
        }
    }
}

fn already_connected_phone_choice(
    ready_phones: Vec<ConnectedPhone>,
    offered_multiple_existing_devices: &mut bool,
) -> Result<Option<ConnectedPhone>> {
    match ready_phones.len() {
        0 => Ok(None),
        1 => {
            let phone = ready_phones[0].clone();
            ui::success(format!(
                "ADB already sees {}; skipping QR scan.",
                phone.display_name
            ));
            Ok(Some(phone))
        }
        _ if *offered_multiple_existing_devices => Ok(None),
        _ => {
            ui::status("ADB already sees multiple ready devices.");

            let mut options: Vec<String> = ready_phones
                .iter()
                .map(|phone| format!("Use {}", phone.display_name))
                .collect();
            options.push("Keep waiting for QR scan".to_string());

            let option_refs: Vec<&str> = options.iter().map(String::as_str).collect();
            let selected = ui::menu(&option_refs)?;
            *offered_multiple_existing_devices = true;

            if selected <= ready_phones.len() {
                Ok(Some(ready_phones[selected - 1].clone()))
            } else {
                Ok(None)
            }
        }
    }
}

fn connect_and_wait_for_device(
    adb: &Adb,
    pairing_address: &str,
    baseline_services: &HashSet<adb::MdnsService>,
    baseline_devices: &HashSet<String>,
    timeout: Duration,
) -> Result<adb::AdbDevice> {
    let deadline = Instant::now() + timeout;
    let mut countdown = ui::Countdown::new("Connection endpoint wait");
    let mut expected_serial = pairing_address.to_string();
    let mut announced_endpoints = HashSet::new();
    let mut reported_waiting_for_endpoint = false;
    let mut attempt = 0;
    let mut last_candidate_summary = String::new();
    let mut last_bonjour_check = None;
    let mut reported_connect_mdns_error = false;

    loop {
        countdown.tick(remaining_until(deadline))?;

        let mut ready_devices = adb.devices().unwrap_or_default();
        let services = match adb.mdns_services() {
            Ok(services) => services,
            Err(error) => {
                if !reported_connect_mdns_error {
                    countdown.finish();
                    ui::warn(format!("adb mDNS connect lookup failed: {error:#}"));
                    reported_connect_mdns_error = true;
                }

                Vec::new()
            }
        };
        let candidates =
            adb::connect_service_candidates(&services, pairing_address, baseline_services);

        if disconnect_duplicate_wireless_aliases(adb, &ready_devices, &services) {
            ready_devices = adb.devices().unwrap_or(ready_devices);
        }

        if let Some(device) = matching_ready_device_from_snapshot(
            &ready_devices,
            &expected_serial,
            baseline_devices,
            &services,
        ) {
            countdown.finish();
            ui::success(format!("ADB device is ready: {}", device.display_name()));
            return Ok(device);
        }

        let ready_device_count = ready_devices
            .iter()
            .filter(|device| device.state == adb::DeviceState::Device)
            .count();

        if ready_device_count > 0 {
            countdown.finish();
            ui::status(format!(
                "ADB sees {ready_device_count} ready device(s), but not the just-paired phone yet."
            ));
        }

        let candidate_summary = endpoint_summary(&candidates);

        for service in &candidates {
            if let Some(device) = adb::matching_ready_device_for_connect_service(
                &ready_devices,
                service,
                baseline_devices,
            ) {
                countdown.finish();
                ui::success(format!(
                    "ADB device is ready through mDNS: {}",
                    device.display_name()
                ));
                return Ok(device);
            }
        }

        if candidates.is_empty() && !reported_waiting_for_endpoint {
            countdown.finish();
            ui::status("Waiting for the phone to advertise its connection endpoint...");
            ui::status(ui::CANCEL_HINT);
            reported_waiting_for_endpoint = true;
        } else if !candidates.is_empty() && candidate_summary != last_candidate_summary {
            countdown.finish();
            ui::status(format!(
                "Connect endpoint candidate(s): {candidate_summary}"
            ));
            last_candidate_summary = candidate_summary;
        }

        if candidates.is_empty() && should_check_bonjour(last_bonjour_check) {
            last_bonjour_check = Some(Instant::now());
            countdown.finish();

            if let Some(device) =
                try_direct_bonjour_connect(adb, pairing_address, baseline_devices, None, timeout)?
            {
                return Ok(device);
            }
        }

        for service in candidates {
            if announced_endpoints.insert(service.address.clone()) {
                countdown.finish();
                ui::status(format!("Connecting to {}...", service.address));
            }

            attempt += 1;
            countdown.finish();
            ui::status(format!(
                "Attempt {attempt}: adb connect {}",
                service.address
            ));

            match adb.connect(&service.address) {
                Ok(output) => {
                    expected_serial = adb::connect_serial_from_output(&output.combined_output())
                        .unwrap_or_else(|| service.address.clone());

                    countdown.finish();
                    ui::status("Verifying the device is ready...");

                    let mut devices = adb.devices().unwrap_or_default();

                    if disconnect_duplicate_wireless_aliases(adb, &devices, &services) {
                        devices = adb.devices().unwrap_or(devices);
                    }

                    if let Some(device) = matching_ready_device_from_snapshot(
                        &devices,
                        &expected_serial,
                        baseline_devices,
                        &services,
                    ) {
                        countdown.finish();
                        return Ok(device);
                    }
                }
                Err(error) => {
                    countdown.finish();
                    ui::warn(format!(
                        "ADB mDNS endpoint {} failed: {error:#}",
                        service.address
                    ));

                    if let Some(device) = try_direct_bonjour_connect(
                        adb,
                        pairing_address,
                        baseline_devices,
                        Some(&service.address),
                        timeout,
                    )? {
                        return Ok(device);
                    }

                    if let Some(device) = try_ui_hierarchy_connect(adb, baseline_devices, timeout)?
                    {
                        return Ok(device);
                    }

                    ui::warn("Automatic discovery did not find a working endpoint.");
                    return manual_connect_device(adb, baseline_devices, timeout);
                }
            }
        }

        if Instant::now() >= deadline {
            countdown.finish();
            if let Some(device) = try_ui_hierarchy_connect(adb, baseline_devices, timeout)? {
                return Ok(device);
            }

            ui::warn(
                "Automatic discovery timed out before finding a connectable wireless debugging endpoint.",
            );
            return manual_connect_device(adb, baseline_devices, timeout);
        }

        if let Err(error) = ui::sleep_or_cancel(poll_delay(deadline, Duration::from_secs(2))) {
            countdown.finish();
            return Err(error);
        }
    }
}

fn try_direct_bonjour_connect(
    adb: &Adb,
    pairing_address: &str,
    baseline_devices: &HashSet<String>,
    skipped_endpoint: Option<&str>,
    timeout: Duration,
) -> Result<Option<adb::AdbDevice>> {
    ui::status("Checking macOS Bonjour directly for wireless debugging endpoints...");

    let endpoints = match dnssd::discover_connect_endpoints(pairing_address, Duration::from_secs(6))
    {
        Ok(endpoints) => endpoints,
        Err(error) => {
            ui::warn(format!("Bonjour connect lookup failed: {error:#}"));
            return Ok(None);
        }
    };

    if endpoints.is_empty() {
        ui::status("No Bonjour connect endpoints found outside ADB.");
        return Ok(None);
    }

    ui::status(format!(
        "Bonjour endpoint candidate(s): {}",
        endpoints.join(", ")
    ));

    let verify_timeout = if timeout < Duration::from_secs(1) {
        Duration::from_secs(1)
    } else {
        timeout.min(Duration::from_secs(8))
    };

    for endpoint in endpoints {
        if skipped_endpoint == Some(endpoint.as_str()) {
            ui::status(format!("Skipping {endpoint}; ADB already tried it."));
            continue;
        }

        match connect_to_endpoint(adb, &endpoint, baseline_devices, verify_timeout) {
            Ok(device) => return Ok(Some(device)),
            Err(error) => ui::warn(format!("Bonjour endpoint {endpoint} failed: {error:#}")),
        }
    }

    Ok(None)
}

fn try_ui_hierarchy_connect(
    adb: &Adb,
    baseline_devices: &HashSet<String>,
    timeout: Duration,
) -> Result<Option<adb::AdbDevice>> {
    let ready_devices: Vec<_> = adb
        .devices()
        .unwrap_or_default()
        .into_iter()
        .filter(|device| device.state == adb::DeviceState::Device)
        .collect();

    if ready_devices.is_empty() {
        ui::status("No existing ADB transport is available for screen parsing.");
        return Ok(None);
    }

    ui::status("Trying to read the visible phone screen through ADB...");
    let mut seen_endpoints = HashSet::new();
    let verify_timeout = if timeout < Duration::from_secs(1) {
        Duration::from_secs(1)
    } else {
        timeout.min(Duration::from_secs(8))
    };

    for device in ready_devices {
        ui::status(format!(
            "Reading UI hierarchy from {}...",
            device.display_name()
        ));

        let hierarchy = match adb.dump_ui_hierarchy(&device.serial) {
            Ok(hierarchy) => hierarchy,
            Err(error) => {
                ui::warn(format!(
                    "Could not read UI hierarchy from {}: {error:#}",
                    device.display_name()
                ));
                continue;
            }
        };

        let endpoints = extract_ipv4_endpoints(&hierarchy);

        if endpoints.is_empty() {
            ui::status(format!(
                "No IP:port text found on {}.",
                device.display_name()
            ));
            continue;
        }

        ui::status(format!(
            "Screen endpoint candidate(s): {}",
            endpoints.join(", ")
        ));

        for endpoint in endpoints {
            if !seen_endpoints.insert(endpoint.clone()) {
                continue;
            }

            match connect_to_endpoint(adb, &endpoint, baseline_devices, verify_timeout) {
                Ok(device) => return Ok(Some(device)),
                Err(error) => ui::warn(format!("Screen endpoint {endpoint} failed: {error:#}")),
            }
        }
    }

    Ok(None)
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
    fn parses_scrcpy_launch_flags() {
        let default_args = Args::try_parse_from(["awb"]).unwrap();
        assert_eq!(default_args.scrcpy_launch_mode(), ScrcpyLaunchMode::Menu);

        let background_args = Args::try_parse_from(["awb", "--background"]).unwrap();
        assert_eq!(
            background_args.scrcpy_launch_mode(),
            ScrcpyLaunchMode::Background
        );

        let foreground_args = Args::try_parse_from(["awb", "--foreground"]).unwrap();
        assert_eq!(
            foreground_args.scrcpy_launch_mode(),
            ScrcpyLaunchMode::Foreground
        );
    }

    #[test]
    fn stable_mode_enables_supervision_defaults() {
        let args = Args::try_parse_from(["awb", "--stable"]).unwrap();

        assert_eq!(args.scrcpy_launch_mode(), ScrcpyLaunchMode::Background);
        assert!(args.watch_enabled());
        assert!(args.keep_screen_awake_enabled());
        assert!(args.wifi_doctor_enabled());
    }

    #[test]
    fn connect_only_closes_without_scrcpy_menu() {
        let args = Args::try_parse_from(["awb", "--connect-only"]).unwrap();

        assert_eq!(args.scrcpy_launch_mode(), ScrcpyLaunchMode::Menu);
        assert_eq!(
            connected_phone_action(&args).unwrap(),
            ConnectedAction::Close
        );
        assert!(Args::try_parse_from(["awb", "--connect-only", "--background"]).is_err());
        assert!(Args::try_parse_from(["awb", "--connect-only", "--stable"]).is_err());
    }

    #[test]
    fn scrcpy_actions_request_stay_awake_by_default() {
        let args = Args::try_parse_from(["awb"]).unwrap();

        assert!(should_request_stay_awake(
            &args,
            ConnectedAction::StartBackground
        ));
        assert!(should_request_stay_awake(
            &args,
            ConnectedAction::StartForeground
        ));
        assert!(!should_request_stay_awake(&args, ConnectedAction::Close));
    }

    #[test]
    fn explicit_keep_screen_awake_applies_without_scrcpy() {
        let args = Args::try_parse_from(["awb", "--keep-screen-awake"]).unwrap();

        assert!(should_request_stay_awake(&args, ConnectedAction::Close));
    }

    #[test]
    fn normalizes_keepalive_settings() {
        let args = Args::try_parse_from([
            "awb",
            "--watch",
            "--keepalive-interval",
            "0",
            "--keepalive-failures",
            "0",
        ])
        .unwrap();

        assert_eq!(args.keepalive_interval(), Duration::from_secs(1));
        assert_eq!(args.keepalive_failures(), 1);
    }

    #[test]
    fn builds_scrcpy_options_from_args() {
        let default_args = Args::try_parse_from(["awb"]).unwrap();
        assert_eq!(default_args.scrcpy_options(), ScrcpyOptions::default());

        let custom_args = Args::try_parse_from([
            "awb",
            "--plain-window",
            "--always-on-top",
            "--window-title",
            "Ovi Pixel",
            "--window-width",
            "443",
            "--window-height",
            "989",
        ])
        .unwrap();

        assert_eq!(
            custom_args.scrcpy_options(),
            ScrcpyOptions {
                borderless: false,
                always_on_top: true,
                window_title: "Ovi Pixel".to_string(),
                window_width: 443,
                window_height: 989,
                ..ScrcpyOptions::default()
            }
        );
    }

    #[test]
    fn parses_explicit_device_serial() {
        let args =
            Args::try_parse_from(["awb", "--device-serial", "R5CT123ABC", "--background"]).unwrap();

        assert_eq!(args.device_serial.as_deref(), Some("R5CT123ABC"));
        assert_eq!(args.scrcpy_launch_mode(), ScrcpyLaunchMode::Background);
    }

    #[test]
    fn rejects_conflicting_scrcpy_launch_flags() {
        assert!(Args::try_parse_from(["awb", "--background", "--foreground"]).is_err());
    }

    #[test]
    fn parses_completions_command() {
        let args = Args::try_parse_from(["awb", "completions", "zsh"]).unwrap();

        match args.command {
            Some(CliCommand::Completions(args)) => {
                assert_eq!(args.shell, CompletionShell::Zsh);
            }
            _ => panic!("expected completions command"),
        }
    }

    #[test]
    fn validates_ipv4_endpoint_shape() {
        assert!(is_plausible_endpoint("192.168.68.54:42209"));
        assert!(is_plausible_endpoint("localhost:5555"));
        assert!(!is_plausible_endpoint("192.168.68.54"));
        assert!(!is_plausible_endpoint("192.168.68.54:notaport"));
        assert!(!is_plausible_endpoint("192.168.68.54:70000"));
        assert!(!is_plausible_endpoint("192.168.68.54:42209 extra"));
    }

    #[test]
    fn extracts_ip_ports_from_ui_hierarchy_text() {
        let hierarchy = r#"
<node text="IP address &amp; Port" />
<node text="192.168.68.54:37197" />
<node bounds="[0,123][456,789]" />
<node text="192.168.68.54:37197." />
"#;

        assert_eq!(
            extract_ipv4_endpoints(hierarchy),
            vec!["192.168.68.54:37197"]
        );
    }

    #[test]
    fn validates_strict_ipv4_endpoint_shape() {
        assert!(is_ipv4_endpoint("192.168.68.54:37197"));
        assert!(!is_ipv4_endpoint("localhost:5555"));
        assert!(!is_ipv4_endpoint("192.168.68.54:0"));
        assert!(!is_ipv4_endpoint("999.168.68.54:37197"));
        assert!(!is_ipv4_endpoint("192.168.68.54:notaport"));
    }
}
