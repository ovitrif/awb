use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Button, Color32, Context, CornerRadius, FontFamily, FontId, Frame, Label, Layout,
    Margin, Rect, RichText, Sense, Stroke, TextEdit, TextureHandle, TextureOptions, Ui,
    ViewportCommand, vec2,
};
use egui_phosphor::regular as ph;
use menu_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use menu_icon::{MenuBarIcon, MenuBarIconBuilder, MenuBarIconEvent, MouseButton, MouseButtonState};

use crate::backend::{self, PairingPhase, Shared, Snapshot};
use crate::config::Settings;
use crate::glyph;
use crate::login_item;
use crate::theme::{self, icon, medium, regular, semibold};

const FOCUS_GRACE: Duration = Duration::from_millis(300);
const STATUS_POLL: Duration = Duration::from_secs(5);

static STATUS_EVENTS: Mutex<Vec<MenuBarIconEvent>> = Mutex::new(Vec::new());
static MENU_EVENTS: Mutex<Vec<MenuEvent>> = Mutex::new(Vec::new());

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Screen {
    Main,
    Settings,
    Pair,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Devices,
    Logs,
}

pub struct App {
    shared: Arc<Mutex<Shared>>,
    settings: Settings,
    width_text: String,
    height_text: String,
    screen: Screen,
    tab: Tab,
    logo: TextureHandle,
    shell: TextureHandle,
    _status_icon: MenuBarIcon,
    show_item: MenuItem,
    pair_id: MenuId,
    refresh_id: MenuId,
    quit_id: MenuId,
    visible: bool,
    shown_at: Instant,
    focus_hidden_at: Option<Instant>,
    last_poll: Instant,
    open_at_login: Option<bool>,
    pending_show: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        let ctx = cc.egui_ctx.clone();
        theme::install_fonts(&ctx);

        let logo_raster = glyph::window_logo(26, 36.0, -5.0, 4);
        let logo_image = egui::ColorImage::from_rgba_premultiplied(
            [logo_raster.width as usize, logo_raster.height as usize],
            &logo_raster.rgba,
        );
        let logo = ctx.load_texture("awb-logo", logo_image, TextureOptions::LINEAR);

        let shell_raster = glyph::shell_background(3);
        let shell_image = egui::ColorImage::from_rgba_premultiplied(
            [shell_raster.width as usize, shell_raster.height as usize],
            &shell_raster.rgba,
        );
        let shell = ctx.load_texture("awb-shell", shell_image, TextureOptions::LINEAR);

        let menu = Menu::new();
        let show_item = MenuItem::new("Show awb", true, None);
        let pair_item = MenuItem::new("Pair new device", true, None);
        let refresh_item = MenuItem::new("Refresh status", true, None);
        let quit_item = MenuItem::new("Quit awb", true, None);
        menu.append_items(&[
            &show_item,
            &PredefinedMenuItem::separator(),
            &pair_item,
            &refresh_item,
            &PredefinedMenuItem::separator(),
            &quit_item,
        ])?;

        let icon_raster = glyph::menubar_icon(44);
        let icon =
            menu_icon::Icon::from_rgba(icon_raster.rgba, icon_raster.width, icon_raster.height)?;
        let status_icon = MenuBarIconBuilder::new()
            .with_icon(icon)
            .with_icon_as_template(true)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .with_tooltip("awb - Android Wireless Bridge")
            .build()?;

        let status_ctx = ctx.clone();
        MenuBarIconEvent::set_event_handler(Some(move |event| {
            STATUS_EVENTS.lock().unwrap().push(event);
            status_ctx.request_repaint();
        }));

        let menu_ctx = ctx.clone();
        MenuEvent::set_event_handler(Some(move |event| {
            MENU_EVENTS.lock().unwrap().push(event);
            menu_ctx.request_repaint();
        }));

        let shared = Arc::new(Mutex::new(Shared::default()));
        backend::refresh_status(shared.clone(), ctx.clone());

        let settings = Settings::load();
        let width_text = settings.window_width.to_string();
        let height_text = settings.window_height.to_string();

