use std::collections::HashSet;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::containers::scroll_area::ScrollBarVisibility;
use eframe::egui::{
    self, Align, Button, Color32, Context, CornerRadius, FontFamily, FontId, Frame, Label, Layout,
    Margin, Rect, Sense, Shadow, Stroke, TextEdit, TextureHandle, TextureOptions, Theme, Ui,
    ViewportCommand, vec2,
};
use egui_phosphor::regular as ph;
use menu_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use menu_icon::{MenuBarIcon, MenuBarIconBuilder, MenuBarIconEvent, MouseButton, MouseButtonState};

use crate::backend::{self, PairingPhase, PairingProgress, Shared, Snapshot};
use crate::config::{Settings, ThemeMode};
use crate::glyph;
use crate::login_item;
use crate::theme::{self, icon, medium, regular, semibold};

const FOCUS_GRACE: Duration = Duration::from_millis(300);
const STATUS_POLL: Duration = Duration::from_secs(5);
const SCREEN_TRANSITION_DURATION: Duration = Duration::from_millis(220);
const SKIN_TRANSITION_DURATION: Duration = Duration::from_millis(180);
const POPOVER_APPEAR_DURATION: Duration = Duration::from_millis(160);
const POPOVER_HIDE_DURATION: Duration = Duration::from_millis(120);
const SCROLLBAR_HIDE_DELAY: Duration = Duration::from_millis(250);
const SCREEN_INCOMING_OFFSET: f32 = 32.0;
const SCREEN_OUTGOING_OFFSET: f32 = 18.0;
const THEME_MODE_GROUP_SIZE: egui::Vec2 = vec2(172.0, 28.0);
const SCROLL_EDGE_FADE_HEIGHT: f32 = 22.0;
const SCROLLBAR_HANDLE_HEIGHT: f32 = 44.0;
const SCROLLBAR_HANDLE_WIDTH: f32 = 2.0;
const SCROLLBAR_TRACK_INSET: f32 = 5.0;
const SCROLLBAR_OPACITY_FADE: Duration = Duration::from_millis(100);
/// How long after launch to keep forcing the window hidden, in case the
/// platform surfaces it despite `with_visible(false)`.
const STARTUP_HIDE: Duration = Duration::from_millis(800);

static STATUS_EVENTS: Mutex<Vec<MenuBarIconEvent>> = Mutex::new(Vec::new());
static MENU_EVENTS: Mutex<Vec<MenuEvent>> = Mutex::new(Vec::new());

const POPOVER_GAP: f64 = 6.5;
const WINDOW_MARGIN: f64 = 8.0;

#[derive(Debug, Clone, Copy, PartialEq)]
struct DisplayBounds {
    min_x: f64,
    max_x: f64,
    min_y: f64,
    max_y: f64,
    scale: f64,
}

