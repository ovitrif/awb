//! Headless drive mode: the real popover UI rendered offscreen and driven over
//! a Unix socket, so agents and scripts can check every screen without a
//! window, a menu bar item, or the user's mouse and keyboard.
//!
//! Start a server with `awb-app --drive <socket>` (build with
//! `--features drive`), then send commands with `awb-app drive <socket>
//! <command>`. One command per line; each gets one reply line, `ok ...` or
//! `error ...`. Coordinates are window points (380 wide, 0 at the top edge
//! including the beak). Commands:
//!
//! - `state`: current screen, tab, transition, theme, pairing phase.
//! - `click X Y`, `hover X Y`, `leave`, `scroll DY` (at the pointer).
//! - `key NAME` with egui key names (`Tab`, `Enter`, `ArrowLeft`, ...);
//!   prefix `shift+` for Shift.
//! - `wait MS`: let time pass, running frames at 60 fps.
//! - `shot PATH`: save the current frame as a PNG with transparency.
//! - `record DIR` / `stop`: save every frame and `times.txt` (seconds).
//! - `theme auto|day|night`, `gradients on|off`.
//! - `quit`.
//!
//! The UI runs in real time, so animations and backend work (adb, emulators,
//! `AWB_MOCK_PAIRING`) behave as in the app.