        Ok(Self {
            shared,
            settings,
            width_text,
            height_text,
            screen: Screen::Main,
            tab: Tab::Devices,
            logo,
            shell,
            _status_icon: status_icon,
            show_item,
            pair_id: pair_item.id().clone(),
            refresh_id: refresh_item.id().clone(),
            quit_id: quit_item.id().clone(),
            visible: false,
            shown_at: Instant::now(),
            focus_hidden_at: None,
            last_poll: Instant::now(),
            open_at_login: None,
            pending_show: false,
        })
    }

    fn show(&mut self, ctx: &Context, anchor: Option<menu_icon::Rect>) {
        if let Some(rect) = anchor {
            let scale = ctx
                .input(|i| i.viewport().native_pixels_per_point)
                .unwrap_or(2.0) as f64;
            let bottom = rect.position.y + f64::from(rect.size.height);
            // the status-item rect is in physical pixels; a logical menu bar bottom
            // would sit under ~50 even on 1x displays.
            let divisor = if bottom > 50.0 { scale } else { 1.0 };
            let center_x = (rect.position.x + f64::from(rect.size.width) / 2.0) / divisor;
            // Leave ~8px between the icon and the beak tip (the tip sits a
            // touch below the window top).
            let y = bottom / divisor + 6.5;
            let mut x = center_x - f64::from(theme::WINDOW_WIDTH) / 2.0;

            if let Some(monitor) = ctx.input(|i| i.viewport().monitor_size) {
                let max_x = f64::from(monitor.x) - f64::from(theme::WINDOW_WIDTH) - 8.0;
                x = x.min(max_x);
            }

            ctx.send_viewport_cmd(ViewportCommand::OuterPosition(
                [x.max(8.0) as f32, y as f32].into(),
            ));
            // Reveal on the next frame so the move lands first; a freshly
            // created window would otherwise flash at its default centered
            // position on the very first open.
            self.pending_show = true;
            ctx.request_repaint();
        } else {
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }

        self.visible = true;
        self.shown_at = Instant::now();
        self.focus_hidden_at = None;
        self.show_item.set_text("Hide awb");
        backend::refresh_status(self.shared.clone(), ctx.clone());
    }

    fn hide(&mut self, ctx: &Context) {
        self.pending_show = false;
        ctx.send_viewport_cmd(ViewportCommand::Visible(false));
        self.visible = false;
        self.show_item.set_text("Show awb");
    }

    fn toggle(&mut self, ctx: &Context, anchor: Option<menu_icon::Rect>) {
        if self.visible {
            self.hide(ctx);
        } else if self
            .focus_hidden_at
            .is_some_and(|at| at.elapsed() < FOCUS_GRACE)
        {
            self.focus_hidden_at = None;
        } else {
            self.show(ctx, anchor);
        }
    }

    fn quit(&mut self, ctx: &Context) {
        backend::cancel_pairing(&self.shared);
        backend::stop_all_mirrors(&self.shared);
        ctx.send_viewport_cmd(ViewportCommand::Visible(true));
        ctx.send_viewport_cmd(ViewportCommand::Close);
    }

    fn open_pairing(&mut self, ctx: &Context) {
        self.screen = Screen::Pair;
        backend::start_pairing(self.shared.clone(), ctx.clone());
    }

    fn handle_events(&mut self, ctx: &Context) {
        let menu_events: Vec<MenuEvent> = std::mem::take(&mut *MENU_EVENTS.lock().unwrap());
        for event in menu_events {
            if event.id == self.show_item.id() {
                if self.visible {
                    self.hide(ctx);
                } else {
                    self.show(ctx, None);
                }
            } else if event.id == self.pair_id {
                self.show(ctx, None);
                self.open_pairing(ctx);
            } else if event.id == self.refresh_id {
                backend::refresh_status(self.shared.clone(), ctx.clone());
            } else if event.id == self.quit_id {
                self.quit(ctx);
            }
        }

        let status_events: Vec<MenuBarIconEvent> =
            std::mem::take(&mut *STATUS_EVENTS.lock().unwrap());
        for event in status_events {
            if let MenuBarIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                self.toggle(ctx, Some(rect));
            }
        }
    }

    fn handle_focus(&mut self, ctx: &Context) {
        if !self.visible {
            return;
        }

        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if !focused && self.shown_at.elapsed() > FOCUS_GRACE {
            self.hide(ctx);
            self.focus_hidden_at = Some(Instant::now());
        }
    }

    fn save_settings(&mut self) {
        self.settings.window_width = self.width_text.trim().parse().unwrap_or(0);
        self.settings.window_height = self.height_text.trim().parse().unwrap_or(0);
        self.settings.save();
    }
}