impl DisplayBounds {
    fn contains(&self, x: f64, y: f64) -> bool {
        (self.min_x..=self.max_x).contains(&x) && (self.min_y..=self.max_y).contains(&y)
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct MenuAnchor {
    rect_x: f64,
    rect_y: f64,
    rect_width: f64,
    rect_height: f64,
    click_x: f64,
    click_y: f64,
}

impl MenuAnchor {
    fn new(rect: menu_icon::Rect, click_x: f64, click_y: f64) -> Self {
        Self {
            rect_x: rect.position.x,
            rect_y: rect.position.y,
            rect_width: f64::from(rect.size.width),
            rect_height: f64::from(rect.size.height),
            click_x,
            click_y,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct LogicalMenuAnchor {
    x: f64,
    bottom_y: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Screen {
    Main,
    Settings,
    Pair,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ScreenTransition {
    from: Screen,
    to: Screen,
    direction: f32,
    started_at: Instant,
    cancel_pairing_on_complete: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PopoverTransitionPhase {
    Appearing,
    Disappearing,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct PopoverTransition {
    phase: PopoverTransitionPhase,
    started_at: Instant,
}

#[derive(Debug, Clone, Copy)]
struct SkinTransition {
    from_day_weight: f32,
    target: theme::Appearance,
    started_at: Instant,
}

#[derive(Debug, Clone, Copy)]
struct SkinState {
    appearance: theme::Appearance,
    day_weight: f32,
    transition: Option<SkinTransition>,
}

#[derive(Debug, Clone, Copy)]
struct SkinUpdate {
    day_weight: f32,
    committed: Option<theme::Appearance>,
    animating: bool,
}

#[derive(Debug, Clone, Copy)]
struct AnimationState {
    screen: Option<ScreenTransition>,
    popover: Option<PopoverTransition>,
    skin: SkinState,
}

impl SkinState {
    fn new(appearance: theme::Appearance) -> Self {
        Self {
            appearance,
            day_weight: appearance_day_weight(appearance),
            transition: None,
        }
    }

    fn set_target_at(&mut self, target: theme::Appearance, now: Instant) -> bool {
        if self
            .transition
            .is_some_and(|active| active.target == target)
            || (self.transition.is_none() && self.appearance == target)
        {
            return false;
        }

        let from_day_weight = self.day_weight_at(now);
        self.day_weight = from_day_weight;
        self.transition = Some(SkinTransition {
            from_day_weight,
            target,
            started_at: now,
        });
        true
    }

    fn day_weight_at(&self, now: Instant) -> f32 {
        self.transition.map_or(self.day_weight, |transition| {
            skin_day_weight(transition, skin_transition_progress(transition, now))
        })
    }
}

impl AnimationState {
    fn new(appearance: theme::Appearance) -> Self {
        Self {
            screen: None,
            popover: None,
            skin: SkinState::new(appearance),
        }
    }

    fn advance_skin_at(&mut self, now: Instant) -> SkinUpdate {
        let Some(transition) = self.skin.transition else {
            return SkinUpdate {
                day_weight: self.skin.day_weight,
                committed: None,
                animating: false,
            };
        };

        let elapsed = now.saturating_duration_since(transition.started_at);
        if elapsed >= SKIN_TRANSITION_DURATION {
            let target = transition.target;
            let committed = self.commit_skin(target);
            return SkinUpdate {
                day_weight: self.skin.day_weight,
                committed: committed.then_some(target),
                animating: false,
            };
        }

        let progress = elapsed.as_secs_f32() / SKIN_TRANSITION_DURATION.as_secs_f32();
        self.skin.day_weight = skin_day_weight(transition, progress);
        SkinUpdate {
            day_weight: self.skin.day_weight,
            committed: None,
            animating: true,
        }
    }

    fn commit_skin(&mut self, target: theme::Appearance) -> bool {
        let changed = self.skin.appearance != target
            || self.skin.transition.is_some()
            || (self.skin.day_weight - appearance_day_weight(target)).abs() > f32::EPSILON;
        self.skin.appearance = target;
        self.skin.day_weight = appearance_day_weight(target);
        self.skin.transition = None;
        changed
    }
}

fn appearance_day_weight(appearance: theme::Appearance) -> f32 {
    match appearance {
        theme::Appearance::Day => 1.0,
        theme::Appearance::Night => 0.0,
    }
}

fn skin_transition_progress(transition: SkinTransition, now: Instant) -> f32 {
    now.saturating_duration_since(transition.started_at)
        .as_secs_f32()
        / SKIN_TRANSITION_DURATION.as_secs_f32()
}

fn skin_day_weight(transition: SkinTransition, progress: f32) -> f32 {
    let eased = egui::emath::easing::cubic_in_out(progress.clamp(0.0, 1.0));
    let target = appearance_day_weight(transition.target);
    transition.from_day_weight + (target - transition.from_day_weight) * eased
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
    animations: AnimationState,
    tab: Tab,
    logo: TextureHandle,
    day_shell: TextureHandle,
    night_shell: TextureHandle,
    status_icon: MenuBarIcon,
    show_item: MenuItem,
    pair_id: MenuId,
    refresh_id: MenuId,
    quit_id: MenuId,
    visible: bool,
    shown_at: Instant,
    focus_hidden_at: Option<Instant>,
    last_poll: Instant,
    created_at: Instant,
    open_at_login: Option<bool>,
    pending_show: bool,
    last_menu_anchor: Option<MenuAnchor>,
    auto_mirrored: HashSet<String>,
    settings_scroll_offset: f32,
    settings_scroll_active_at: Option<Instant>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        let ctx = cc.egui_ctx.clone();
        theme::install_fonts(&ctx);
        let settings = Settings::load();
        let appearance = resolved_appearance(settings.theme, ctx.system_theme());
        theme::apply(&ctx, appearance);

        let logo_raster = glyph::window_logo(26, 36.0, -5.0, 4);
        let logo_image = egui::ColorImage::from_rgba_premultiplied(
            [logo_raster.width as usize, logo_raster.height as usize],
            &logo_raster.rgba,
        );
        let logo = ctx.load_texture("awb-logo", logo_image, TextureOptions::LINEAR);

        let day_shell = load_shell_texture(&ctx, "awb-shell-day", theme::Appearance::Day);
        let night_shell = load_shell_texture(&ctx, "awb-shell-night", theme::Appearance::Night);

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

        let width_text = settings.window_width.to_string();
        let height_text = settings.window_height.to_string();
        Ok(Self {
            shared,
            settings,
            width_text,
            height_text,
            screen: Screen::Main,
            animations: AnimationState::new(appearance),
            tab: Tab::Devices,
            logo,
            day_shell,
            night_shell,
            status_icon,
            show_item,
            pair_id: pair_item.id().clone(),
            refresh_id: refresh_item.id().clone(),
            quit_id: quit_item.id().clone(),
            visible: false,
            shown_at: Instant::now(),
            focus_hidden_at: None,
            last_poll: Instant::now(),
            created_at: Instant::now(),
            open_at_login: None,
            pending_show: false,
            last_menu_anchor: None,
            auto_mirrored: HashSet::new(),
            settings_scroll_offset: 0.0,
            settings_scroll_active_at: None,
        })
    }

    fn show(&mut self, ctx: &Context, anchor: Option<MenuAnchor>) {
        if let Some(anchor) = anchor {
            let fallback_monitor_width = ctx.input(|i| {
                i.viewport()
                    .monitor_size
                    .map(|monitor| f64::from(monitor.x))
            });
            let displays = active_display_bounds();
            let (x, y) = popover_position(anchor, fallback_monitor_width, &displays);

            ctx.send_viewport_cmd(ViewportCommand::OuterPosition([x as f32, y as f32].into()));
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
        self.animations.popover = Some(PopoverTransition {
            phase: PopoverTransitionPhase::Appearing,
            started_at: Instant::now(),
        });
        self.shown_at = Instant::now();
        self.focus_hidden_at = None;
        self.show_item.set_text("Hide awb");
        backend::refresh_status(self.shared.clone(), ctx.clone());
    }

    fn hide(&mut self, ctx: &Context) {
        if !self.visible || self.is_disappearing() {
            return;
        }

        self.pending_show = false;
        self.animations.popover = Some(PopoverTransition {
            phase: PopoverTransitionPhase::Disappearing,
            started_at: Instant::now(),
        });
        self.show_item.set_text("Show awb");
        ctx.request_repaint();
    }

    fn toggle(&mut self, ctx: &Context, anchor: Option<MenuAnchor>) {
        if self.visible && !self.is_disappearing() {
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
        self.navigate_to(Screen::Pair, ctx);
        backend::start_pairing(self.shared.clone(), ctx.clone());
    }

    fn navigate_to(&mut self, screen: Screen, ctx: &Context) {
        if self.screen == screen {
            return;
        }

        self.animations.screen = Some(ScreenTransition {
            from: self.screen,
            to: screen,
            direction: screen_transition_direction(self.screen, screen),
            started_at: Instant::now(),
            cancel_pairing_on_complete: false,
        });
        self.screen = screen;
        ctx.request_repaint();
    }

    fn leave_pairing(&mut self, ctx: &Context) {
        self.navigate_to(Screen::Main, ctx);
        if let Some(transition) = &mut self.animations.screen {
            transition.cancel_pairing_on_complete = true;
        } else {
            backend::cancel_pairing(&self.shared);
        }
    }

    fn handle_events(&mut self, ctx: &Context) {
        let status_events: Vec<MenuBarIconEvent> =
            std::mem::take(&mut *STATUS_EVENTS.lock().unwrap());
        for event in status_events {
            if let MenuBarIconEvent::Click {
                button,
                button_state,
                rect,
                position,
                ..
            } = event
            {
                let anchor = MenuAnchor::new(rect, position.x, position.y);
                self.last_menu_anchor = Some(anchor);

                if button == MouseButton::Left && button_state == MouseButtonState::Up {
                    self.toggle(ctx, Some(anchor));
                }
            }
        }

        let menu_events: Vec<MenuEvent> = std::mem::take(&mut *MENU_EVENTS.lock().unwrap());
        for event in menu_events {
            if event.id == self.show_item.id() {
                if self.visible && !self.is_disappearing() {
                    self.hide(ctx);
                } else {
                    self.show(ctx, self.menu_anchor());
                }
            } else if event.id == self.pair_id {
                self.open_pairing(ctx);
                self.show(ctx, self.menu_anchor());
            } else if event.id == self.refresh_id {
                backend::refresh_status(self.shared.clone(), ctx.clone());
            } else if event.id == self.quit_id {
                self.quit(ctx);
            }
        }
    }

    fn handle_focus(&mut self, ctx: &Context) {
        if !self.visible || self.is_disappearing() {
            return;
        }

        let focused = ctx.input(|i| i.viewport().focused.unwrap_or(true));
        if !focused && self.shown_at.elapsed() > FOCUS_GRACE {
            self.hide(ctx);
            self.focus_hidden_at = Some(Instant::now());
        }
    }

    fn menu_anchor(&self) -> Option<MenuAnchor> {
        self.last_menu_anchor.or_else(|| {
            self.status_icon.rect().map(|rect| {
                let click_x = rect.position.x + f64::from(rect.size.width) / 2.0;
                let click_y = rect.position.y + f64::from(rect.size.height) / 2.0;
                MenuAnchor::new(rect, click_x, click_y)
            })
        })
    }

    fn is_disappearing(&self) -> bool {
        self.animations
            .popover
            .is_some_and(|transition| transition.phase == PopoverTransitionPhase::Disappearing)
    }

    fn update_popover_transition(&mut self, ctx: &Context) {
        let Some(transition) = self.animations.popover else {
            return;
        };
        let duration = popover_transition_duration(transition.phase);

        if transition.started_at.elapsed() >= duration {
            self.animations.popover = None;
            if transition.phase == PopoverTransitionPhase::Disappearing {
                ctx.send_viewport_cmd(ViewportCommand::Visible(false));
                self.visible = false;
            }
        } else {
            ctx.request_repaint();
        }
    }

    fn save_settings(&mut self) {
        self.settings.window_width = self.width_text.trim().parse().unwrap_or(0);
        self.settings.window_height = self.height_text.trim().parse().unwrap_or(0);
        self.settings.save();
    }

    fn sync_appearance(&mut self, ctx: &Context, now: Instant) {
        let appearance = resolved_appearance(self.settings.theme, ctx.system_theme());
        if self.animations.skin.set_target_at(appearance, now) {
            ctx.request_repaint();
        }
    }

    fn update_skin_transition(&mut self, ctx: &Context, now: Instant) {
        let update = self.animations.advance_skin_at(now);
        theme::set_day_weight(update.day_weight);
        if let Some(appearance) = update.committed {
            theme::apply(ctx, appearance);
        }
        if update.animating {
            ctx.request_repaint();
        }
    }

    /// Auto-start mirroring for newly connected physical phones (never
    /// emulators) when the setting is on and scrcpy is available.
    fn maybe_auto_mirror(&mut self, ctx: &Context) {
        if !self.settings.auto_mirror {
            self.auto_mirrored.clear();
            return;
        }

        let (snapshot, mirroring) = {
            let state = self.shared.lock().unwrap();
            let mut mirroring = state.mirrors.keys().cloned().collect::<HashSet<String>>();
            mirroring.extend(state.starting_mirrors.iter().cloned());
            (state.snapshot.clone(), mirroring)
        };
        let Some(snapshot) = snapshot else { return };
        if !snapshot.scrcpy.available {
            return;
        }

        // Forget devices that have disconnected so a later reconnect re-mirrors.
        let present = ready_device_mirror_keys(&snapshot.devices);
        self.auto_mirrored
            .retain(|serial| present.contains(serial.as_str()));

        for device in &snapshot.devices {
            if device.ready
                && !device.is_emulator
                && !mirroring.contains(&device.mirror_key)
                && self.auto_mirrored.insert(device.mirror_key.clone())
            {
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

fn ready_device_mirror_keys(devices: &[backend::DeviceInfo]) -> HashSet<&str> {
    devices
        .iter()
        .filter(|device| device.ready)
        .map(|device| device.mirror_key.as_str())
        .collect()
}

fn load_shell_texture(ctx: &Context, name: &str, appearance: theme::Appearance) -> TextureHandle {
    let raster = glyph::shell_background(3, appearance);
    let image = egui::ColorImage::from_rgba_premultiplied(
        [raster.width as usize, raster.height as usize],
        &raster.rgba,
    );
    ctx.load_texture(name, image, TextureOptions::LINEAR)
}

fn resolved_appearance(mode: ThemeMode, system_theme: Option<Theme>) -> theme::Appearance {
    match mode {
        ThemeMode::Day => theme::Appearance::Day,
        ThemeMode::Night => theme::Appearance::Night,
        ThemeMode::Auto => match system_theme {
            Some(Theme::Light) => theme::Appearance::Day,
            Some(Theme::Dark) | None => theme::Appearance::Night,
        },
    }
}

fn screen_transition_direction(from: Screen, to: Screen) -> f32 {
    let depth = |screen| match screen {
        Screen::Main => 0,
        Screen::Settings | Screen::Pair => 1,
    };

    if depth(to) < depth(from) { -1.0 } else { 1.0 }
}

fn popover_position(
    anchor: MenuAnchor,
    fallback_monitor_width: Option<f64>,
    displays: &[DisplayBounds],
) -> (f64, f64) {
    let anchor = logical_menu_anchor(anchor, displays);
    let y = anchor.bottom_y + POPOVER_GAP;
    let x = clamp_window_x_to_displays(
        anchor.x - f64::from(theme::WINDOW_WIDTH) / 2.0,
        anchor.x,
        y,
        f64::from(theme::WINDOW_WIDTH),
        WINDOW_MARGIN,
        fallback_monitor_width,
        displays,
    );

    (x, y)
}

fn logical_menu_anchor(anchor: MenuAnchor, displays: &[DisplayBounds]) -> LogicalMenuAnchor {
    let display = display_for_physical_menu_anchor(anchor, displays);
    let scale = display
        .map(|display| display.scale)
        .unwrap_or_else(|| inferred_status_scale(anchor.rect_height));

    let click_x = anchor.click_x / scale;
    let click_y = anchor.click_y / scale;
    let rect_center_x = (anchor.rect_x + anchor.rect_width / 2.0) / scale;
    let rect_bottom_y = (anchor.rect_y + anchor.rect_height) / scale;
    let icon_height = (anchor.rect_height / scale).clamp(16.0, 36.0);

    let rect_matches_click_display = display
        .map(|display| {
            display.contains(rect_center_x, rect_bottom_y)
                && (rect_center_x - click_x).abs() <= 96.0
        })
        .unwrap_or(false);
    let x = if rect_matches_click_display {
        rect_center_x
    } else {
        click_x
    };
    let bottom_y = if rect_matches_click_display {
        rect_bottom_y
    } else {
        click_y + icon_height / 2.0
    };

    LogicalMenuAnchor { x, bottom_y }
}

fn popover_transition_duration(phase: PopoverTransitionPhase) -> Duration {
    match phase {
        PopoverTransitionPhase::Appearing => POPOVER_APPEAR_DURATION,
        PopoverTransitionPhase::Disappearing => POPOVER_HIDE_DURATION,
    }
}

fn popover_transition_opacity(phase: PopoverTransitionPhase, progress: f32) -> f32 {
    let progress = progress.clamp(0.0, 1.0);
    match phase {
        PopoverTransitionPhase::Appearing => egui::emath::easing::cubic_out(progress),
        PopoverTransitionPhase::Disappearing => 1.0 - egui::emath::easing::cubic_in(progress),
    }
}

#[cfg(target_os = "macos")]
fn set_native_window_opacity(frame: &eframe::Frame, opacity: f32) -> bool {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let Ok(handle) = frame.window_handle() else {
        return false;
    };
    let RawWindowHandle::AppKit(handle) = handle.as_raw() else {
        return false;
    };
    let ns_view_ptr = handle.ns_view.as_ptr().cast::<objc2_app_kit::NSView>();
    // SAFETY: AppKit supplies this non-null NSView pointer for the lifetime of
    // the eframe window, and it is used only synchronously on the UI thread.
    let Some(ns_view) = (unsafe { ns_view_ptr.as_ref() }) else {
        return false;
    };
    let Some(window) = ns_view.window() else {
        return false;
    };

    window.setAlphaValue(f64::from(opacity.clamp(0.0, 1.0)));
    true
}

#[cfg(not(target_os = "macos"))]
fn set_native_window_opacity(_frame: &eframe::Frame, _opacity: f32) -> bool {
    false
}

fn display_for_physical_menu_anchor(
    anchor: MenuAnchor,
    displays: &[DisplayBounds],
) -> Option<DisplayBounds> {
    displays
        .iter()
        .copied()
        .filter(|display| {
            let x = anchor.click_x / display.scale;
            let y = anchor.click_y / display.scale;
            display.contains(x, y)
        })
        .min_by(|left, right| {
            score_display_for_menu_anchor(anchor, *left)
                .total_cmp(&score_display_for_menu_anchor(anchor, *right))
        })
}

fn score_display_for_menu_anchor(anchor: MenuAnchor, display: DisplayBounds) -> f64 {
    let icon_height = anchor.rect_height / display.scale;
    let height_score = if (16.0..=36.0).contains(&icon_height) {
        (icon_height - 22.0).abs()
    } else if icon_height < 16.0 {
        100.0 + (16.0 - icon_height)
    } else {
        100.0 + (icon_height - 36.0)
    };

    let rect_center_x = (anchor.rect_x + anchor.rect_width / 2.0) / display.scale;
    let rect_bottom_y = (anchor.rect_y + anchor.rect_height) / display.scale;
    let rect_score = if display.contains(rect_center_x, rect_bottom_y) {
        0.0
    } else {
        8.0
    };

    height_score + rect_score
}

fn inferred_status_scale(rect_height: f64) -> f64 {
    if rect_height >= 36.0 { 2.0 } else { 1.0 }
}

fn clamp_window_x_to_displays(
    desired_x: f64,
    anchor_x: f64,
    anchor_y: f64,
    window_width: f64,
    margin: f64,
    fallback_monitor_width: Option<f64>,
    displays: &[DisplayBounds],
) -> f64 {
    let display = display_for_anchor(anchor_x, anchor_y, displays)
        .or_else(|| fallback_display_bounds(anchor_x, fallback_monitor_width));
    let Some(display) = display else {
        return desired_x.max(margin);
    };

    let min_x = display.min_x + margin;
    let max_x = display.max_x - window_width - margin;

    if max_x < min_x {
        min_x
    } else {
        desired_x.clamp(min_x, max_x)
    }
}

fn display_for_anchor(
    anchor_x: f64,
    anchor_y: f64,
    displays: &[DisplayBounds],
) -> Option<DisplayBounds> {
    displays
        .iter()
        .copied()
        .find(|display| {
            (display.min_x..=display.max_x).contains(&anchor_x)
                && (display.min_y..=display.max_y).contains(&anchor_y)
        })
        .or_else(|| {
            displays
                .iter()
                .copied()
                .find(|display| (display.min_x..=display.max_x).contains(&anchor_x))
        })
}

fn fallback_display_bounds(
    anchor_x: f64,
    fallback_monitor_width: Option<f64>,
) -> Option<DisplayBounds> {
    let width = fallback_monitor_width.filter(|width| *width > 1.0)?;
    let min_x = (anchor_x / width).floor() * width;

    Some(DisplayBounds {
        min_x,
        max_x: min_x + width,
        min_y: f64::NEG_INFINITY,
        max_y: f64::INFINITY,
        scale: 1.0,
    })
}

#[cfg(target_os = "macos")]
fn active_display_bounds() -> Vec<DisplayBounds> {
    use core_graphics::display::CGDisplay;

    let Ok(display_ids) = CGDisplay::active_displays() else {
        return Vec::new();
    };

    display_ids
        .into_iter()
        .filter_map(|id| {
            let display = CGDisplay::new(id);
            let bounds = display.bounds();
            let min_x = bounds.origin.x;
            let min_y = bounds.origin.y;
            let max_x = min_x + bounds.size.width;
            let max_y = min_y + bounds.size.height;
            let scale = display
                .display_mode()
                .and_then(|mode| {
                    let width = mode.width();
                    (width > 0).then_some(mode.pixel_width() as f64 / width as f64)
                })
                .filter(|scale| scale.is_finite() && *scale > 0.0)
                .unwrap_or(1.0);

            if max_x <= min_x || max_y <= min_y {
                None
            } else {
                Some(DisplayBounds {
                    min_x,
                    max_x,
                    min_y,
                    max_y,
                    scale,
                })
            }
        })
        .collect()
}

#[cfg(not(target_os = "macos"))]
fn active_display_bounds() -> Vec<DisplayBounds> {
    Vec::new()
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
        let now = Instant::now();
        self.sync_appearance(ctx, now);
        self.update_skin_transition(ctx, now);

        // Some setups surface the window on launch despite `with_visible(false)`;
        // keep it hidden until the user opens it from the menu bar icon.
        if !self.visible && self.created_at.elapsed() < STARTUP_HIDE {
            ctx.send_viewport_cmd(ViewportCommand::Visible(false));
            ctx.request_repaint_after(Duration::from_millis(50));
        }

        // Apply a queued move before the window is shown (see `show`).
        if self.pending_show {
            self.pending_show = false;
            if let Some(transition) = &mut self.animations.popover
                && transition.phase == PopoverTransitionPhase::Appearing
            {
                transition.started_at = Instant::now();
            }
            ctx.send_viewport_cmd(ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(ViewportCommand::Focus);
        }

        self.handle_events(ctx);
        self.handle_focus(ctx);
        self.update_popover_transition(ctx);

        // Poll while the popover is open, or in the background when auto-mirror
        // must watch for newly connected phones.
        let watching = self.visible || self.settings.auto_mirror;
        if watching && self.last_poll.elapsed() > STATUS_POLL {
            self.last_poll = Instant::now();
            backend::refresh_status(self.shared.clone(), ctx.clone());
        }

        self.maybe_auto_mirror(ctx);

        if watching {
            let interval = if self.visible {
                Duration::from_secs(1)
            } else {
                STATUS_POLL
            };
            ctx.request_repaint_after(interval);
        }

        let pairing_done = {
            let state = self.shared.lock().unwrap();
            self.screen == Screen::Pair && state.pairing.is_none()
        };
        if pairing_done {
            self.navigate_to(Screen::Main, ctx);
        }
    }

    fn ui(&mut self, ui: &mut Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let rect = ui.max_rect();
        let opacity = if !self.visible {
            0.0
        } else if let Some(transition) = self.animations.popover {
            let progress = transition.started_at.elapsed().as_secs_f32()
                / popover_transition_duration(transition.phase).as_secs_f32();
            popover_transition_opacity(transition.phase, progress)
        } else {
            1.0
        };
        if !set_native_window_opacity(frame, opacity) {
            ui.set_opacity(opacity);
        }

        // Beak + rounded body + gradient + hairline, baked into one texture.
        ui.painter().image(
            self.night_shell.id(),
            rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
        let day_alpha = (self.animations.skin.day_weight * 255.0).round() as u8;
        if day_alpha > 0 {
            ui.painter().image(
                self.day_shell.id(),
                rect,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::from_white_alpha(day_alpha),
            );
        }

        let content = Rect::from_min_max(
            egui::pos2(rect.left() + 16.0, rect.top() + theme::BEAK_HEIGHT + 14.0),
            egui::pos2(rect.right() - 16.0, rect.bottom() - 14.0),
        );
        if let Some(transition) = self.animations.screen {
            let progress = (transition.started_at.elapsed().as_secs_f32()
                / SCREEN_TRANSITION_DURATION.as_secs_f32())
            .clamp(0.0, 1.0);

            if progress >= 1.0 {
                self.animations.screen = None;
                if transition.cancel_pairing_on_complete {
                    backend::cancel_pairing(&self.shared);
                }
                self.render_screen(ui, content, &ctx, transition.to, 0.0, 1.0);
            } else {
                let eased = egui::emath::easing::cubic_in_out(progress);
                let outgoing_x = -transition.direction * SCREEN_OUTGOING_OFFSET * eased;
                let incoming_x = transition.direction * SCREEN_INCOMING_OFFSET * (1.0 - eased);

                self.render_screen(ui, content, &ctx, transition.from, outgoing_x, 1.0 - eased);
                self.render_screen(ui, content, &ctx, transition.to, incoming_x, eased);
                ctx.request_repaint();
            }
        } else {
            self.render_screen(ui, content, &ctx, self.screen, 0.0, 1.0);
        }
    }
}

impl App {
    fn render_screen(
        &mut self,
        ui: &mut Ui,
        content: Rect,
        ctx: &Context,
        screen: Screen,
        offset_x: f32,
        opacity: f32,
    ) {
        let mut screen_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt(("screen", screen))
                .max_rect(content.translate(vec2(offset_x, 0.0)))
                .layout(Layout::top_down(Align::Min)),
        );
        screen_ui.set_clip_rect(content);
        screen_ui.set_opacity(opacity);
        if self.animations.screen.is_some() {
            // Both screens are painted during the transition. Keep their
            // visual treatment intact while preventing clicks from reaching
            // either the outgoing or incoming controls mid-animation.
            screen_ui.visuals_mut().disabled_alpha = 1.0;
            screen_ui.disable();
        }

        match screen {
            Screen::Main => self.main_screen(&mut screen_ui, ctx),
            Screen::Settings => self.settings_screen(&mut screen_ui, ctx),
            Screen::Pair => self.pair_screen(&mut screen_ui, ctx),
        }
    }

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
                ui.add(Label::new(semibold("awb", 15.0, theme::text_strong())).selectable(false));
                ui.add(
                    Label::new(regular("Android Wireless Bridge", 11.0, theme::text_soft()))
                        .selectable(false),
                );
            });

            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if icon_button(ui, ph::QR_CODE, 14.0, theme::text_muted()).clicked() {
                    self.open_pairing(ctx);
                }
                ui.add_space(10.0);
                if icon_button(ui, ph::GEAR_SIX, 14.0, theme::text_muted()).clicked() {
                    self.navigate_to(Screen::Settings, ctx);
                }
                ui.add_space(10.0);
                if icon_button(ui, ph::ARROWS_CLOCKWISE, 14.0, theme::text_muted()).clicked() {
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
        let (snapshot, mirrors, starting_avds): (Option<Snapshot>, Vec<String>, HashSet<String>) = {
            let state = self.shared.lock().unwrap();
            let mut mirrors = state.mirrors.keys().cloned().collect::<Vec<_>>();
            mirrors.extend(state.starting_mirrors.iter().cloned());
            (state.snapshot.clone(), mirrors, state.starting_avds.clone())
        };

        let Some(snapshot) = snapshot else {
            ui.add_space(24.0);
            ui.vertical_centered(|ui| {
                ui.add(Label::new(regular(
                    "Checking devices…",
                    11.0,
                    theme::text_faint(),
                )));
            });
            return;
        };

        if snapshot.devices.is_empty() && snapshot.avds.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.add(Label::new(regular(
                    "No devices connected",
                    12.5,
                    theme::text_muted(),
                )));
                ui.add_space(6.0);
                ui.add(Label::new(regular(
                    "Tap the QR icon to pair a phone over Wi-Fi",
                    11.0,
                    theme::text_faint(),
                )));
            });
            return;
        }

        let scrcpy_ok = snapshot.scrcpy.available;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                for (index, device) in snapshot.devices.iter().enumerate() {
                    if index > 0 {
                        divider(ui);
                    }

                    let mirroring = mirrors.contains(&device.mirror_key);
                    let action = if mirroring {
                        RowAction::enabled(ph::STOP, theme::green())
                    } else if device.ready && scrcpy_ok {
                        RowAction::enabled(ph::PLAY, theme::text_bright())
                    } else {
                        RowAction::disabled(ph::PLAY)
                    };
                    let detail = if device.ready {
                        device.serial.clone()
                    } else {
                        format!("{} · {}", device.serial, device.state)
                    };

                    let row_icon = if device.is_emulator {
                        ph::DESKTOP
                    } else {
                        ph::DEVICE_MOBILE
                    };
                    if list_row(ui, row_icon, &device.name, &detail, Some(action)).clicked() {
                        if mirroring {
                            backend::stop_mirror(&self.shared, &device.mirror_key);
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

                for (index, avd) in snapshot.avds.iter().enumerate() {
                    if index > 0 || !snapshot.devices.is_empty() {
                        divider(ui);
                    }

                    let starting = starting_avds.contains(avd);
                    let action = if starting {
                        RowAction::disabled(ph::PLAY)
                    } else {
                        RowAction::enabled(ph::PLAY, theme::text_bright())
                    };
                    let name = avd.replace('_', " ");
                    let detail = if starting {
                        "Starting…"
                    } else {
                        "Android Virtual Device"
                    };

                    if list_row(ui, ph::DESKTOP, &name, detail, Some(action)).clicked() {
                        backend::start_avd(self.shared.clone(), ctx.clone(), avd.clone());
                    }
                }
            });
    }

    fn logs_tab(&mut self, ui: &mut Ui) {
        let logs: Vec<String> = self.shared.lock().unwrap().logs.clone();
        let empty = logs.is_empty();
        let mut log_text = if empty {
            "No output yet.".to_string()
        } else {
            logs.join("\n")
        };

        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.add(
                    TextEdit::multiline(&mut log_text)
                        .desired_width(f32::INFINITY)
                        .desired_rows(logs.len().max(1))
                        .font(FontId::new(10.5, FontFamily::Monospace))
                        .text_color(if empty {
                            theme::text_faint()
                        } else {
                            theme::text_check()
                        })
                        .frame(Frame::NONE),
                );
            });
    }

    fn settings_screen(&mut self, ui: &mut Ui, ctx: &Context) {
        if nav_header(ui, "Settings").clicked() {
            self.navigate_to(Screen::Main, ctx);
        }
        ui.add_space(12.0);

        let scroll_rect = ui.available_rect_before_wrap();
        let scroll_input_active = ctx.input(|input| {
            let pointer_over = input
                .pointer
                .hover_pos()
                .is_some_and(|position| scroll_rect.contains(position));
            pointer_over
                && (input.is_scrolling()
                    || input.time_since_last_scroll() <= input.predicted_dt.max(0.05))
        });
        if scroll_input_active {
            self.settings_scroll_active_at = Some(Instant::now());
        }

        let scrollbar_opacity = self
            .settings_scroll_active_at
            .map(scrollbar_opacity)
            .unwrap_or(0.0);
        if let Some(active_at) = self.settings_scroll_active_at
            && let Some(remaining) = SCROLLBAR_HIDE_DELAY.checked_sub(active_at.elapsed())
        {
            ctx.request_repaint_after(remaining);
            if remaining <= SCROLLBAR_OPACITY_FADE {
                ctx.request_repaint_after(Duration::from_millis(16));
            }
        }

        let output = egui::ScrollArea::vertical()
            .id_salt("settings-scroll")
            .auto_shrink([false, false])
            .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.add(
                    Label::new(semibold("Screen Mirroring", 12.5, theme::text_bright()))
                        .selectable(false),
                );
                ui.add_space(10.0);

                let mut changed = false;
                ui.horizontal(|ui| {
                    let total = ui.available_width();
                    let title_width = total - 2.0 * 64.0 - 2.0 * 8.0;
                    changed |=
                        labeled_input(ui, "Title", title_width, &mut self.settings.window_title);
                    ui.add_space(8.0);
                    changed |= labeled_input(ui, "W", 64.0, &mut self.width_text);
                    ui.add_space(8.0);
                    changed |= labeled_input(ui, "H", 64.0, &mut self.height_text);
                });

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    changed |= check_item(ui, "Always on top", &mut self.settings.always_on_top);
                    ui.add_space(20.0);
                    changed |= check_item(ui, "Borderless", &mut self.settings.borderless);
                    ui.add_space(20.0);
                    changed |= check_item(ui, "Auto mirror", &mut self.settings.auto_mirror);
                });

                ui.add_space(12.0);
                divider(ui);
                ui.add_space(12.0);

                ui.add(
                    Label::new(semibold("General", 12.5, theme::text_bright())).selectable(false),
                );
                ui.add_space(10.0);

                let mut theme_changed = false;
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), THEME_MODE_GROUP_SIZE.y),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.add(
                            Label::new(regular("Appearance", 12.0, theme::text_check()))
                                .selectable(false),
                        );
                        theme_changed |= ui
                            .with_layout(Layout::right_to_left(Align::Center), |ui| {
                                theme_mode_group(ui, &mut self.settings.theme)
                            })
                            .inner;
                    },
                );
                changed |= theme_changed;

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

                if changed {
                    self.save_settings();
                }
                if theme_changed {
                    ctx.request_repaint();
                }

                ui.add_space(12.0);
                divider(ui);
                ui.add_space(12.0);

                ui.add(
                    Label::new(semibold("Dependencies", 12.5, theme::text_bright()))
                        .selectable(false),
                );

                let snapshot = self.shared.lock().unwrap().snapshot.clone();
                let (adb, emulator, scrcpy) = match &snapshot {
                    Some(snapshot) => (
                        Some(&snapshot.adb),
                        Some(&snapshot.emulator),
                        Some(&snapshot.scrcpy),
                    ),
                    None => (None, None, None),
                };

                dependency_row(ui, ph::TERMINAL_WINDOW, "ADB", adb);
                divider(ui);
                dependency_row(ui, ph::DESKTOP, "Android Emulator", emulator);
                divider(ui);
                dependency_row(ui, ph::MONITOR_PLAY, "scrcpy", scrcpy);
            });

        let scroll_moving = (output.state.offset.y - self.settings_scroll_offset).abs() > 0.01
            || output.state.velocity().y.abs() > 0.01;
        self.settings_scroll_offset = output.state.offset.y;
        if scroll_moving {
            self.settings_scroll_active_at = Some(Instant::now());
            ctx.request_repaint_after(SCROLLBAR_HIDE_DELAY);
        }

        paint_settings_scroll_chrome(
            ui,
            output.inner_rect,
            output.content_size.y,
            output.state.offset.y,
            scrollbar_opacity,
        );
    }