use std::io::{BufRead, BufReader, ErrorKind, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use anyhow::{Context as _, anyhow, bail};
use eframe::egui::{self, Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;

use crate::app::App;
use crate::config::ThemeMode;
use crate::theme;

const FRAME: Duration = Duration::from_millis(16);
const PIXELS_PER_POINT: f32 = 2.0;

/// Runs the headless app and serves commands on `socket` until `quit`.
pub fn serve(socket: &Path) -> anyhow::Result<()> {
    // Driven settings changes never reach the user's config: always use a
    // throwaway config folder, seeded from `AWB_DRIVE_CONFIG` when given.
    let config_home = std::env::temp_dir().join(format!("awb-drive-{}", std::process::id()));
    std::fs::create_dir_all(config_home.join("awb"))?;
    if let Some(seed) = std::env::var_os("AWB_DRIVE_CONFIG") {
        std::fs::copy(&seed, config_home.join("awb/config.toml"))
            .context("failed to copy AWB_DRIVE_CONFIG")?;
    }
    // SAFETY: set on the main thread before the app or any worker thread
    // exists.
    unsafe { std::env::set_var("XDG_CONFIG_HOME", &config_home) };

    let _ = std::fs::remove_file(socket);
    let listener = UnixListener::bind(socket)
        .with_context(|| format!("failed to listen on {}", socket.display()))?;
    listener.set_nonblocking(true)?;

    let harness = Harness::builder()
        .with_size(egui::vec2(theme::WINDOW_WIDTH, theme::WINDOW_FULL_HEIGHT))
        .with_pixels_per_point(PIXELS_PER_POINT)
        .with_step_dt(FRAME.as_secs_f32())
        .wgpu()
        .build_eframe(|cc| App::new_headless(cc).expect("headless app"));
    let mut driver = Driver {
        harness,
        pointer: None,
        recording: None,
    };
    println!("awb drive: listening on {}", socket.display());

    let result = loop {
        match listener.accept() {
            Ok((stream, _)) => match driver.session(stream) {
                Ok(Flow::Quit) => break Ok(()),
                Ok(Flow::Continue) => {}
                Err(error) => eprintln!("awb drive: {error:#}"),
            },
            Err(error) if error.kind() == ErrorKind::WouldBlock => driver.frame()?,
            Err(error) => break Err(error.into()),
        }
    };
    let _ = std::fs::remove_file(socket);
    result
}

/// Sends one command line (or each line of stdin for `-`) and prints replies.
pub fn send(socket: &Path, command: &str) -> anyhow::Result<()> {
    let mut stream = UnixStream::connect(socket)
        .with_context(|| format!("no drive server on {}", socket.display()))?;
    let lines: Vec<String> = if command == "-" {
        std::io::stdin().lock().lines().collect::<Result<_, _>>()?
    } else {
        vec![command.to_string()]
    };
    let mut replies = BufReader::new(stream.try_clone()?);
    let mut failed = false;
    for line in lines.iter().filter(|line| !line.trim().is_empty()) {
        writeln!(stream, "{line}")?;
        let mut reply = String::new();
        replies.read_line(&mut reply)?;
        print!("{reply}");
        failed |= reply.starts_with("error");
    }
    if failed {
        bail!("a drive command failed");
    }
    Ok(())
}

#[derive(PartialEq)]
enum Flow {
    Continue,
    Quit,
}

struct Recording {
    dir: PathBuf,
    started: Instant,
    times: Vec<f64>,
}

struct Driver<'a> {
    harness: Harness<'a, App>,
    pointer: Option<Pos2>,
    recording: Option<Recording>,
}

impl Driver<'_> {
    fn session(&mut self, stream: UnixStream) -> anyhow::Result<Flow> {
        stream.set_nonblocking(false)?;
        let mut writer = stream.try_clone()?;
        for line in BufReader::new(stream).lines() {
            let line = line?;
            let (reply, flow) = match self.command(line.trim()) {
                Ok(Some(text)) => (format!("ok {text}"), Flow::Continue),
                Ok(None) => (String::from("ok"), Flow::Quit),
                Err(error) => (format!("error {error:#}"), Flow::Continue),
            };
            writeln!(writer, "{reply}")?;
            if flow == Flow::Quit {
                return Ok(Flow::Quit);
            }
        }
        Ok(Flow::Continue)
    }

    /// Runs one command; `None` means quit.
    fn command(&mut self, line: &str) -> anyhow::Result<Option<String>> {
        let mut words = line.split_whitespace();
        let name = words.next().unwrap_or_default();
        let args: Vec<&str> = words.collect();
        let point = |args: &[&str]| -> anyhow::Result<Pos2> {
            match args {
                [x, y] => Ok(egui::pos2(x.parse()?, y.parse()?)),
                _ => bail!("expected X Y"),
            }
        };

        match name {
            "state" => {}
            "hover" => self.pointer_to(point(&args)?)?,
            "click" => {
                let pos = point(&args)?;
                self.pointer_to(pos)?;
                for pressed in [true, false] {
                    self.push(Event::PointerButton {
                        pos,
                        button: PointerButton::Primary,
                        pressed,
                        modifiers: Modifiers::NONE,
                    });
                    self.frame()?;
                }
            }
            "leave" => {
                self.pointer = None;
                self.push(Event::PointerGone);
                self.frame()?;
            }
            "scroll" => {
                let delta: f32 = args.first().ok_or(anyhow!("expected DY"))?.parse()?;
                self.push(Event::MouseWheel {
                    unit: egui::MouseWheelUnit::Point,
                    delta: egui::vec2(0.0, delta),
                    phase: egui::TouchPhase::Move,
                    modifiers: Modifiers::NONE,
                });
                self.frame()?;
            }
            "key" => {
                let spec = args.first().ok_or(anyhow!("expected a key name"))?;
                let (modifiers, key_name) = match spec.strip_prefix("shift+") {
                    Some(rest) => (Modifiers::SHIFT, rest),
                    None => (Modifiers::NONE, *spec),
                };
                let key = Key::from_name(key_name).ok_or(anyhow!("unknown key {key_name}"))?;
                for pressed in [true, false] {
                    self.push(Event::Key {
                        key,
                        physical_key: None,
                        pressed,
                        repeat: false,
                        modifiers,
                    });
                    self.frame()?;
                }
            }
            "wait" => {
                let ms: u64 = args.first().ok_or(anyhow!("expected MS"))?.parse()?;
                let until = Instant::now() + Duration::from_millis(ms);
                while Instant::now() < until {
                    self.frame()?;
                }
            }
            "shot" => {
                let path = args.first().ok_or(anyhow!("expected PATH"))?;
                self.frame()?;
                self.save_frame(Path::new(path))?;
            }
            "record" => {
                let dir = PathBuf::from(args.first().ok_or(anyhow!("expected DIR"))?);
                std::fs::create_dir_all(&dir)?;
                self.recording = Some(Recording {
                    dir,
                    started: Instant::now(),
                    times: Vec::new(),
                });
            }
            "stop" => {
                let recording = self.recording.take().ok_or(anyhow!("not recording"))?;
                let times: Vec<String> = recording
                    .times
                    .iter()
                    .map(|time| format!("{time:.4}"))
                    .collect();
                std::fs::write(recording.dir.join("times.txt"), times.join("\n") + "\n")?;
                return Ok(Some(format!("{} frames", recording.times.len())));
            }
            "theme" => {
                let mode = match args.first().copied() {
                    Some("auto") => ThemeMode::Auto,
                    Some("day") => ThemeMode::Day,
                    Some("night") => ThemeMode::Night,
                    _ => bail!("expected auto, day or night"),
                };
                self.harness.state_mut().drive_set_theme(mode);
                self.wait_frames(20)?;
            }
            "gradients" => {
                let on = match args.first().copied() {
                    Some("on") => true,
                    Some("off") => false,
                    _ => bail!("expected on or off"),
                };
                let ctx = self.harness.ctx.clone();
                self.harness.state_mut().drive_set_gradients(&ctx, on);
                self.frame()?;
            }
            "quit" => return Ok(None),
            "" => bail!("empty command"),
            other => bail!("unknown command {other}"),
        }
        Ok(Some(self.harness.state().drive_state()))
    }

    fn pointer_to(&mut self, pos: Pos2) -> anyhow::Result<()> {
        self.pointer = Some(pos);
        self.push(Event::PointerMoved(pos));
        self.frame()
    }

    fn push(&mut self, event: Event) {
        self.harness.input_mut().events.push(event);
    }

    fn wait_frames(&mut self, frames: usize) -> anyhow::Result<()> {
        for _ in 0..frames {
            self.frame()?;
        }
        Ok(())
    }

    /// Runs one 60 fps frame: eframe's raw input hook, the app's logic and UI,
    /// and a saved image when recording.
    fn frame(&mut self) -> anyhow::Result<()> {
        let started = Instant::now();
        let ctx = self.harness.ctx.clone();
        let mut input = std::mem::take(self.harness.input_mut());
        eframe::App::raw_input_hook(self.harness.state_mut(), &ctx, &mut input);
        *self.harness.input_mut() = input;
        self.harness.step();

        if let Some(recording) = &self.recording {
            let path = recording
                .dir
                .join(format!("f{:05}.png", recording.times.len()));
            let time = recording.started.elapsed().as_secs_f64();
            self.save_frame(&path)?;
            if let Some(recording) = &mut self.recording {
                recording.times.push(time);
            }
        }

        if let Some(rest) = FRAME.checked_sub(started.elapsed()) {
            std::thread::sleep(rest);
        }
        Ok(())
    }

    fn save_frame(&mut self, path: &Path) -> anyhow::Result<()> {
        let image = self.harness.render().map_err(|error| anyhow!(error))?;
        image
            .save(path)
            .with_context(|| format!("failed to save {}", path.display()))
    }
}