impl eframe::App for App {
    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        [0.0, 0.0, 0.0, 0.0]
    }

    fn on_exit(&mut self) {
        backend::cancel_pairing(&self.shared);
        backend::stop_all_mirrors(&self.shared);
    }

    fn logic(&mut self, ctx: &Context, _frame: &mut eframe::Frame) {
        // Apply a queued move before the window is shown (see `show`).
        if self.pending_show {
            self.pending_show = false;
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }

        self.handle_events(ctx);
        self.handle_focus(ctx);

        if self.visible {
            if self.last_poll.elapsed() > STATUS_POLL {
                self.last_poll = Instant::now();
                backend::refresh_status(self.shared.clone(), ctx.clone());
            }
            ctx.request_repaint_after(Duration::from_secs(1));
        }

        let pairing_done = {
            let state = self.shared.lock().unwrap();
            self.screen == Screen::Pair && state.pairing.is_none()
        };
        if pairing_done {
            self.screen = Screen::Main;
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let rect = ui.max_rect();

        // Beak + rounded body + gradient + hairline, baked into one texture.
        ui.painter().image(
            self.shell.id(),
            rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        let content = Rect::from_min_max(
            egui::pos2(rect.left() + 16.0, rect.top() + theme::BEAK_HEIGHT + 14.0),
            egui::pos2(rect.right() - 16.0, rect.bottom() - 14.0),
        );
        let mut content_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(content)
                .layout(Layout::top_down(Align::Min)),
        );

        match self.screen {
            Screen::Main => self.main_screen(&mut content_ui, &ctx),
            Screen::Settings => self.settings_screen(&mut content_ui, &ctx),
            Screen::Pair => self.pair_screen(&mut content_ui, &ctx),
        }
    }
}

impl App {
    fn main_screen(&mut self, ui: &mut Ui, ctx: &Context) {
        self.main_header(ui, ctx);
        ui.add_space(12.0);
        self.tab_bar(ui);

        match self.tab {
            Tab::Devices => self.devices_tab(ui, ctx),
            Tab::Logs => self.logs_tab(ui),
        }
    }