    fn pair_screen(&mut self, ui: &mut Ui, ctx: &Context) {
        if nav_header(ui, "Pair device").clicked() {
            self.leave_pairing(ctx);
            return;
        }

        let phase = {
            let state = self.shared.lock().unwrap();
            state.pairing.as_ref().map(|session| session.phase.clone())
        };

        let Some(phase) = phase else {
            self.navigate_to(Screen::Main, ctx);
            return;
        };

        match phase {
            PairingPhase::Qr { modules, progress } => {
                let content_height = 168.0 + 14.0 + 18.0 + 4.0 + 30.0 + 12.0 + 50.0;
                center_pad_at_least(ui, content_height, 24.0);

                ui.vertical_centered(|ui| {
                    let (rect, _) = ui.allocate_exact_size(vec2(168.0, 168.0), Sense::hover());
                    let painter = ui.painter();
                    painter.rect_filled(rect, 12.0, theme::qr_card());

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
                                    theme::qr_ink(),
                                );
                            }
                        }
                    }

                    ui.add_space(14.0);
                    ui.add(Label::new(semibold(
                        "Scan with your phone",
                        13.0,
                        theme::text_bright(),
                    )));
                    ui.add_space(4.0);
                    hint_label(
                        ui,
                        "Developer options → Wireless debugging → Pair device with QR code",
                        300.0,
                    );
                    ui.add_space(12.0);
                    pairing_progress_block(ui, &progress, 310.0);
                });
            }
            PairingPhase::Connecting { progress } => {
                center_pad(ui, 28.0 + 12.0 + 18.0 + 4.0 + 34.0 + 10.0 + 32.0);
                ui.vertical_centered(|ui| {
                    ui.add(egui::Spinner::new().size(28.0).color(theme::green()));
                    ui.add_space(12.0);
                    ui.add(Label::new(semibold(
                        progress.title.clone(),
                        13.0,
                        theme::text_bright(),
                    )));
                    ui.add_space(4.0);
                    hint_label(ui, &progress.detail, 300.0);
                    ui.add_space(10.0);
                    pairing_progress_block(ui, &progress, 310.0);
                });
            }
            PairingPhase::Failed { message } => {
                center_pad(ui, 28.0 + 12.0 + 18.0 + 4.0 + 32.0 + 36.0);
                ui.vertical_centered(|ui| {
                    ui.add(Label::new(icon(ph::WARNING_CIRCLE, 28.0, theme::red())));
                    ui.add_space(12.0);
                    ui.add(Label::new(semibold(
                        "Pairing failed",
                        13.0,
                        theme::text_bright(),
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
                            theme::green(),
                            theme::green_ink(),
                            true,
                        )
                        .clicked()
                        {
                            backend::start_pairing(self.shared.clone(), ctx.clone());
                        }
                        ui.add_space(10.0);
                        if pill_button(
                            ui,
                            None,
                            "Cancel",
                            theme::surface(),
                            theme::text_check(),
                            false,
                        )
                        .clicked()
                        {
                            self.leave_pairing(ctx);
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
            color: theme::text_faint(),
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
        semibold(label, 12.5, theme::text_strong())
    } else {
        medium(label, 12.5, theme::text_muted())
    };

    let response = ui
        .vertical(|ui| {
            let response = ui.add(Label::new(text).selectable(false).sense(Sense::click()));
            ui.add_space(6.0);
            let (rect, _) =
                ui.allocate_exact_size(vec2(response.rect.width(), 2.0), Sense::hover());
            if active {
                ui.painter().rect_filled(rect, 1.0, theme::green());
            }
            response
        })
        .inner;

    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

fn divider(ui: &mut Ui) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 1.0), Sense::hover());
    ui.painter().rect_filled(rect, 0.0, theme::hairline());
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
        ui.add(Label::new(icon(row_icon, 14.0, theme::text_label())).selectable(false));
        ui.add_space(10.0);
        ui.add(Label::new(medium(name, 12.5, theme::text_bright())).selectable(false));
        ui.add_space(10.0);

        let reserved = if action.is_some() { 34.0 } else { 4.0 };
        ui.scope(|ui| {
            ui.set_max_width((ui.available_width() - reserved).max(20.0));
            ui.add(
                Label::new(regular(detail, 11.0, theme::text_faint()))
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
                ui.painter().rect_filled(rect, 6.0, theme::surface());
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
        ui.add(Label::new(icon(row_icon, 14.0, theme::text_label())).selectable(false));
        ui.add_space(10.0);
        ui.add(Label::new(medium(name, 12.5, theme::text_bright())).selectable(false));
        ui.add_space(10.0);

        let warning = info.and_then(|tool| tool.warnings.first());
        let detail = match (info, warning) {
            (_, Some(warning)) => warning.clone(),
            (Some(tool), None) => tool.detail.clone(),
            (None, None) => "checking…".to_string(),
        };
        let available_width = ui.available_width() - 50.0;
        ui.scope(|ui| {
            ui.set_max_width(available_width.max(40.0));
            ui.add(
                Label::new(regular(
                    detail,
                    11.0,
                    if warning.is_some() {
                        theme::amber()
                    } else {
                        theme::text_faint()
                    },
                ))
                .truncate()
                .selectable(false),
            );
        });

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add_space(2.0);
            match info {
                Some(tool) if tool.available && tool.warnings.is_empty() => {
                    ui.add(Label::new(regular("Ready", 11.0, theme::green())).selectable(false));
                }
                Some(tool) if tool.available => {
                    ui.add(Label::new(regular("Update", 11.0, theme::amber())).selectable(false));
                }
                Some(_) => {
                    ui.add(Label::new(regular("Missing", 11.0, theme::red())).selectable(false));
                }
                None => {}
            }
        });
    });
}

fn labeled_input(ui: &mut Ui, label: &str, width: f32, value: &mut String) -> bool {
    ui.vertical(|ui| {
        ui.set_width(width);
        ui.add(Label::new(regular(label, 10.5, theme::text_label())).selectable(false));
        ui.add_space(4.0);

        let frame = Frame::new()
            .fill(theme::input_bg())
            .stroke(Stroke::new(1.0_f32, Color32::TRANSPARENT))
            .shadow(Shadow {
                offset: [0, 1],
                blur: 4,
                spread: 0,
                color: theme::control_shadow(),
            })
            .corner_radius(CornerRadius::same(6))
            .inner_margin(Margin::symmetric(9, 6));

        let output = frame.show(ui, |ui| {
            ui.set_width(width - 20.0);
            ui.add(
                TextEdit::singleline(value)
                    .frame(Frame::NONE)
                    .font(FontId::new(12.0, FontFamily::Proportional))
                    .text_color(theme::text_bright())
                    .margin(Margin::ZERO)
                    .desired_width(width - 20.0),
            )
        });
        let (stroke_width, stroke_color) = if output.inner.has_focus() {
            (1.5_f32, theme::input_focus_stroke())
        } else if output.response.hovered() || output.inner.hovered() {
            (1.0_f32, theme::input_hover_stroke())
        } else {
            (1.0_f32, theme::input_stroke())
        };
        ui.painter().rect_stroke(
            output.response.rect,
            6.0,
            Stroke::new(stroke_width, stroke_color),
            egui::StrokeKind::Inside,
        );

        output.inner.changed()
    })
    .inner
}

fn scrollbar_opacity(active_at: Instant) -> f32 {
    let elapsed = active_at.elapsed();
    let Some(remaining) = SCROLLBAR_HIDE_DELAY.checked_sub(elapsed) else {
        return 0.0;
    };

    (remaining.as_secs_f32() / SCROLLBAR_OPACITY_FADE.as_secs_f32()).clamp(0.0, 1.0)
}

fn paint_settings_scroll_chrome(
    ui: &Ui,
    viewport: Rect,
    content_height: f32,
    offset: f32,
    scrollbar_opacity: f32,
) {
    let max_offset = (content_height - viewport.height()).max(0.0);
    if max_offset <= 0.5 {
        return;
    }

    let painter = ui.painter().with_clip_rect(viewport);
    let top_strength = (offset / SCROLL_EDGE_FADE_HEIGHT).clamp(0.0, 1.0);
    let bottom_strength = ((max_offset - offset) / SCROLL_EDGE_FADE_HEIGHT).clamp(0.0, 1.0);

    if top_strength > 0.0 {
        let rect = Rect::from_min_max(
            viewport.min,
            egui::pos2(viewport.right(), viewport.top() + SCROLL_EDGE_FADE_HEIGHT),
        );
        paint_vertical_gradient(
            &painter,
            rect,
            with_opacity(theme::scroll_fade_top(), top_strength),
            Color32::TRANSPARENT,
        );
    }

    if bottom_strength > 0.0 {
        let rect = Rect::from_min_max(
            egui::pos2(viewport.left(), viewport.bottom() - SCROLL_EDGE_FADE_HEIGHT),
            viewport.max,
        );
        paint_vertical_gradient(
            &painter,
            rect,
            Color32::TRANSPARENT,
            with_opacity(theme::scroll_fade_bottom(), bottom_strength),
        );
    }

    if scrollbar_opacity > 0.0
        && let Some(handle) = settings_scrollbar_rect(viewport, content_height, offset)
    {
        painter.rect_filled(
            handle,
            SCROLLBAR_HANDLE_WIDTH / 2.0,
            with_opacity(theme::scrollbar_thumb(), scrollbar_opacity),
        );
    }
}

fn settings_scrollbar_rect(viewport: Rect, content_height: f32, offset: f32) -> Option<Rect> {
    let max_offset = content_height - viewport.height();
    if max_offset <= 0.5 {
        return None;
    }

    let track_top = viewport.top() + SCROLLBAR_TRACK_INSET;
    let track_height = (viewport.height() - 2.0 * SCROLLBAR_TRACK_INSET).max(0.0);
    let handle_height = SCROLLBAR_HANDLE_HEIGHT.min(track_height);
    let travel = (track_height - handle_height).max(0.0);
    let progress = (offset / max_offset).clamp(0.0, 1.0);
    let top = track_top + travel * progress;

    Some(Rect::from_min_size(
        egui::pos2(viewport.right() - SCROLLBAR_HANDLE_WIDTH - 1.0, top),
        vec2(SCROLLBAR_HANDLE_WIDTH, handle_height),
    ))
}

fn paint_vertical_gradient(painter: &egui::Painter, rect: Rect, top: Color32, bottom: Color32) {
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);
    painter.add(egui::Shape::mesh(mesh));
}