    fn main_header(&mut self, ui: &mut Ui, ctx: &Context) {
        ui.horizontal(|ui| {
            ui.add(
                egui::Image::new(&self.logo)
                    .fit_to_exact_size(vec2(26.0, 26.0))
                    .corner_radius(0.0),
            );
            ui.add_space(10.0);
            ui.vertical(|ui| {
                ui.add(Label::new(semibold("awb", 15.0, theme::TEXT_STRONG)).selectable(false));
                ui.add(
                    Label::new(regular("Android Wireless Bridge", 11.0, theme::TEXT_SOFT))
                        .selectable(false),
                );
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, ph::QR_CODE, 14.0, theme::TEXT_MUTED).clicked() {
                    self.open_pairing(ctx);
                }
                ui.add_space(10.0);
                if icon_button(ui, ph::GEAR_SIX, 14.0, theme::TEXT_MUTED).clicked() {
                    self.screen = Screen::Settings;
                }
                ui.add_space(10.0);
                if icon_button(ui, ph::ARROWS_CLOCKWISE, 14.0, theme::TEXT_MUTED).clicked() {
                    backend::refresh_status(self.shared.clone(), ctx.clone());
                }
            });
        });
    }

    fn tab_bar(&mut self, ui: &mut Ui) {
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            if tab_item(ui, "Devices", self.tab == Tab::Devices).clicked() {
                self.tab = Tab::Devices;
            }
            ui.add_space(18.0);
            if tab_item(ui, "Logs", self.tab == Tab::Logs).clicked() {
                self.tab = Tab::Logs;
            }
        });
        divider(ui);
    }

    fn devices_tab(&mut self, ui: &mut Ui, ctx: &Context) {
        let (snapshot, mirrors): (Option<Snapshot>, Vec<String>) = {
            let state = self.shared.lock().unwrap();
            (
                state.snapshot.clone(),
                state.mirrors.keys().cloned().collect(),
            )
        };

        let Some(snapshot) = snapshot else {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.add(Label::new(regular(
                    "Checking devices…",
                    11.0,
                    theme::TEXT_FAINT,
                )));
            });
            return;
        };

        if snapshot.devices.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.add(Label::new(regular(
                    "No devices connected",
                    12.5,
                    theme::TEXT_MUTED,
                )));
                ui.add_space(6.0);
                ui.add(Label::new(regular(
                    "Tap the QR icon to pair a phone over Wi-Fi",
                    11.0,
                    theme::TEXT_FAINT,
                )));
            });
            return;
        }

        let scrcpy_ok = snapshot.scrcpy.available;
        for (index, device) in snapshot.devices.iter().enumerate() {
            if index > 0 {
                divider(ui);
            }

            let mirroring = mirrors.contains(&device.serial);
            let action = if mirroring {
                RowAction::enabled(ph::STOP, theme::GREEN)
            } else if device.ready && scrcpy_ok {
                RowAction::enabled(ph::PLAY, theme::TEXT_BRIGHT)
            } else {
                RowAction::disabled(ph::PLAY)
            };
            let detail = if device.ready {
                device.serial.clone()
            } else {
                format!("{} · {}", device.serial, device.state)
            };

            if list_row(ui, ph::DEVICE_MOBILE, &device.name, &detail, Some(action)).clicked() {
                if mirroring {
                    backend::stop_mirror(&self.shared, &device.serial);
                } else {
                    backend::start_mirror(
                        self.shared.clone(),
                        ctx.clone(),
                        device.clone(),
                        self.settings.scrcpy_options(),
                    );
                }
            }
        }
    }

    fn logs_tab(&mut self, ui: &mut Ui) {
        let logs: Vec<String> = self.shared.lock().unwrap().logs.clone();

        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if logs.is_empty() {
                    ui.add(Label::new(
                        RichText::new("No output yet.")
                            .font(FontId::new(10.5, FontFamily::Monospace))
                            .color(theme::TEXT_FAINT),
                    ));
                }

                for line in &logs {
                    ui.add(
                        Label::new(
                            RichText::new(line)
                                .font(FontId::new(10.5, FontFamily::Monospace))
                                .color(theme::TEXT_CHECK),
                        )
                        .wrap(),
                    );
                    ui.add_space(2.0);
                }
            });
    }

    fn settings_screen(&mut self, ui: &mut Ui, ctx: &Context) {
        let _ = ctx;
        if nav_header(ui, "Settings").clicked() {
            self.screen = Screen::Main;
        }
        ui.add_space(12.0);

        ui.add(
            Label::new(semibold("Screen Mirroring", 12.5, theme::TEXT_BRIGHT)).selectable(false),
        );
        ui.add_space(10.0);

        let mut changed = false;
        ui.horizontal(|ui| {
            let total = ui.available_width();
            let title_width = total - 2.0 * 64.0 - 2.0 * 8.0;
            changed |= labeled_input(ui, "Title", title_width, &mut self.settings.window_title);
            ui.add_space(8.0);
            changed |= labeled_input(ui, "W", 64.0, &mut self.width_text);
            ui.add_space(8.0);
            changed |= labeled_input(ui, "H", 64.0, &mut self.height_text);
        });

        ui.add_space(10.0);
        ui.horizontal(|ui| {
            changed |= check_item(ui, "Top", &mut self.settings.always_on_top);
            ui.add_space(20.0);
            changed |= check_item(ui, "Plain", &mut self.settings.plain_window);
        });

        if changed {
            self.save_settings();
        }

        ui.add_space(12.0);
        divider(ui);
        ui.add_space(12.0);

        ui.add(Label::new(semibold("General", 12.5, theme::TEXT_BRIGHT)).selectable(false));
        ui.add_space(10.0);
        // Queried lazily: the System Events lookup prompts for Automation
        // access the first time, so we defer it until Settings is opened.
        let mut open_at_login = match self.open_at_login {
            Some(value) => value,
            None => {
                let enabled = login_item::is_enabled();
                self.open_at_login = Some(enabled);
                enabled
            }
        };
        if check_item(ui, "Open at Login", &mut open_at_login) {
            login_item::set_enabled(open_at_login);
            self.open_at_login = Some(open_at_login);
        }

        ui.add_space(12.0);
        divider(ui);
        ui.add_space(12.0);

        ui.add(Label::new(semibold("Dependencies", 12.5, theme::TEXT_BRIGHT)).selectable(false));

        let snapshot = self.shared.lock().unwrap().snapshot.clone();
        let (adb, scrcpy) = match &snapshot {
            Some(snapshot) => (Some(&snapshot.adb), Some(&snapshot.scrcpy)),
            None => (None, None),
        };

        dependency_row(ui, ph::TERMINAL_WINDOW, "ADB", adb);
        divider(ui);
        dependency_row(ui, ph::MONITOR_PLAY, "scrcpy", scrcpy);
    }

    fn pair_screen(&mut self, ui: &mut Ui, ctx: &Context) {
        if nav_header(ui, "Pair device").clicked() {
            backend::cancel_pairing(&self.shared);
            self.screen = Screen::Main;
            return;
        }

        let phase = {
            let state = self.shared.lock().unwrap();
            state.pairing.as_ref().map(|session| session.phase.clone())
        };

        let Some(phase) = phase else {
            self.screen = Screen::Main;
            return;
        };

        match phase {
            PairingPhase::Qr { modules } => {
                let content_height = 168.0 + 14.0 + 18.0 + 4.0 + 30.0;
                center_pad(ui, content_height);

                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(168.0, 168.0), Sense::hover());
                    let painter = ui.painter();
                    painter.rect_filled(rect, 12.0, theme::QR_CARD);

                    // Reserve a 4-module quiet zone inside the 144px area; the
                    // raw matrix has none and Android's scanner rejects codes
                    // without it.
                    let qr_size = 144.0;
                    let quiet = 4.0;
                    let cell = qr_size / (modules.size as f32 + 2.0 * quiet);
                    let origin = rect.center() - vec2(qr_size / 2.0, qr_size / 2.0)
                        + vec2(quiet * cell, quiet * cell);
                    for y in 0..modules.size {
                        for x in 0..modules.size {
                            if modules.dark[y * modules.size + x] {
                                let min = origin + vec2(x as f32 * cell, y as f32 * cell);
                                painter.rect_filled(
                                    Rect::from_min_size(min, vec2(cell + 0.3, cell + 0.3)),
                                    0.0,
                                    theme::QR_INK,
                                );
                            }
                        }
                    }

                    ui.add_space(14.0);
                    ui.add(Label::new(semibold(
                        "Scan with your phone",
                        13.0,
                        theme::TEXT_BRIGHT,
                    )));
                    ui.add_space(4.0);
                    hint_label(
                        ui,
                        "Developer options → Wireless debugging → Pair device with QR code",
                        300.0,
                    );
                });
            }
            PairingPhase::Connecting { label } => {
                center_pad(ui, 28.0 + 12.0 + 18.0 + 4.0 + 16.0);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Spinner::new().size(28.0).color(theme::GREEN));
                    ui.add_space(12.0);
                    ui.add(Label::new(semibold(label, 13.0, theme::TEXT_BRIGHT)));
                    ui.add_space(4.0);
                    hint_label(ui, "Keep both devices on the same Wi-Fi network", 280.0);
                });
            }
            PairingPhase::Failed { message } => {
                center_pad(ui, 28.0 + 12.0 + 18.0 + 4.0 + 32.0 + 36.0);
                ui.vertical_centered(|ui| {
                    ui.add(Label::new(icon(ph::WARNING_CIRCLE, 28.0, theme::RED)));
                    ui.add_space(12.0);
                    ui.add(Label::new(semibold(
                        "Pairing failed",
                        13.0,
                        theme::TEXT_BRIGHT,
                    )));
                    ui.add_space(4.0);
                    hint_label(ui, &message, 280.0);
                    ui.add_space(12.0);

                    ui.horizontal(|ui| {
                        let buttons_width = 2.0 * 70.0 + 10.0;
                        let pad = (ui.available_width() - buttons_width) / 2.0;
                        ui.add_space(pad.max(0.0));

                        if pill_button(
                            ui,
                            Some(ph::ARROW_CLOCKWISE),
                            "Retry",
                            theme::GREEN,
                            theme::GREEN_INK,
                            true,
                        )
                        .clicked()
                        {
                            backend::start_pairing(self.shared.clone(), ctx.clone());
                        }
                        ui.add_space(10.0);
                        if pill_button(ui, None, "Cancel", theme::SURFACE, theme::TEXT_CHECK, false)
                            .clicked()
                        {
                            backend::cancel_pairing(&self.shared);
                            self.screen = Screen::Main;
                        }
                    });
                });
            }
        }
    }
}

struct RowAction {
    glyph: &'static str,
    color: Color32,
    enabled: bool,
}

impl RowAction {
    fn enabled(glyph: &'static str, color: Color32) -> Self {
        Self {
            glyph,
            color,
            enabled: true,
        }
    }

    fn disabled(glyph: &'static str) -> Self {
        Self {
            glyph,
            color: Color32::from_rgb(0x5A, 0x5E, 0x68),
            enabled: false,
        }
    }
}

fn icon_button(ui: &mut Ui, glyph: &str, size: f32, color: Color32) -> egui::Response {
    let response = ui.add(Button::new(icon(glyph, size, color)).frame(false));
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn tab_item(ui: &mut Ui, label: &str, active: bool) -> egui::Response {
    let text = if active {
        semibold(label, 12.5, theme::TEXT_STRONG)
    } else {
        medium(label, 12.5, theme::TEXT_MUTED)
    };

    let response = ui
        .vertical(|ui| {
            let response = ui.add(Label::new(text).selectable(false).sense(Sense::click()));
            ui.add_space(6.0);
            let (rect, _) =
                ui.allocate_exact_size(vec2(response.rect.width(), 2.0), Sense::hover());
            if active {
                ui.painter().rect_filled(rect, 1.0, theme::GREEN);
            }
            response
        })
        .inner;

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::HAIRLINE);
}