fn with_opacity(color: Color32, opacity: f32) -> Color32 {
    let [red, green, blue, alpha] = color.to_srgba_unmultiplied();
    Color32::from_rgba_unmultiplied(
        red,
        green,
        blue,
        (f32::from(alpha) * opacity.clamp(0.0, 1.0)).round() as u8,
    )
}

fn theme_mode_group(ui: &mut Ui, selected: &mut ThemeMode) -> bool {
    let mut changed = false;
    let modes = [
        (ThemeMode::Auto, "Auto"),
        (ThemeMode::Day, "Day"),
        (ThemeMode::Night, "Night"),
    ];

    ui.allocate_ui_with_layout(
        THEME_MODE_GROUP_SIZE,
        Layout::left_to_right(Align::Center),
        |ui| {
            Frame::new()
                .fill(theme::segment_bg())
                .stroke(Stroke::new(1.0_f32, theme::input_stroke()))
                .shadow(Shadow {
                    offset: [0, 1],
                    blur: 4,
                    spread: 0,
                    color: theme::control_shadow(),
                })
                .corner_radius(CornerRadius::same(8))
                .inner_margin(Margin::same(2))
                .show(ui, |ui| {
                    for (mode, label) in modes {
                        let active = *selected == mode;
                        let text = if active {
                            semibold(label, 11.5, theme::text_strong())
                        } else {
                            medium(label, 11.5, theme::text_muted())
                        };
                        let response = ui.add_sized(
                            [56.0, 24.0],
                            Button::new(text)
                                .fill(if active {
                                    theme::segment_selected()
                                } else {
                                    Color32::TRANSPARENT
                                })
                                .stroke(if active {
                                    Stroke::new(1.0_f32, theme::segment_selected_stroke())
                                } else {
                                    Stroke::NONE
                                })
                                .corner_radius(CornerRadius::same(6)),
                        );
                        if response.clicked() && !active {
                            *selected = mode;
                            changed = true;
                        }
                        response.on_hover_cursor(egui::CursorIcon::PointingHand);
                    }
                });
        },
    );

    changed
}