fn list_row(
    ui: &mut Ui,
    row_icon: &str,
    name: &str,
    detail: &str,
    action: Option<RowAction>,
) -> egui::Response {
    let mut clicked = false;

    let response = ui.horizontal(|ui| {
        ui.set_height(38.0);
        ui.add_space(2.0);
        ui.add(Label::new(icon(row_icon, 14.0, theme::TEXT_LABEL)).selectable(false));
        ui.add_space(10.0);
        ui.add(Label::new(medium(name, 12.5, theme::TEXT_BRIGHT)).selectable(false));
        ui.add_space(10.0);

        let reserved = if action.is_some() { 34.0 } else { 4.0 };
        ui.scope(|ui| {
            ui.set_max_width((ui.available_width() - reserved).max(20.0));
            ui.add(
                Label::new(regular(detail, 11.0, theme::TEXT_FAINT))
                    .truncate()
                    .selectable(false),
            );
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(2.0);
            if let Some(action) = action {
                let (rect, response) = ui.allocate_exact_size(
                    vec2(22.0, 22.0),
                    if action.enabled {
                        Sense::click()
                    } else {
                        Sense::hover()
                    },
                );
                ui.painter().rect_filled(rect, 6.0, theme::SURFACE);
                ui.painter().text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    action.glyph,
                    theme::icon_font(10.0),
                    action.color,
                );

                if action.enabled {
                    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    clicked = response.clicked();
                }
            }
        });
    });

    let mut response = response.response;
    if clicked {
        response.flags |= egui::response::Flags::CLICKED;
    }
    response
}

fn dependency_row(ui: &mut Ui, row_icon: &str, name: &str, info: Option<&backend::ToolInfo>) {
    ui.horizontal(|ui| {
        ui.set_height(38.0);
        ui.add_space(2.0);
        ui.add(Label::new(icon(row_icon, 14.0, theme::TEXT_LABEL)).selectable(false));
        ui.add_space(10.0);
        ui.add(Label::new(medium(name, 12.5, theme::TEXT_BRIGHT)).selectable(false));
        ui.add_space(10.0);

        let detail = info.map_or("checking…".to_string(), |tool| tool.detail.clone());
        let available_width = ui.available_width() - 50.0;
        ui.scope(|ui| {
            ui.set_max_width(available_width.max(40.0));
            ui.add(
                Label::new(regular(detail, 11.0, theme::TEXT_FAINT))
                    .truncate()
                    .selectable(false),
            );
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(2.0);
            match info {
                Some(tool) if tool.available => {
                    ui.add(Label::new(regular("Ready", 11.0, theme::TEXT_MUTED)).selectable(false));
                }
                Some(_) => {
                    ui.add(Label::new(regular("Missing", 11.0, theme::RED)).selectable(false));
                }
                None => {}
            }
        });
    });
}