fn check_item(ui: &mut Ui, label: &str, value: &mut bool) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        let (rect, response) = ui.allocate_exact_size(vec2(15.0, 15.0), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

        ui.add_space(7.0);
        let label_response = ui
            .add(
                Label::new(regular(label, 12.0, theme::text_check()))
                    .selectable(false)
                    .sense(Sense::click()),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);

        let painter = ui.painter();
        painter.add(
            Shadow {
                offset: [0, 1],
                blur: 3,
                spread: 0,
                color: theme::control_shadow(),
            }
            .as_shape(rect, 4.0),
        );
        if *value {
            painter.rect_filled(rect, 4.0, theme::green());
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                ph::CHECK,
                theme::icon_font(10.0),
                theme::green_ink(),
            );
        } else {
            painter.rect_filled(rect, 4.0, theme::input_bg());
            painter.rect_stroke(
                rect,
                4.0,
                Stroke::new(
                    1.25_f32,
                    if response.hovered() || label_response.hovered() {
                        theme::input_hover_stroke()
                    } else {
                        theme::check_stroke()
                    },
                ),
                egui::StrokeKind::Inside,
            );
        }

        if response.clicked() || label_response.clicked() {
            *value = !*value;
            changed = true;
        }
    });

    changed
}

fn nav_header(ui: &mut Ui, title: &str) -> egui::Response {
    ui.horizontal(|ui| {
        let response = icon_button(ui, ph::CARET_LEFT, 16.0, theme::text_muted());
        ui.add_space(10.0);
        ui.add(Label::new(semibold(title, 15.0, theme::text_strong())).selectable(false));
        response
    })
    .inner
}

fn center_pad(ui: &mut Ui, content_height: f32) {
    let pad = (ui.available_height() - content_height) / 2.0;
    ui.add_space(pad.max(0.0));
}

fn center_pad_at_least(ui: &mut Ui, content_height: f32, min_top: f32) {
    let pad = (ui.available_height() - content_height) / 2.0;
    ui.add_space(pad.max(min_top));
}

fn hint_label(ui: &mut Ui, text: &str, width: f32) {
    ui.scope(|ui| {
        ui.set_max_width(width);
        ui.add(
            Label::new(regular(text, 11.0, theme::text_soft()))
                .halign(egui::Align::Center)
                .wrap(),
        );
    });
}

fn pairing_progress_block(ui: &mut Ui, progress: &PairingProgress, width: f32) {
    if progress.deadline.is_some() {
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }

    let meta = pairing_progress_meta(progress);
    if !meta.is_empty() {
        ui.scope(|ui| {
            ui.set_max_width(width);
            ui.add(
                Label::new(medium(meta, 11.0, theme::green()))
                    .halign(egui::Align::Center)
                    .wrap(),
            );
        });
    }

    if let Some(endpoint) = &progress.endpoint {
        ui.add_space(4.0);
        ui.scope(|ui| {
            ui.set_max_width(width);
            ui.add(
                Label::new(regular(endpoint, 10.5, theme::text_faint()))
                    .halign(egui::Align::Center)
                    .wrap(),
            );
        });
    }
}