fn labeled_input(ui: &mut Ui, label: &str, width: f32, value: &mut String) -> bool {
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.add(Label::new(regular(label, 10.5, theme::TEXT_LABEL)).selectable(false));
        ui.add_space(4.0);

        let frame = Frame::new()
            .fill(theme::INPUT_BG)
            .stroke(Stroke::new(1.0, theme::INPUT_STROKE))
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(9, 6));

        frame
            .show(ui, |ui| {
                ui.set_width(width - 20.0);
                ui.add(
                    TextEdit::singleline(value)
                        .frame(Frame::NONE)
                        .font(FontId::new(12.0, FontFamily::Proportional))
                        .text_color(theme::TEXT_BRIGHT)
                        .margin(Margin::ZERO)
                        .desired_width(width - 20.0),
                )
            })
            .inner
            .changed()
    })
    .inner
}

fn check_item(ui: &mut Ui, label: &str, value: &mut bool) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(vec2(15.0, 15.0), Sense::click());
        let painter = ui.painter();

        if *value {
            painter.rect_filled(rect, 4.0, theme::GREEN);
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                ph::CHECK,
                theme::icon_font(10.0),
                theme::GREEN_INK,
            );
        } else {
            painter.rect_filled(rect, 4.0, theme::INPUT_BG);
            painter.rect_stroke(
                rect,
                4.0,
                Stroke::new(1.0, theme::CHECK_STROKE),
                egui::StrokeKind::Inside,
            );
        }

        ui.add_space(7.0);
        let label_response = ui.add(
            Label::new(regular(label, 12.0, theme::TEXT_CHECK))
                .selectable(false)
                .sense(Sense::click()),
        );

        if response.clicked() || label_response.clicked() {
            *value = !*value;
            changed = true;
        }
    });

    changed
}

fn nav_header(ui: &mut Ui, title: &str) -> egui::Response {
    ui.horizontal(|ui| {
        let response = icon_button(ui, ph::CARET_LEFT, 16.0, theme::TEXT_MUTED);
        ui.add_space(10.0);
        ui.add(Label::new(semibold(title, 15.0, theme::TEXT_STRONG)).selectable(false));
        response
    })
    .inner
}

fn center_pad(ui: &mut Ui, content_height: f32) {
    let pad = (ui.available_height() - content_height) / 2.0;
    ui.add_space(pad.max(0.0));
}

fn hint_label(ui: &mut Ui, text: &str, width: f32) {
    ui.scope(|ui| {
        ui.set_max_width(width);
        ui.add(
            Label::new(regular(text, 11.0, theme::TEXT_SOFT))
                .halign(egui::Align::Center)
                .wrap(),
        );
    });
}

fn pill_button(
    ui: &mut Ui,
    leading_icon: Option<&str>,
    label: &str,
    fill: Color32,
    text_color: Color32,
    bold: bool,
) -> egui::Response {
    let family = if bold {
        FontFamily::Name(theme::SEMIBOLD.into())
    } else {
        FontFamily::Name(theme::MEDIUM.into())
    };
    let font = FontId::new(11.5, family);
    let text_width = ui
        .painter()
        .layout_no_wrap(label.to_owned(), font.clone(), text_color)
        .size()
        .x;

    let icon_width = if leading_icon.is_some() {
        11.0 + 6.0
    } else {
        0.0
    };
    let size = vec2(14.0 + icon_width + text_width + 14.0, 28.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter();
    painter.rect_filled(rect, 7.0, fill);

    let mut cursor_x = rect.min.x + 14.0;
    if let Some(glyph) = leading_icon {
        painter.text(
            egui::pos2(cursor_x, rect.center().y),
            egui::Align2::LEFT_CENTER,
            glyph,
            theme::icon_font(11.0),
            text_color,
        );
        cursor_x += icon_width;
    }
    painter.text(
        egui::pos2(cursor_x, rect.center().y),
        egui::Align2::LEFT_CENTER,
        label,
        font,
        text_color,
    );

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