fn pairing_progress_meta(progress: &PairingProgress) -> String {
    let mut parts = Vec::new();

    if let Some(deadline) = progress.deadline {
        let seconds = display_remaining_seconds(deadline);
        parts.push(format!("{seconds}s remaining"));
    }

    if let Some(attempt) = progress.attempt {
        parts.push(format!("attempt {attempt}"));
    }

    parts.join(" · ")
}

fn display_remaining_seconds(deadline: Instant) -> u64 {
    let remaining = deadline.saturating_duration_since(Instant::now());

    if remaining.is_zero() {
        0
    } else {
        remaining.as_secs() + u64::from(remaining.subsec_nanos() > 0)
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: f64 = 380.0;
    const MARGIN: f64 = 8.0;

    #[test]
    fn automatic_theme_follows_the_system() {
        assert_eq!(
            resolved_appearance(ThemeMode::Auto, Some(Theme::Light)),
            theme::Appearance::Day
        );
        assert_eq!(
            resolved_appearance(ThemeMode::Auto, Some(Theme::Dark)),
            theme::Appearance::Night
        );
    }

    #[test]
    fn explicit_theme_overrides_the_system() {
        assert_eq!(
            resolved_appearance(ThemeMode::Day, Some(Theme::Dark)),
            theme::Appearance::Day
        );
        assert_eq!(
            resolved_appearance(ThemeMode::Night, Some(Theme::Light)),
            theme::Appearance::Night
        );
    }

    #[test]
    fn automatic_theme_falls_back_to_night_when_unknown() {
        assert_eq!(
            resolved_appearance(ThemeMode::Auto, None),
            theme::Appearance::Night
        );
    }

    #[test]
    fn committing_skin_preserves_overlapping_screen_and_popover_animations() {
        let started_at = Instant::now();
        let screen = ScreenTransition {
            from: Screen::Main,
            to: Screen::Settings,
            direction: 1.0,
            started_at,
            cancel_pairing_on_complete: false,
        };
        let popover = PopoverTransition {
            phase: PopoverTransitionPhase::Appearing,
            started_at,
        };
        let mut animations = AnimationState {
            screen: Some(screen),
            popover: Some(popover),
            skin: SkinState {
                appearance: theme::Appearance::Night,
                day_weight: 0.75,
                transition: Some(SkinTransition {
                    from_day_weight: 0.0,
                    target: theme::Appearance::Day,
                    started_at,
                }),
            },
        };

        assert!(animations.commit_skin(theme::Appearance::Day));
        assert_eq!(animations.skin.appearance, theme::Appearance::Day);
        assert_eq!(animations.skin.day_weight, 1.0);
        assert!(animations.skin.transition.is_none());
        assert_eq!(animations.screen, Some(screen));
        assert_eq!(animations.popover, Some(popover));

        assert!(!animations.commit_skin(theme::Appearance::Day));
        assert_eq!(animations.screen, Some(screen));
        assert_eq!(animations.popover, Some(popover));
    }

    #[test]
    fn skin_transition_commits_once_at_the_endpoint() {
        let started_at = Instant::now();
        let mut animations = AnimationState::new(theme::Appearance::Night);
        assert!(
            animations
                .skin
                .set_target_at(theme::Appearance::Day, started_at)
        );

        let before = animations
            .advance_skin_at(started_at + SKIN_TRANSITION_DURATION - Duration::from_nanos(1));
        assert!(before.animating);
        assert_eq!(before.committed, None);

        let endpoint = animations.advance_skin_at(started_at + SKIN_TRANSITION_DURATION);
        assert!(!endpoint.animating);
        assert_eq!(endpoint.committed, Some(theme::Appearance::Day));
        assert_eq!(endpoint.day_weight, 1.0);

        let after = animations
            .advance_skin_at(started_at + SKIN_TRANSITION_DURATION + Duration::from_millis(1));
        assert!(!after.animating);
        assert_eq!(after.committed, None);
        assert_eq!(after.day_weight, 1.0);
    }

    #[test]
    fn screen_transitions_push_forward_and_pull_back() {
        assert_eq!(
            screen_transition_direction(Screen::Main, Screen::Settings),
            1.0
        );
        assert_eq!(screen_transition_direction(Screen::Main, Screen::Pair), 1.0);
        assert_eq!(
            screen_transition_direction(Screen::Settings, Screen::Main),
            -1.0
        );
    }

    #[test]
    fn popover_fades_in_and_out_without_overshooting_opacity() {
        assert_eq!(
            popover_transition_opacity(PopoverTransitionPhase::Appearing, 0.0),
            0.0
        );
        assert_eq!(
            popover_transition_opacity(PopoverTransitionPhase::Appearing, 1.0),
            1.0
        );
        assert_eq!(
            popover_transition_opacity(PopoverTransitionPhase::Disappearing, 0.0),
            1.0
        );
        assert_eq!(
            popover_transition_opacity(PopoverTransitionPhase::Disappearing, 1.0),
            0.0
        );
        assert!(popover_transition_opacity(PopoverTransitionPhase::Appearing, 0.5) > 0.5);
        assert!(popover_transition_opacity(PopoverTransitionPhase::Disappearing, 0.5) > 0.5);
    }

    #[test]
    fn custom_scrollbar_keeps_a_short_handle_and_tracks_progress() {
        let viewport = Rect::from_min_size(egui::pos2(0.0, 0.0), vec2(100.0, 200.0));

        let start = settings_scrollbar_rect(viewport, 400.0, 0.0).unwrap();
        let middle = settings_scrollbar_rect(viewport, 400.0, 100.0).unwrap();
        let end = settings_scrollbar_rect(viewport, 400.0, 200.0).unwrap();

        assert_eq!(start.height(), SCROLLBAR_HANDLE_HEIGHT);
        assert_eq!(middle.height(), SCROLLBAR_HANDLE_HEIGHT);
        assert_eq!(end.height(), SCROLLBAR_HANDLE_HEIGHT);
        assert_eq!(start.top(), viewport.top() + SCROLLBAR_TRACK_INSET);
        assert!(middle.top() > start.top());
        assert_eq!(end.bottom(), viewport.bottom() - SCROLLBAR_TRACK_INSET);
    }

    #[test]
    fn ready_mirror_keys_ignore_offline_devices() {
        let devices = [
            device_info("adb-ready._adb-tls-connect._tcp", true),
            device_info("adb-offline._adb-tls-connect._tcp", false),
        ];

        let keys = ready_device_mirror_keys(&devices);

        assert!(keys.contains("adb-ready._adb-tls-connect._tcp"));
        assert!(!keys.contains("adb-offline._adb-tls-connect._tcp"));
    }

    #[test]
    fn clamps_primary_display_edges() {
        let displays = [DisplayBounds {
            min_x: 0.0,
            max_x: 1512.0,
            min_y: 0.0,
            max_y: 982.0,
            scale: 2.0,
        }];

        let x = clamp_window_x_to_displays(1290.0, 1480.0, 24.0, WIDTH, MARGIN, None, &displays);

        assert_eq!(x, 1124.0);
    }

    #[test]
    fn keeps_popover_on_secondary_display() {
        let displays = [
            DisplayBounds {
                min_x: 0.0,
                max_x: 1512.0,
                min_y: 0.0,
                max_y: 982.0,
                scale: 2.0,
            },
            DisplayBounds {
                min_x: 1512.0,
                max_x: 3024.0,
                min_y: 0.0,
                max_y: 982.0,
                scale: 2.0,
            },
        ];

        let x = clamp_window_x_to_displays(2010.0, 2200.0, 24.0, WIDTH, MARGIN, None, &displays);

        assert_eq!(x, 2010.0);
    }

    #[test]
    fn anchors_to_status_item_center_regardless_of_click_position() {
        let displays = [DisplayBounds {
            min_x: 0.0,
            max_x: 1512.0,
            min_y: 0.0,
            max_y: 982.0,
            scale: 2.0,
        }];
        let anchor = |click_x| MenuAnchor {
            rect_x: 900.0 * 2.0,
            rect_y: 0.0,
            rect_width: 32.0 * 2.0,
            rect_height: 22.0 * 2.0,
            click_x: click_x * 2.0,
            click_y: 12.0 * 2.0,
        };

        let left_click = popover_position(anchor(904.0), None, &displays);
        let center_click = popover_position(anchor(916.0), None, &displays);
        let right_click = popover_position(anchor(928.0), None, &displays);

        assert_eq!(left_click, center_click);
        assert_eq!(right_click, center_click);
        assert_eq!(center_click, (726.0, 28.5));
    }

    #[test]
    fn anchors_to_clicked_display_when_status_rect_is_stale() {
        let displays = [
            DisplayBounds {
                min_x: 0.0,
                max_x: 1728.0,
                min_y: 0.0,
                max_y: 1117.0,
                scale: 2.0,
            },
            DisplayBounds {
                min_x: -567.0,
                max_x: 2313.0,
                min_y: -1620.0,
                max_y: 0.0,
                scale: 2.0,
            },
        ];
        let anchor = MenuAnchor {
            rect_x: 1640.0 * 2.0,
            rect_y: 0.0,
            rect_width: 32.0 * 2.0,
            rect_height: 22.0 * 2.0,
            click_x: -420.0 * 2.0,
            click_y: -1610.0 * 2.0,
        };

        let (x, y) = popover_position(anchor, None, &displays);

        assert_eq!(x, -559.0);
        assert_eq!(y, -1592.5);
    }

    #[test]
    fn prefers_status_icon_scale_for_overlapping_physical_coordinates() {
        let displays = [
            DisplayBounds {
                min_x: 0.0,
                max_x: 1512.0,
                min_y: 0.0,
                max_y: 982.0,
                scale: 2.0,
            },
            DisplayBounds {
                min_x: 1512.0,
                max_x: 3024.0,
                min_y: 0.0,
                max_y: 982.0,
                scale: 1.0,
            },
        ];
        let anchor = MenuAnchor {
            rect_x: 2184.0,
            rect_y: 0.0,
            rect_width: 32.0,
            rect_height: 22.0,
            click_x: 2200.0,
            click_y: 12.0,
        };

        let logical = logical_menu_anchor(anchor, &displays);

        assert_eq!(logical.x, 2200.0);
        assert_eq!(logical.bottom_y, 22.0);
    }

    #[test]
    fn falls_back_to_anchor_coordinate_span_without_native_displays() {
        let x = clamp_window_x_to_displays(2010.0, 2200.0, 24.0, WIDTH, MARGIN, Some(1512.0), &[]);

        assert_eq!(x, 2010.0);
    }

    fn device_info(mirror_key: &str, ready: bool) -> backend::DeviceInfo {
        backend::DeviceInfo {
            serial: mirror_key.to_string(),
            mirror_key: mirror_key.to_string(),
            name: "Pixel".to_string(),
            ready,
            state: if ready { "device" } else { "offline" }.to_string(),
            is_emulator: false,
        }
    }
}
