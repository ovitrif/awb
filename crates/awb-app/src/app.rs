use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::containers::scroll_area::ScrollBarVisibility;
use eframe::egui::{
    self, Align, Color32, Context, CornerRadius, FontFamily, FontId, Frame, Label, Layout, Margin,
    Rect, Sense, Shadow, Stroke, TextEdit, TextureHandle, TextureOptions, Theme, Ui,
    ViewportCommand, vec2,
};
use egui_phosphor::regular as ph;
use menu_icon::menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem};
use menu_icon::{
    Icon, MenuBarIcon, MenuBarIconBuilder, MenuBarIconEvent, MouseButton, MouseButtonState,
};

use crate::backend::{self, PairingPhase, PairingProgress, Shared};
use crate::config::{Settings, ThemeMode};
use crate::glyph;
use crate::login_item;
use crate::theme::{self, icon, medium, regular, semibold};

const FOCUS_GRACE: Duration = Duration::from_millis(300);
const STATUS_POLL: Duration = Duration::from_secs(5);
const SCREEN_TRANSITION_DURATION: Duration = Duration::from_millis(280);
const SKIN_TRANSITION_DURATION: Duration = Duration::from_millis(180);
const POPOVER_APPEAR_DURATION: Duration = Duration::from_millis(160);
const POPOVER_HIDE_DURATION: Duration = Duration::from_millis(120);
const THEME_MODE_GROUP_SIZE: egui::Vec2 = vec2(172.0, 28.0);
const SCROLL_EDGE_FADE_HEIGHT: f32 = 22.0;
/// How far scroll viewports reach into the side margins: room for hover
/// shapes and focus rings at the content edge, and for the scrollbar.
const SCROLL_BLEED: f32 = 9.0;
const SCROLLBAR_MIN_HANDLE: f32 = 28.0;
const SCROLLBAR_WIDTH: f32 = 3.0;
const SCROLLBAR_HOVER_WIDTH: f32 = 6.0;
const SCROLLBAR_HIT_WIDTH: f32 = 9.0;
const SCROLLBAR_TRACK_INSET: f32 = 4.0;
/// Seconds the scrollbar stays after scrolling stops, then its fade length.
const SCROLLBAR_LINGER: f64 = 0.8;
const SCROLLBAR_FADE: f64 = 0.25;
const HEADER_HEIGHT: f32 = 26.0;
const HEADER_GAP: f32 = 10.0;
const HEADER_BUTTON_SIZE: f32 = 26.0;
/// Width of the logo or back caret ahead of the page title.
const HEADER_LEADING_WIDTH: f32 = 32.0;
const HEADER_BUTTON_GAP: f32 = 6.0;
const SHELL_OVERSAMPLE: u32 = 3;
const ROW_HEIGHT: f32 = 38.0;
const ROW_ACTION_SIZE: f32 = 22.0;
const ROW_ACTION_GAP: f32 = 4.0;
const STATUS_COLUMN_WIDTH: f32 = 66.0;
const ROW_ICON_SIZE: f32 = 16.0;
/// How far a row's hover highlight reaches past the content column.
const ROW_HOVER_BLEED: f32 = 7.0;
/// How far pages may draw above the body, into the gap under the header.
const SCREEN_TOP_BLEED: f32 = 6.0;
const QR_CARD_SIZE: f32 = 164.0;
/// How long the Pair page shows a successful pairing before going back.
const PAIRED_HOLD: Duration = Duration::from_millis(1100);
const CONTROL_HOVER_TRANSITION: f32 = 0.14;
const CONTROL_PRESS_TRANSITION: f32 = 0.07;
/// How long after launch to keep forcing the window hidden, in case the
/// platform surfaces it despite `with_visible(false)`.
const STARTUP_HIDE: Duration = Duration::from_millis(800);

static STATUS_EVENTS: Mutex<Vec<MenuBarIconEvent>> = Mutex::new(Vec::new());
static MENU_EVENTS: Mutex<Vec<MenuEvent>> = Mutex::new(Vec::new());
static STATUS_PRIMARY_CLICK: AtomicBool = AtomicBool::new(false);

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
    /// The menu bar status item; absent in the headless drive mode.
    menu_bar: Option<MenuBar>,
    /// Headless drive mode: no window or status item, always shown.
    headless: bool,
    _menu: Menu,
    menu_icon_connected: bool,
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
    login_query: Option<std::sync::mpsc::Receiver<bool>>,
    pending_show: bool,
    pending_egui_theme: Option<theme::Appearance>,
    last_menu_anchor: Option<MenuAnchor>,
    auto_mirrored: HashSet<String>,
    pending_avd_delete: Option<String>,
    shell: Arc<ShellSampler>,
    back_stack: Vec<Screen>,
    forward_stack: Vec<Screen>,
    pending_page_step: Option<i8>,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        Self::create(cc, false)
    }

    /// The app without a window or menu bar item, shown from the start, for
    /// the headless drive mode.
    #[cfg(feature = "drive")]
    pub fn new_headless(cc: &eframe::CreationContext<'_>) -> anyhow::Result<Self> {
        Self::create(cc, true)
    }

    fn create(cc: &eframe::CreationContext<'_>, headless: bool) -> anyhow::Result<Self> {
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

        let (day_shell, night_shell, shell) = load_shells(&ctx, settings.gradients);

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

        let menu_bar = if headless {
            None
        } else {
            Some(MenuBar::install(&ctx, &menu)?)
        };

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
        if needs_full_status_refresh(false, settings.auto_mirror) {
            backend::refresh_status(shared.clone(), ctx.clone());
        } else {
            backend::refresh_connection_state(shared.clone(), ctx.clone());
        }

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
            menu_bar,
            headless,
            _menu: menu,
            menu_icon_connected: false,
            show_item,
            pair_id: pair_item.id().clone(),
            refresh_id: refresh_item.id().clone(),
            quit_id: quit_item.id().clone(),
            visible: headless,
            shown_at: Instant::now(),
            focus_hidden_at: None,
            last_poll: Instant::now(),
            created_at: Instant::now(),
            open_at_login: None,
            login_query: None,
            pending_show: false,
            pending_egui_theme: None,
            last_menu_anchor: None,
            auto_mirrored: HashSet::new(),
            pending_avd_delete: None,
            shell,
            back_stack: Vec::new(),
            forward_stack: Vec::new(),
            pending_page_step: None,
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

    /// Opens `screen` as a new step in the navigation history.
    fn navigate(&mut self, screen: Screen, ctx: &Context) {
        if self.screen == screen {
            return;
        }
        self.back_stack.push(self.screen);
        self.forward_stack.clear();
        self.go(screen, ctx);
    }

    /// Steps back through history; with none left, `ordered` moves to the
    /// previous page in Main → Settings → Pair order instead of to Main.
    fn nav_back(&mut self, ctx: &Context, ordered: bool) {
        let target = self.back_stack.pop().or_else(|| {
            if ordered {
                screen_in_order(self.screen, -1)
            } else {
                (self.screen != Screen::Main).then_some(Screen::Main)
            }
        });
        if let Some(target) = target {
            self.forward_stack.push(self.screen);
            self.go(target, ctx);
        }
    }

    /// Steps forward through history, or to the next page in order.
    fn nav_forward(&mut self, ctx: &Context) {
        if let Some(target) = self
            .forward_stack
            .pop()
            .or_else(|| screen_in_order(self.screen, 1))
        {
            self.back_stack.push(self.screen);
            self.go(target, ctx);
        }
    }

    /// Moves to `screen`, starting pairing on entering Pair and cancelling it
    /// once the transition away from Pair completes.
    fn go(&mut self, screen: Screen, ctx: &Context) {
        let leaving_pair = self.screen == Screen::Pair && screen != Screen::Pair;
        self.navigate_to(screen, ctx);
        if leaving_pair {
            if let Some(transition) = &mut self.animations.screen {
                transition.cancel_pairing_on_complete = true;
            } else {
                backend::cancel_pairing(&self.shared);
            }
        }
        if screen == Screen::Pair {
            backend::start_pairing(self.shared.clone(), ctx.clone());
        }
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

        if STATUS_PRIMARY_CLICK.swap(false, Ordering::SeqCst) {
            self.last_menu_anchor = None;
            let anchor = self.menu_anchor();
            self.last_menu_anchor = anchor;
            self.toggle(ctx, anchor);
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
                self.navigate(Screen::Pair, ctx);
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
        if !focused && !self.headless && self.shown_at.elapsed() > FOCUS_GRACE {
            self.hide(ctx);
            self.focus_hidden_at = Some(Instant::now());
        }
    }

    fn handle_escape(&mut self, ctx: &Context) {
        if !self.visible || self.is_disappearing() {
            return;
        }

        let escape =
            ctx.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
        if escape {
            self.hide(ctx);
        }
    }

    fn handle_keyboard_navigation(&mut self, ctx: &Context) {
        let (tab, pointer) = ctx.input(|input| {
            (
                input.key_pressed(egui::Key::Tab),
                input.pointer.any_pressed(),
            )
        });
        if tab || pointer {
            ctx.data_mut(|data| data.insert_temp(keyboard_focus_id(), tab));
        }

        match self.pending_page_step.take() {
            Some(step) if self.visible && !self.is_disappearing() => {
                if step < 0 {
                    self.nav_back(ctx, true);
                } else {
                    self.nav_forward(ctx);
                }
            }
            _ => {}
        }
    }

    fn menu_anchor(&self) -> Option<MenuAnchor> {
        self.last_menu_anchor.or_else(|| {
            self.menu_bar.as_ref()?.icon.rect().map(|rect| {
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
            self.pending_egui_theme = Some(appearance);
        }
        // Switching egui's theme changes its text anti-aliasing, which rebuilds
        // the font atlas. eframe drops texture uploads while the window is
        // hidden, so a rebuild then left the next popover drawing text from a
        // stale atlas. Apply it only on a frame that is actually painted.
        let painted = ctx
            .input(|input| input.viewport().visible())
            .unwrap_or(true);
        if self.visible
            && painted
            && let Some(appearance) = self.pending_egui_theme.take()
        {
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

    fn sync_menu_icon(&mut self) {
        let connected = self.shared.lock().unwrap().device_connected;
        if connected == self.menu_icon_connected {
            return;
        }

        // `set_icon` clears the template flag on macOS, leaving a fixed black
        // glyph after the connection state changes. Keep every replacement a
        // template so AppKit recolors it when the menu-bar appearance changes.
        let Some(menu_bar) = &self.menu_bar else {
            return;
        };
        match menu_bar_icon(connected).and_then(|icon| {
            menu_bar
                .icon
                .set_icon_with_as_template(Some(icon), true)
                .map_err(Into::into)
        }) {
            Ok(()) => self.menu_icon_connected = connected,
            Err(error) => {
                self.shared
                    .lock()
                    .unwrap()
                    .log(format!("Menu bar icon update failed: {error:#}"));
            }
        }
    }
}

/// Hooks for the headless drive mode (see `drive.rs`).
#[cfg(feature = "drive")]
impl App {
    /// One line describing what is on screen, for drive replies.
    pub fn drive_state(&self) -> String {
        let pairing = self
            .shared
            .lock()
            .unwrap()
            .pairing
            .as_ref()
            .map_or("none", |session| match session.phase {
                PairingPhase::Qr { .. } => "qr",
                PairingPhase::Connecting { .. } => "connecting",
                PairingPhase::Failed { .. } => "failed",
                PairingPhase::Paired { .. } => "paired",
            });
        let transition = self
            .animations
            .screen
            .map_or(String::from("none"), |transition| {
                format!("{:?}->{:?}", transition.from, transition.to)
            });
        format!(
            "screen={:?} tab={:?} transition={transition} theme={:?} gradients={} pairing={pairing}",
            self.screen, self.tab, self.settings.theme, self.settings.gradients,
        )
    }

    pub fn drive_set_theme(&mut self, mode: ThemeMode) {
        self.settings.theme = mode;
    }

    pub fn drive_set_gradients(&mut self, ctx: &Context, on: bool) {
        self.settings.gradients = on;
        (self.day_shell, self.night_shell, self.shell) = load_shells(ctx, on);
    }
}

/// The menu bar status item and, on macOS, the target routing its clicks.
struct MenuBar {
    icon: MenuBarIcon,
    #[cfg(target_os = "macos")]
    _click: objc2::rc::Retained<crate::status_click::StatusClickTarget>,
}

impl MenuBar {
    fn install(ctx: &Context, menu: &Menu) -> anyhow::Result<Self> {
        let icon = menu_bar_icon(false)?;
        let builder = MenuBarIconBuilder::new()
            .with_icon(icon)
            .with_icon_as_template(true)
            .with_tooltip("awb - Android Wifi Bridge");
        #[cfg(not(target_os = "macos"))]
        let builder = builder
            .with_menu(Box::new(menu.clone()))
            .with_menu_on_left_click(false);
        let status_icon = builder.build()?;

        #[cfg(target_os = "macos")]
        let click = {
            use menu_icon::menu::ContextMenu;
            let primary_ctx = ctx.clone();
            let ns_menu = unsafe {
                objc2::rc::Retained::retain(menu.ns_menu().cast::<objc2_app_kit::NSMenu>())
            };
            status_icon
                .ns_status_item()
                .zip(ns_menu)
                .and_then(|(item, ns_menu)| {
                    crate::status_click::StatusClickTarget::install(item, ns_menu, move || {
                        STATUS_PRIMARY_CLICK.store(true, Ordering::SeqCst);
                        primary_ctx.request_repaint();
                    })
                })
                .ok_or_else(|| anyhow::anyhow!("menu bar status item unavailable"))?
        };
        #[cfg(not(target_os = "macos"))]
        let _ = ctx;

        Ok(Self {
            icon: status_icon,
            #[cfg(target_os = "macos")]
            _click: click,
        })
    }
}

fn ready_device_mirror_keys(devices: &[backend::DeviceInfo]) -> HashSet<&str> {
    devices
        .iter()
        .filter(|device| device.ready)
        .map(|device| device.mirror_key.as_str())
        .collect()
}

fn menu_bar_icon(connected: bool) -> anyhow::Result<Icon> {
    let raster = glyph::menubar_icon(44, connected);
    Ok(Icon::from_rgba(raster.rgba, raster.width, raster.height)?)
}

fn needs_full_status_refresh(visible: bool, auto_mirror: bool) -> bool {
    visible || auto_mirror
}

/// Day and night shell textures plus the sampler over both rasters.
fn load_shells(
    ctx: &Context,
    gradients: bool,
) -> (TextureHandle, TextureHandle, Arc<ShellSampler>) {
    let (day, day_raster) =
        load_shell_texture(ctx, "awb-shell-day", theme::Appearance::Day, gradients);
    let (night, night_raster) =
        load_shell_texture(ctx, "awb-shell-night", theme::Appearance::Night, gradients);
    (
        day,
        night,
        Arc::new(ShellSampler::new(night_raster, day_raster)),
    )
}

fn load_shell_texture(
    ctx: &Context,
    name: &str,
    appearance: theme::Appearance,
    gradients: bool,
) -> (TextureHandle, glyph::Raster) {
    let raster = glyph::shell_background(
        SHELL_OVERSAMPLE,
        appearance,
        theme::WINDOW_FULL_HEIGHT,
        gradients,
    );
    let image = egui::ColorImage::from_rgba_premultiplied(
        [raster.width as usize, raster.height as usize],
        &raster.rgba,
    );
    (
        ctx.load_texture(name, image, TextureOptions::LINEAR),
        raster,
    )
}

/// Both shell rasters, kept so scroll edges can fade into the exact surface
/// color beneath them instead of painting a flat band over the lit gradient.
struct ShellSampler {
    width: u32,
    height: u32,
    night: Vec<u8>,
    day: Vec<u8>,
}

impl ShellSampler {
    fn new(night: glyph::Raster, day: glyph::Raster) -> Self {
        Self {
            width: night.width,
            height: night.height,
            night: night.rgba,
            day: day.rgba,
        }
    }

    fn color_at(&self, pos: egui::Pos2, day_weight: f32) -> Color32 {
        let scale = SHELL_OVERSAMPLE as f32;
        let x = ((pos.x * scale).max(0.0) as u32).min(self.width - 1);
        let y = ((pos.y * scale).max(0.0) as u32).min(self.height - 1);
        let index = ((y * self.width + x) * 4) as usize;
        let pick = |rgba: &[u8]| {
            Color32::from_rgba_premultiplied(
                rgba[index],
                rgba[index + 1],
                rgba[index + 2],
                rgba[index + 3],
            )
        };
        pick(&self.night).lerp_to_gamma(pick(&self.day), day_weight)
    }
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

const SCREEN_ORDER: [Screen; 3] = [Screen::Main, Screen::Settings, Screen::Pair];

fn screen_in_order(from: Screen, step: isize) -> Option<Screen> {
    let index = SCREEN_ORDER.iter().position(|screen| *screen == from)?;
    SCREEN_ORDER.get(index.checked_add_signed(step)?).copied()
}

fn screen_transition_direction(from: Screen, to: Screen) -> f32 {
    let index = |screen| {
        SCREEN_ORDER
            .iter()
            .position(|candidate| *candidate == screen)
    };
    if index(to) < index(from) { -1.0 } else { 1.0 }
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

    /// ← and → move between pages. Take them before egui sees them, which
    /// would otherwise use them to move focus between controls; text fields
    /// keep them for the caret.
    fn raw_input_hook(&mut self, ctx: &Context, raw_input: &mut egui::RawInput) {
        let editing_text = ctx
            .memory(|memory| memory.focused())
            .is_some_and(|id| egui::text_edit::TextEditState::load(ctx, id).is_some());
        if !self.visible || self.pending_avd_delete.is_some() || editing_text {
            return;
        }
        raw_input.events.retain(|event| match event {
            egui::Event::Key {
                key: key @ (egui::Key::ArrowLeft | egui::Key::ArrowRight),
                pressed,
                modifiers,
                ..
            } if modifiers.is_none() => {
                if *pressed {
                    self.pending_page_step =
                        Some(if *key == egui::Key::ArrowLeft { -1 } else { 1 });
                }
                false
            }
            _ => true,
        });
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
        self.handle_escape(ctx);
        self.handle_keyboard_navigation(ctx);
        self.update_popover_transition(ctx);

        // Keep the full UI snapshot current while it is in use. When hidden,
        // a lightweight ADB-only probe is enough to update the menu bar icon.
        if self.last_poll.elapsed() > STATUS_POLL {
            self.last_poll = Instant::now();
            if needs_full_status_refresh(self.visible, self.settings.auto_mirror) {
                backend::refresh_status(self.shared.clone(), ctx.clone());
            } else {
                backend::refresh_connection_state(self.shared.clone(), ctx.clone());
            }
        }

        self.maybe_auto_mirror(ctx);
        self.sync_menu_icon();

        let interval = if self.visible {
            Duration::from_secs(1)
        } else {
            STATUS_POLL
        };
        ctx.request_repaint_after(interval);

        // A finished pairing holds its "Paired" state for a moment, then
        // returns to the device list as a step back (Main slides in from the
        // left while the Pair page slides out to the right).
        let pairing_done = {
            let state = self.shared.lock().unwrap();
            match state.pairing.as_ref().map(|session| &session.phase) {
                None => true,
                Some(PairingPhase::Paired { at, .. }) => {
                    let remaining = PAIRED_HOLD.saturating_sub(at.elapsed());
                    if !remaining.is_zero() {
                        ctx.request_repaint_after(remaining);
                    }
                    remaining.is_zero()
                }
                Some(_) => false,
            }
        };
        if self.screen == Screen::Pair && pairing_done {
            self.navigate(Screen::Main, ctx);
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
        let header = Rect::from_min_size(content.min, vec2(content.width(), HEADER_HEIGHT));
        let body = Rect::from_min_max(
            egui::pos2(content.left(), header.bottom() + HEADER_GAP),
            content.max,
        );

        let transition = self.animations.screen.map(|transition| {
            let progress = (transition.started_at.elapsed().as_secs_f32()
                / SCREEN_TRANSITION_DURATION.as_secs_f32())
            .clamp(0.0, 1.0);
            (transition, progress)
        });
        if let Some((transition, progress)) = transition
            && progress >= 1.0
        {
            self.animations.screen = None;
            if transition.cancel_pairing_on_complete {
                backend::cancel_pairing(&self.shared);
            }
        }

        match transition {
            Some((transition, progress)) if progress < 1.0 => {
                let eased = egui::emath::easing::cubic_in_out(progress);
                // A push: both pages stay opaque and move side by side, so
                // they never draw over each other.
                let distance = body.width() + 2.0 * SCROLL_BLEED + 16.0;
                let outgoing_x = -transition.direction * distance * eased;
                let incoming_x = transition.direction * distance * (1.0 - eased);

                self.header(ui, &ctx, header, transition.from, transition.to, eased);
                self.render_screen(ui, body, &ctx, transition.from, outgoing_x, 1.0);
                self.render_screen(ui, body, &ctx, transition.to, incoming_x, 1.0);
                ctx.request_repaint();
            }
            _ => {
                self.header(ui, &ctx, header, self.screen, self.screen, 1.0);
                self.render_screen(ui, body, &ctx, self.screen, 0.0, 1.0);
            }
        }
    }
}

impl App {
    /// The header stays put across pages: its title cross-fades with the page
    /// while refresh, settings and pairing remain in the top-right corner.
    /// `progress` runs from `from` to `to`; equal screens mean a settled page.
    /// Titles fade out and then in rather than overlapping, and the back caret
    /// stays put when moving between two sub-pages.
    fn header(
        &mut self,
        ui: &mut Ui,
        ctx: &Context,
        rect: Rect,
        from: Screen,
        to: Screen,
        progress: f32,
    ) {
        let settled = from == to;
        let out_opacity = (1.0 - 2.0 * progress).max(0.0);
        let in_opacity = if settled {
            1.0
        } else {
            (2.0 * progress - 1.0).max(0.0)
        };
        let has_back = |screen| screen != Screen::Main;
        let shared_back = has_back(from) && has_back(to);

        let mut title_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("header-title")
                .max_rect(rect)
                .layout(Layout::left_to_right(Align::Center)),
        );
        if !settled {
            title_ui.disable();
        }
        let leading = Rect::from_min_size(rect.min, vec2(HEADER_LEADING_WIDTH, rect.height()));
        let mut lead_ui = |ui: &mut Ui, screen: Screen, opacity: f32| {
            let mut lead = ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("header-leading", has_back(screen)))
                    .max_rect(leading)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            lead.set_opacity(opacity);
            if has_back(screen) {
                let back = icon_button(&mut lead, ph::CARET_LEFT, 14.0, theme::text_muted())
                    .on_hover_text("Back");
                if back.clicked() {
                    self.nav_back(ctx, false);
                }
            } else {
                lead.add(
                    egui::Image::new(&self.logo)
                        .fit_to_exact_size(vec2(24.0, 24.0))
                        .corner_radius(0.0),
                );
            }
        };
        if settled || shared_back {
            lead_ui(&mut title_ui, to, 1.0);
        } else {
            lead_ui(&mut title_ui, from, out_opacity);
            lead_ui(&mut title_ui, to, in_opacity);
        }

        let text_rect = Rect::from_min_max(
            egui::pos2(rect.left() + HEADER_LEADING_WIDTH, rect.top()),
            rect.max,
        );
        let titles: &[(Screen, f32)] = if settled {
            &[(to, 1.0)]
        } else {
            &[(from, out_opacity), (to, in_opacity)]
        };
        for &(screen, opacity) in titles {
            if opacity <= 0.0 {
                continue;
            }
            let mut text_ui = title_ui.new_child(
                egui::UiBuilder::new()
                    .id_salt(("header-text", screen))
                    .max_rect(text_rect)
                    .layout(Layout::left_to_right(Align::Center)),
            );
            text_ui.set_opacity(opacity);
            text_ui.add(
                Label::new(semibold(screen_title(screen), 14.0, theme::text_strong()))
                    .selectable(false),
            );
        }

        // Laid out left to right so Tab visits them in reading order.
        let actions_width = 3.0 * HEADER_BUTTON_SIZE + 2.0 * HEADER_BUTTON_GAP;
        let mut actions_ui = ui.new_child(
            egui::UiBuilder::new()
                .id_salt("header-actions")
                .max_rect(Rect::from_min_max(
                    egui::pos2(rect.right() - actions_width, rect.top()),
                    rect.max,
                ))
                .layout(Layout::left_to_right(Align::Center)),
        );
        if header_button(&mut actions_ui, ph::ARROWS_CLOCKWISE, false)
            .on_hover_text("Refresh")
            .clicked()
        {
            backend::refresh_status(self.shared.clone(), ctx.clone());
        }
        actions_ui.add_space(HEADER_BUTTON_GAP);
        if header_button(
            &mut actions_ui,
            ph::GEAR_SIX,
            self.screen == Screen::Settings,
        )
        .on_hover_text("Settings")
        .clicked()
        {
            if self.screen == Screen::Settings {
                self.nav_back(ctx, false);
            } else {
                self.navigate(Screen::Settings, ctx);
            }
        }
        actions_ui.add_space(HEADER_BUTTON_GAP);
        if header_button(&mut actions_ui, ph::QR_CODE, self.screen == Screen::Pair)
            .on_hover_text("Pair new device")
            .clicked()
        {
            if self.screen == Screen::Pair {
                self.nav_back(ctx, false);
            } else {
                self.navigate(Screen::Pair, ctx);
            }
        }
    }

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
        // Hover shapes and scroll edges may bleed into the side margins, and
        // sliding pages run edge to edge, so the clip spans the window width
        // (inside its hairline border) rather than the content column.
        let window = ui.max_rect();
        screen_ui.set_clip_rect(Rect::from_min_max(
            egui::pos2(window.left() + 1.0, content.top() - SCREEN_TOP_BLEED),
            egui::pos2(window.right() - 1.0, content.bottom()),
        ));
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
        self.tab_bar(ui);

        match self.tab {
            Tab::Devices => self.devices_tab(ui, ctx),
            Tab::Logs => self.logs_tab(ui),
        }
    }

    fn tab_bar(&mut self, ui: &mut Ui) {
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
        let (snapshot, mirrors, starting_avds, deleting_avds) = {
            let state = self.shared.lock().unwrap();
            let mut mirrors = state.mirrors.keys().cloned().collect::<Vec<_>>();
            mirrors.extend(state.starting_mirrors.iter().cloned());
            (
                state.snapshot.clone(),
                mirrors,
                state.starting_avds.clone(),
                state.deleting_avds.clone(),
            )
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
        let day_weight = self.animations.skin.day_weight;
        let shell = Arc::clone(&self.shell);
        let settings = &self.settings;
        let shared = &self.shared;
        let pending_avd_delete = &mut self.pending_avd_delete;
        chrome_scroll(ui, "devices-scroll", &shell, day_weight, false, |ui| {
            let devices = snapshot.devices.iter().filter(|device| !device.is_emulator);
            let mut first = true;
            for device in devices {
                if !std::mem::take(&mut first) {
                    divider(ui);
                }

                let mirroring = mirrors.contains(&device.mirror_key);
                let action = if mirroring {
                    RowAction::enabled(ph::STOP, theme::green()).with_tooltip("Stop mirroring")
                } else if device.ready && scrcpy_ok {
                    RowAction::enabled(ph::PLAY, theme::text_bright()).with_tooltip("Mirror screen")
                } else {
                    RowAction::disabled(ph::PLAY)
                };
                let status = if mirroring {
                    "Mirroring"
                } else if device.ready {
                    "Ready"
                } else {
                    device.state.as_str()
                };

                let row = Row {
                    icon: ph::DEVICE_MOBILE,
                    name: &device.name,
                    tooltip: &device.serial,
                    status,
                    status_color: if mirroring {
                        theme::green()
                    } else {
                        theme::text_faint()
                    },
                };
                if list_row_with(ui, &row, &[action]).is_some() {
                    if mirroring {
                        backend::stop_mirror(shared, &device.mirror_key);
                    } else {
                        backend::start_mirror(
                            shared.clone(),
                            ctx.clone(),
                            device.clone(),
                            settings.scrcpy_options(),
                        );
                    }
                }
            }

            for avd in &snapshot.avds {
                if !std::mem::take(&mut first) {
                    divider(ui);
                }

                let starting = starting_avds.contains(&avd.name);
                let deleting = deleting_avds.contains(&avd.name);
                let idle = avd.can_launch(starting) && !deleting;
                // Play sits on the right edge; actions are laid out from the
                // right, so it is also the first to take Tab focus.
                let actions = [
                    if idle {
                        RowAction::enabled(ph::TRASH, theme::text_muted())
                    } else {
                        RowAction::disabled(ph::TRASH)
                    }
                    .with_tooltip("Delete emulator"),
                    if idle {
                        RowAction::enabled(ph::PLAY, theme::text_bright())
                    } else {
                        RowAction::disabled(ph::PLAY)
                    }
                    .with_tooltip("Start emulator"),
                ];
                let name = avd.name.replace('_', " ");
                let status = if deleting {
                    "Deleting…"
                } else {
                    avd.status(starting)
                };
                let row = Row {
                    icon: ph::ANDROID_LOGO,
                    name: &name,
                    tooltip: &name,
                    status,
                    status_color: if status == "Running" {
                        theme::green()
                    } else {
                        theme::text_faint()
                    },
                };

                match list_row_with(ui, &row, &actions) {
                    Some(0) => *pending_avd_delete = Some(avd.name.clone()),
                    Some(1) => backend::start_avd(shared.clone(), ctx.clone(), avd.name.clone()),
                    _ => {}
                }
            }
        });

        if let Some(name) = self.pending_avd_delete.clone() {
            let mut close = false;
            let modal = egui::Modal::new(egui::Id::new("delete-avd")).show(ctx, |ui| {
                ui.set_width(280.0);
                ui.label(semibold("Delete emulator?", 14.0, theme::text_bright()));
                ui.label(regular(&name, 12.0, theme::text_bright()));
                ui.label(regular(
                    "This permanently deletes the AVD and its saved data.",
                    11.0,
                    theme::text_muted(),
                ));
                ui.add_space(8.0);
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, ph::TRASH, 16.0, theme::red())
                        .on_hover_text("Delete emulator and its saved data")
                        .clicked()
                    {
                        backend::delete_avd(self.shared.clone(), ctx.clone(), name.clone());
                        close = true;
                    }
                    if icon_button(ui, ph::X, 16.0, theme::text_bright())
                        .on_hover_text("Cancel")
                        .clicked()
                    {
                        close = true;
                    }
                });
            });
            if close || modal.should_close() {
                self.pending_avd_delete = None;
            }
        }
    }

    fn logs_tab(&mut self, ui: &mut Ui) {
        let logs: Vec<String> = self.shared.lock().unwrap().logs.clone();
        let empty = logs.is_empty();
        let mut log_text = if empty {
            "No output yet.".to_string()
        } else {
            logs.join("\n")
        };

        let shell = Arc::clone(&self.shell);
        chrome_scroll(
            ui,
            "logs-scroll",
            &shell,
            self.animations.skin.day_weight,
            true,
            |ui| {
                // Keep breathing room in the resting state while letting
                // scrolled output disappear directly beneath the tab divider.
                ui.add_space(8.0);
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
            },
        );
    }

    fn settings_screen(&mut self, ui: &mut Ui, ctx: &Context) {
        let day_weight = self.animations.skin.day_weight;
        let shell = Arc::clone(&self.shell);
        chrome_scroll(ui, "settings-scroll", &shell, day_weight, false, |ui| {
            ui.add(
                Label::new(semibold("Screen Mirroring", 12.5, theme::text_bright()))
                    .selectable(false),
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
                changed |= check_item(ui, "Always on top", &mut self.settings.always_on_top);
                ui.add_space(20.0);
                changed |= check_item(ui, "Borderless", &mut self.settings.borderless);
                ui.add_space(20.0);
                changed |= check_item(ui, "Auto mirror", &mut self.settings.auto_mirror);
            });

            ui.add_space(12.0);
            divider(ui);
            ui.add_space(12.0);

            ui.add(Label::new(semibold("General", 12.5, theme::text_bright())).selectable(false));
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
            // It runs off the UI thread so opening Settings never stutters.
            if self.open_at_login.is_none() && self.login_query.is_none() {
                let (sender, receiver) = std::sync::mpsc::channel();
                let repaint = ctx.clone();
                std::thread::spawn(move || {
                    let _ = sender.send(login_item::is_enabled());
                    repaint.request_repaint();
                });
                self.login_query = Some(receiver);
            }
            if let Some(enabled) = self
                .login_query
                .as_ref()
                .and_then(|receiver| receiver.try_recv().ok())
            {
                self.open_at_login = Some(enabled);
                self.login_query = None;
            }
            let mut open_at_login = self.open_at_login.unwrap_or(false);
            let mut gradients_changed = false;
            ui.horizontal(|ui| {
                if check_item(ui, "Open at Login", &mut open_at_login) {
                    login_item::set_enabled(open_at_login);
                    self.open_at_login = Some(open_at_login);
                    self.login_query = None;
                }
                ui.add_space(20.0);
                gradients_changed = check_item(ui, "Gradients", &mut self.settings.gradients);
            });
            if gradients_changed {
                changed = true;
                (self.day_shell, self.night_shell, self.shell) =
                    load_shells(ctx, self.settings.gradients);
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
                Label::new(semibold("Dependencies", 12.5, theme::text_bright())).selectable(false),
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
    }

    fn pair_screen(&mut self, ui: &mut Ui, ctx: &Context) {
        let phase = {
            let state = self.shared.lock().unwrap();
            state.pairing.as_ref().map(|session| session.phase.clone())
        };

        let Some(phase) = phase else {
            return;
        };

        match phase {
            PairingPhase::Qr { modules, progress } => {
                let steps = [
                    "Developer options",
                    "Wireless debugging",
                    "Pair device with QR code",
                ];
                let steps_height = steps.len() as f32 * PAIRING_STEP_HEIGHT;
                center_pad(
                    ui,
                    QR_CARD_SIZE + 16.0 + 18.0 + 10.0 + steps_height + 10.0 + 16.0,
                );
                ui.vertical_centered(|ui| {
                    let (rect, _) =
                        ui.allocate_exact_size(vec2(QR_CARD_SIZE, QR_CARD_SIZE), Sense::hover());
                    paint_qr_card(ui, rect, &modules);
                    ui.add_space(16.0);
                    ui.add(
                        Label::new(semibold("Scan with your phone", 13.0, theme::text_bright()))
                            .selectable(false),
                    );
                    ui.add_space(10.0);
                    // The steps stay left-aligned as one block, centered under the code.
                    let block_width = pairing_steps_width(ui, &steps);
                    ui.allocate_ui_with_layout(
                        vec2(block_width, steps_height),
                        Layout::top_down(Align::Min),
                        |ui| {
                            for (index, step) in steps.iter().enumerate() {
                                pairing_step(ui, index + 1, step);
                            }
                        },
                    );
                    ui.add_space(10.0);
                    pairing_progress_block(ui, &progress, 310.0, Align::Center);
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
                    pairing_progress_block(ui, &progress, 310.0, Align::Center);
                });
            }
            PairingPhase::Paired { device_name, .. } => {
                center_pad(ui, 28.0 + 12.0 + 18.0 + 4.0 + 18.0);
                ui.vertical_centered(|ui| {
                    ui.add(Label::new(icon(ph::CHECK_CIRCLE, 28.0, theme::green())));
                    ui.add_space(12.0);
                    ui.add(Label::new(semibold("Paired", 13.0, theme::text_bright())));
                    ui.add_space(4.0);
                    hint_label(ui, &format!("{device_name} is connected."), 300.0);
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
                            self.navigate(Screen::Main, ctx);
                        }
                    });
                });
            }
        }
    }
}

struct RowAction {
    glyph: &'static str,
    tooltip: Option<&'static str>,
    color: Color32,
    enabled: bool,
}

impl RowAction {
    fn enabled(glyph: &'static str, color: Color32) -> Self {
        Self {
            glyph,
            tooltip: None,
            color,
            enabled: true,
        }
    }

    fn disabled(glyph: &'static str) -> Self {
        Self {
            glyph,
            tooltip: None,
            color: theme::text_faint(),
            enabled: false,
        }
    }

    fn with_tooltip(mut self, tooltip: &'static str) -> Self {
        self.tooltip = Some(tooltip);
        self
    }
}

fn icon_button(ui: &mut Ui, glyph: &str, size: f32, color: Color32) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(size + 8.0, size + 8.0), Sense::click());
    let interaction = interaction_visual(
        ui,
        response.id,
        response.hovered(),
        response.is_pointer_button_down_on(),
    );
    ui.painter().rect_filled(rect, 6.0, interaction.overlay);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        theme::icon_font(size),
        color,
    );
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
            let background = ui.painter().add(egui::Shape::Noop);
            // The label only shows text; a separate click area takes focus so
            // egui does not underline the focused label.
            let label = ui.add(Label::new(text).selectable(false));
            let response = ui.interact(label.rect, label.id.with("tab"), Sense::click());
            let interaction = interaction_visual(
                ui,
                response.id,
                response.hovered(),
                response.is_pointer_button_down_on(),
            );
            ui.painter().set(
                background,
                egui::Shape::rect_filled(
                    response.rect.expand2(vec2(6.0, 4.0)),
                    5.0,
                    interaction.overlay,
                ),
            );
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

fn dependency_row(ui: &mut Ui, row_icon: &str, name: &str, info: Option<&backend::ToolInfo>) {
    ui.horizontal(|ui| {
        ui.set_height(ROW_HEIGHT);
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

        ui.with_layout(Layout::right_to_left(Align::Center), |ui| match info {
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
        let hovered = output.response.hovered() || output.inner.hovered();
        let hover_progress = ui.ctx().animate_bool_with_time(
            output.response.id.with("input-hover"),
            hovered,
            CONTROL_HOVER_TRANSITION,
        );
        let (stroke_width, stroke_color) = if output.inner.has_focus() {
            (1.5_f32, theme::input_focus_stroke())
        } else {
            (
                1.0_f32,
                theme::input_stroke().lerp_to_gamma(theme::input_hover_stroke(), hover_progress),
            )
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

#[derive(Clone, Copy, Default)]
struct ScrollChrome {
    offset: f32,
    active_at: f64,
}

/// A vertical scroll area with the popover's own chrome: content edges fade
/// into the shell surface, and a thin overlay scrollbar appears while
/// scrolling, widens under the pointer and can be dragged. The viewport
/// bleeds into the side margins so content stays aligned with the header.
fn chrome_scroll(
    ui: &mut Ui,
    salt: &str,
    shell: &ShellSampler,
    day_weight: f32,
    stick_to_bottom: bool,
    add_contents: impl FnOnce(&mut Ui),
) {
    let column = ui.available_rect_before_wrap();
    let viewport_rect = column.expand2(vec2(SCROLL_BLEED, 0.0));
    let mut area_ui = ui.new_child(
        egui::UiBuilder::new()
            .id_salt(salt)
            .max_rect(viewport_rect)
            .layout(Layout::top_down(Align::Min)),
    );
    let output = egui::ScrollArea::vertical()
        .id_salt(salt)
        .auto_shrink([false, false])
        .stick_to_bottom(stick_to_bottom)
        .scroll_bar_visibility(ScrollBarVisibility::AlwaysHidden)
        .show(&mut area_ui, |ui| {
            Frame::NONE
                .inner_margin(Margin::symmetric(SCROLL_BLEED as i8, 0))
                .show(ui, |ui| {
                    ui.set_width(column.width());
                    add_contents(ui);
                });
        });
    ui.allocate_rect(column, Sense::hover());

    let ctx = ui.ctx().clone();
    let chrome_id = output.id.with("chrome");
    let mut chrome: ScrollChrome = ctx
        .data(|data| data.get_temp(chrome_id))
        .unwrap_or_default();
    let now = ctx.input(|input| input.time);
    let viewport = output.inner_rect;
    let max_offset = (output.content_size.y - viewport.height()).max(0.0);
    let mut offset = output.state.offset.y;
    if (offset - chrome.offset).abs() > 0.01 {
        chrome.active_at = now;
    }

    if max_offset > 0.5 {
        let track = Rect::from_min_max(
            egui::pos2(viewport.right() - SCROLLBAR_HIT_WIDTH, viewport.top()),
            viewport.max,
        )
        .shrink2(vec2(0.0, SCROLLBAR_TRACK_INSET));
        let (handle_height, travel) =
            scrollbar_handle(track.height(), viewport.height(), output.content_size.y);

        let bar = ui.interact(track, chrome_id.with("bar"), Sense::DRAG);
        if bar.dragged() {
            offset = (offset + bar.drag_delta().y * max_offset / travel).clamp(0.0, max_offset);
            let mut state = output.state;
            state.offset.y = offset;
            state.store(&ctx, output.id);
            ctx.request_repaint();
        }
        let engaged = bar.hovered() || bar.dragged();
        if engaged {
            chrome.active_at = now;
        }
        let since = now - chrome.active_at;
        let visibility = if since <= SCROLLBAR_LINGER {
            1.0
        } else {
            (1.0 - (since - SCROLLBAR_LINGER) / SCROLLBAR_FADE).clamp(0.0, 1.0) as f32
        };
        if visibility > 0.0 {
            ctx.request_repaint_after(Duration::from_millis(16));
        }
        let widen = ctx.animate_bool_with_time(chrome_id.with("widen"), engaged, 0.12);

        let painter = ui.painter().with_clip_rect(viewport);
        let top_strength = (offset / SCROLL_EDGE_FADE_HEIGHT).clamp(0.0, 1.0);
        let bottom_strength = ((max_offset - offset) / SCROLL_EDGE_FADE_HEIGHT).clamp(0.0, 1.0);
        let fade = |top: f32, strength: f32, solid_at_top: bool| {
            let rect = Rect::from_min_max(
                egui::pos2(viewport.left(), top),
                egui::pos2(viewport.right(), top + SCROLL_EDGE_FADE_HEIGHT),
            );
            paint_edge_fade(&painter, shell, day_weight, rect, solid_at_top, strength);
        };
        if top_strength > 0.0 {
            fade(viewport.top(), top_strength, true);
        }
        if bottom_strength > 0.0 {
            fade(
                viewport.bottom() - SCROLL_EDGE_FADE_HEIGHT,
                bottom_strength,
                false,
            );
        }

        if visibility > 0.0 {
            let width = egui::lerp(SCROLLBAR_WIDTH..=SCROLLBAR_HOVER_WIDTH, widen);
            let top = track.top() + travel * (offset / max_offset).clamp(0.0, 1.0);
            let handle = Rect::from_min_size(
                egui::pos2(viewport.right() - 2.0 - width, top),
                vec2(width, handle_height),
            );
            let color =
                theme::scrollbar_thumb().lerp_to_gamma(theme::scrollbar_thumb_hover(), widen);
            painter.rect_filled(handle, width / 2.0, with_opacity(color, visibility));
        }
    }

    chrome.offset = offset;
    ctx.data_mut(|data| data.insert_temp(chrome_id, chrome));
}

/// Handle length, proportional to the visible share of the content but never
/// shorter than a comfortable grab target, and the distance it can travel.
fn scrollbar_handle(track_height: f32, viewport_height: f32, content_height: f32) -> (f32, f32) {
    let handle = (track_height * viewport_height / content_height)
        .clamp(SCROLLBAR_MIN_HANDLE.min(track_height), track_height);
    (handle, (track_height - handle).max(1.0))
}

/// Fades content into the shell along one edge of `rect`, sampling the shell
/// across the width so the band matches the lit surface beneath it.
fn paint_edge_fade(
    painter: &egui::Painter,
    shell: &ShellSampler,
    day_weight: f32,
    rect: Rect,
    solid_at_top: bool,
    strength: f32,
) {
    let columns = ((rect.width() / 12.0).ceil() as u32).max(1);
    let edge_y = if solid_at_top {
        rect.top()
    } else {
        rect.bottom()
    };
    let mut mesh = egui::Mesh::default();
    for column in 0..=columns {
        let x = rect.left() + rect.width() * column as f32 / columns as f32;
        let solid = shell
            .color_at(egui::pos2(x, edge_y), day_weight)
            .gamma_multiply(strength);
        let (top, bottom) = if solid_at_top {
            (solid, Color32::TRANSPARENT)
        } else {
            (Color32::TRANSPARENT, solid)
        };
        mesh.colored_vertex(egui::pos2(x, rect.top()), top);
        mesh.colored_vertex(egui::pos2(x, rect.bottom()), bottom);
    }
    for column in 0..columns {
        let index = column * 2;
        mesh.add_triangle(index, index + 1, index + 2);
        mesh.add_triangle(index + 1, index + 3, index + 2);
    }
    painter.add(egui::Shape::mesh(mesh));
}

/// Whether focus last moved by keyboard, so focus highlights show for Tab
/// navigation but not after a click.
fn keyboard_focus_id() -> egui::Id {
    egui::Id::new("awb-keyboard-focus")
}

fn keyboard_focused(ui: &Ui, id: egui::Id) -> bool {
    ui.memory(|memory| memory.has_focus(id))
        && ui
            .ctx()
            .data(|data| data.get_temp::<bool>(keyboard_focus_id()))
            .unwrap_or(false)
}

fn screen_title(screen: Screen) -> &'static str {
    match screen {
        Screen::Main => "Android Wifi Bridge",
        Screen::Settings => "Settings",
        Screen::Pair => "Pair device",
    }
}

/// A header action; `active` marks the page it opened.
fn header_button(ui: &mut Ui, glyph: &str, active: bool) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(vec2(HEADER_BUTTON_SIZE, HEADER_BUTTON_SIZE), Sense::click());
    let active_progress = ui
        .ctx()
        .animate_bool_with_time(response.id.with("active"), active, 0.16);
    if active_progress > 0.0 {
        ui.painter().rect_filled(
            rect,
            7.0,
            theme::control_selected().gamma_multiply(active_progress),
        );
    }
    let interaction = interaction_visual(
        ui,
        response.id,
        response.hovered(),
        response.is_pointer_button_down_on(),
    );
    ui.painter().rect_filled(rect, 7.0, interaction.overlay);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        glyph,
        theme::icon_font(15.0),
        theme::text_muted().lerp_to_gamma(
            theme::text_strong(),
            active_progress.max(interaction.hover_progress),
        ),
    );
    response.on_hover_cursor(egui::CursorIcon::PointingHand)
}

struct Row<'a> {
    icon: &'a str,
    name: &'a str,
    tooltip: &'a str,
    status: &'a str,
    status_color: Color32,
}

/// One device row: icon, name, a status column at a fixed position so the
/// statuses line up, then the action buttons on the right edge.
fn list_row_with(ui: &mut Ui, row: &Row<'_>, actions: &[RowAction]) -> Option<usize> {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_HEIGHT), Sense::hover());
    let hover = ui.ctx().animate_bool_with_time(
        ui.id().with(("row-hover", row.name)),
        ui.is_enabled() && ui.rect_contains_pointer(rect),
        CONTROL_HOVER_TRANSITION,
    );
    if hover > 0.0 {
        ui.painter().rect_filled(
            rect.expand2(vec2(ROW_HOVER_BLEED, -2.0)),
            8.0,
            theme::control_hover().gamma_multiply(hover),
        );
    }
    let center_y = rect.center().y;
    ui.painter().text(
        egui::pos2(rect.left() + ROW_ICON_SIZE / 2.0, center_y),
        egui::Align2::CENTER_CENTER,
        row.icon,
        theme::icon_font(ROW_ICON_SIZE),
        theme::text_label(),
    );

    let actions_width = actions.len() as f32 * ROW_ACTION_SIZE
        + actions.len().saturating_sub(1) as f32 * ROW_ACTION_GAP;
    let status_left = rect.right() - actions_width - 12.0 - STATUS_COLUMN_WIDTH;
    let name_rect = Rect::from_min_max(
        egui::pos2(rect.left() + 26.0, rect.top()),
        egui::pos2(status_left - 10.0, rect.bottom()),
    );
    let status_rect = Rect::from_min_max(
        egui::pos2(status_left, rect.top()),
        egui::pos2(status_left + STATUS_COLUMN_WIDTH, rect.bottom()),
    );
    let left_aligned = |rect| {
        egui::UiBuilder::new()
            .max_rect(rect)
            .layout(Layout::left_to_right(Align::Center))
    };
    ui.scope_builder(left_aligned(name_rect), |ui| {
        ui.add(
            Label::new(medium(row.name, 12.5, theme::text_bright()))
                .truncate()
                .selectable(false),
        )
        .on_hover_text(row.tooltip);
    });
    ui.scope_builder(left_aligned(status_rect), |ui| {
        ui.add(
            Label::new(regular(row.status, 11.0, row.status_color))
                .truncate()
                .selectable(false),
        );
    });

    let mut clicked = None;
    let mut right = rect.right();
    for (index, action) in actions.iter().enumerate().rev() {
        let action_rect = Rect::from_min_max(
            egui::pos2(right - ROW_ACTION_SIZE, center_y - ROW_ACTION_SIZE / 2.0),
            egui::pos2(right, center_y + ROW_ACTION_SIZE / 2.0),
        );
        right -= ROW_ACTION_SIZE + ROW_ACTION_GAP;
        let response = ui.interact(
            action_rect,
            ui.id().with((row.name, index)),
            if action.enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        ui.painter().rect_filled(action_rect, 6.0, theme::surface());
        let interaction = interaction_visual(
            ui,
            response.id,
            response.hovered() && action.enabled,
            response.is_pointer_button_down_on() && action.enabled,
        );
        ui.painter()
            .rect_filled(action_rect, 6.0, interaction.overlay);
        ui.painter().text(
            action_rect.center(),
            egui::Align2::CENTER_CENTER,
            action.glyph,
            theme::icon_font(10.0),
            action.color,
        );

        let response = if let Some(tooltip) = action.tooltip {
            response.on_hover_text(tooltip)
        } else {
            response
        };
        if action.enabled {
            let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
            if response.clicked() {
                clicked = Some(index);
            }
        }
    }
    clicked
}

#[cfg(test)]
fn list_row(
    ui: &mut Ui,
    row_icon: &str,
    name: &str,
    detail: &str,
    actions: &[RowAction],
) -> Option<usize> {
    let row = Row {
        icon: row_icon,
        name,
        tooltip: name,
        status: detail,
        status_color: theme::text_faint(),
    };
    list_row_with(ui, &row, actions)
}

fn paint_qr_card(ui: &Ui, rect: Rect, modules: &awb_core::qr::QrModules) {
    let painter = ui.painter();
    painter.rect_filled(rect, 14.0, theme::qr_card());
    painter.rect_stroke(
        rect,
        14.0,
        Stroke::new(1.0_f32, theme::qr_card_stroke()),
        egui::StrokeKind::Inside,
    );

    // Reserve a 4-module quiet zone; the raw matrix has none and Android's
    // scanner rejects codes without it.
    let qr_size = rect.width() - 20.0;
    let quiet = 4.0;
    let cell = qr_size / (modules.size as f32 + 2.0 * quiet);
    let origin =
        rect.center() - vec2(qr_size / 2.0, qr_size / 2.0) + vec2(quiet * cell, quiet * cell);
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
}

const PAIRING_STEP_HEIGHT: f32 = 22.0;
const PAIRING_BADGE_SIZE: f32 = 17.0;
const PAIRING_BADGE_GAP: f32 = 8.0;

fn pairing_step_font() -> FontId {
    FontId::new(11.5, FontFamily::Proportional)
}

/// Width of the widest numbered step, so the steps center as one block.
fn pairing_steps_width(ui: &Ui, steps: &[&str]) -> f32 {
    let text = steps
        .iter()
        .map(|step| {
            ui.painter()
                .layout_no_wrap((*step).to_owned(), pairing_step_font(), theme::text_check())
                .size()
                .x
        })
        .fold(0.0, f32::max);
    PAIRING_BADGE_SIZE + PAIRING_BADGE_GAP + text.ceil()
}

/// A numbered step of the phone-side pairing path.
fn pairing_step(ui: &mut Ui, number: usize, text: &str) {
    ui.horizontal(|ui| {
        ui.set_height(PAIRING_STEP_HEIGHT);
        let (badge, _) =
            ui.allocate_exact_size(vec2(PAIRING_BADGE_SIZE, PAIRING_BADGE_SIZE), Sense::hover());
        ui.painter()
            .circle_filled(badge.center(), 8.5, theme::control_selected());
        ui.painter().text(
            badge.center(),
            egui::Align2::CENTER_CENTER,
            number.to_string(),
            FontId::new(10.0, FontFamily::Name(theme::SEMIBOLD.into())),
            theme::text_bright(),
        );
        ui.add_space(PAIRING_BADGE_GAP);
        ui.add(
            Label::new(
                egui::RichText::new(text)
                    .font(pairing_step_font())
                    .color(theme::text_check()),
            )
            .selectable(false),
        );
    });
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
                        let (rect, response) =
                            ui.allocate_exact_size(vec2(56.0, 24.0), Sense::click());
                        if active {
                            ui.painter()
                                .rect_filled(rect, 6.0, theme::segment_selected());
                        }
                        let interaction = interaction_visual(
                            ui,
                            response.id,
                            response.hovered(),
                            response.is_pointer_button_down_on(),
                        );
                        ui.painter().rect_filled(rect, 6.0, interaction.overlay);
                        if active {
                            ui.painter().rect_stroke(
                                rect,
                                6.0,
                                Stroke::new(1.0_f32, theme::segment_selected_stroke()),
                                egui::StrokeKind::Inside,
                            );
                        }
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            label,
                            FontId::new(
                                11.5,
                                FontFamily::Name(
                                    if active {
                                        theme::SEMIBOLD
                                    } else {
                                        theme::MEDIUM
                                    }
                                    .into(),
                                ),
                            ),
                            if active {
                                theme::text_strong()
                            } else {
                                theme::text_muted()
                                    .lerp_to_gamma(theme::text_bright(), interaction.hover_progress)
                            },
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
        let background = ui.painter().add(egui::Shape::Noop);
        let (rect, response) = ui.allocate_exact_size(vec2(15.0, 15.0), Sense::click());
        let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);

        ui.add_space(7.0);
        let label_response = ui
            .add(
                Label::new(regular(label, 12.0, theme::text_check()))
                    .selectable(false)
                    .sense(Sense::CLICK),
            )
            .on_hover_cursor(egui::CursorIcon::PointingHand);

        let hovered = response.hovered() || label_response.hovered();
        let pressed =
            response.is_pointer_button_down_on() || label_response.is_pointer_button_down_on();
        let interaction = interaction_visual(ui, response.id, hovered, pressed);
        ui.painter().set(
            background,
            egui::Shape::rect_filled(
                rect.union(label_response.rect).expand2(vec2(5.0, 3.0)),
                5.0,
                interaction.overlay,
            ),
        );

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
                    theme::check_stroke()
                        .lerp_to_gamma(theme::input_hover_stroke(), interaction.hover_progress),
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

fn center_pad(ui: &mut Ui, content_height: f32) {
    let pad = (ui.available_height() - content_height) / 2.0;
    ui.add_space(pad.max(0.0));
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

/// Countdown with a timer icon (and the attempt, when retrying), then the
/// endpoint being paired underneath.
fn pairing_progress_block(ui: &mut Ui, progress: &PairingProgress, width: f32, align: Align) {
    if progress.deadline.is_some() {
        ui.ctx().request_repaint_after(Duration::from_secs(1));
    }

    let mut meta = Vec::new();
    if let Some(deadline) = progress.deadline {
        meta.push(format!("{}s", display_remaining_seconds(deadline)));
    }
    if let Some(attempt) = progress.attempt {
        meta.push(format!("attempt {attempt}"));
    }
    if !meta.is_empty() {
        let text = meta.join(" · ");
        let font = FontId::new(11.5, FontFamily::Name(theme::MEDIUM.into()));
        let text_width = ui
            .painter()
            .layout_no_wrap(text.clone(), font.clone(), theme::green())
            .size()
            .x;
        let icon_width = 13.0 + 5.0;
        let size = vec2(icon_width + text_width, 16.0);
        let (rect, _) = ui.allocate_exact_size(size, Sense::hover());
        let rect = if align == Align::Center {
            Rect::from_center_size(rect.center(), size)
        } else {
            rect
        };
        let painter = ui.painter();
        painter.text(
            egui::pos2(rect.left(), rect.center().y),
            egui::Align2::LEFT_CENTER,
            if progress.deadline.is_some() {
                ph::TIMER
            } else {
                ph::ARROWS_CLOCKWISE
            },
            theme::icon_font(13.0),
            theme::green(),
        );
        painter.text(
            egui::pos2(rect.left() + icon_width, rect.center().y),
            egui::Align2::LEFT_CENTER,
            text,
            font,
            theme::green(),
        );
    }

    if let Some(endpoint) = &progress.endpoint {
        ui.add_space(4.0);
        ui.scope(|ui| {
            ui.set_max_width(width);
            ui.add(
                Label::new(regular(endpoint, 10.5, theme::text_faint()))
                    .halign(align)
                    .wrap(),
            );
        });
    }
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
    let interaction = interaction_visual(
        ui,
        response.id,
        response.hovered(),
        response.is_pointer_button_down_on(),
    );
    painter.rect_filled(rect, 7.0, interaction.overlay);

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

struct InteractionVisual {
    overlay: Color32,
    hover_progress: f32,
}

/// Hover and press feedback. A control focused from the keyboard takes the
/// hover highlight, so focus reads as a lit background rather than an outline.
fn interaction_visual(ui: &Ui, id: egui::Id, hovered: bool, pressed: bool) -> InteractionVisual {
    let hovered = hovered || keyboard_focused(ui, id);
    let hover_progress = ui.ctx().animate_bool_with_time(
        id.with("control-hover"),
        hovered,
        CONTROL_HOVER_TRANSITION,
    );
    let press_progress = ui.ctx().animate_bool_with_time(
        id.with("control-press"),
        pressed,
        CONTROL_PRESS_TRANSITION,
    );

    InteractionVisual {
        overlay: interaction_overlay_color(hover_progress, press_progress),
        hover_progress,
    }
}

fn interaction_overlay_color(hover_progress: f32, press_progress: f32) -> Color32 {
    theme::control_hover()
        .gamma_multiply(hover_progress)
        .blend(theme::control_pressed().gamma_multiply(press_progress))
}

#[cfg(test)]
mod tests {
    use super::*;

    const WIDTH: f64 = 380.0;
    const MARGIN: f64 = 8.0;

    #[test]
    fn long_avd_name_leaves_status_and_action_icons_visible() {
        let ctx = Context::default();
        theme::install_fonts(&ctx);
        let output = ctx.run_ui(
            egui::RawInput {
                screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(340.0, 100.0))),
                ..Default::default()
            },
            |ui| {
                egui::CentralPanel::default().show_inside(ui, |ui| {
                    list_row(
                        ui,
                        ph::DESKTOP,
                        "bitkit recording with a very long emulator name",
                        "Starting…",
                        &[
                            RowAction::disabled(ph::PLAY),
                            RowAction::disabled(ph::TRASH),
                        ],
                    );
                });
            },
        );
        let text: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text),
                _ => None,
            })
            .collect();
        let status = text
            .iter()
            .find(|text| text.galley.text() == "Starting…")
            .unwrap();
        let play = text
            .iter()
            .find(|text| text.galley.text() == ph::PLAY)
            .unwrap();
        let trash = text
            .iter()
            .find(|text| text.galley.text() == ph::TRASH)
            .unwrap();
        assert!(status.pos.x + status.galley.size().x < play.pos.x);
        assert!(play.pos.x + play.galley.size().x < trash.pos.x);
        assert!(trash.pos.x + trash.galley.size().x <= 332.0);
        assert!(text.iter().all(|text| text.galley.text() != "Launch"));
        assert!(text.iter().any(|text| text.galley.elided));
    }

    #[test]
    fn row_icons_dispatch_separate_actions_and_ignore_disabled_clicks() {
        let ctx = Context::default();
        theme::install_fonts(&ctx);
        let render = |events, enabled| {
            let mut clicked = None;
            let output = ctx.run_ui(
                egui::RawInput {
                    screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(340.0, 100.0))),
                    events,
                    ..Default::default()
                },
                |ui| {
                    egui::CentralPanel::default().show_inside(ui, |ui| {
                        let actions = [ph::PLAY, ph::TRASH].map(|glyph| {
                            if enabled {
                                RowAction::enabled(glyph, theme::text_bright())
                            } else {
                                RowAction::disabled(glyph)
                            }
                        });
                        clicked = list_row(ui, ph::DESKTOP, "Pixel 9a", "Stopped", &actions);
                    });
                },
            );
            (output, clicked)
        };
        let (output, _) = render(vec![], true);
        for (index, glyph) in [ph::PLAY, ph::TRASH].iter().enumerate() {
            let pos = output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.text() == *glyph => {
                        Some(text.pos + text.galley.size() / 2.0)
                    }
                    _ => None,
                })
                .unwrap();
            for enabled in [true, false] {
                render(vec![], enabled);
                render(
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::NONE,
                        },
                    ],
                    enabled,
                );
                let (_, clicked) = render(
                    vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::NONE,
                    }],
                    enabled,
                );
                assert_eq!(clicked, enabled.then_some(index));
            }
        }
    }

    #[test]
    fn interaction_overlay_fades_fully_to_transparent() {
        let hidden = interaction_overlay_color(0.0, 0.0);
        let hovered = interaction_overlay_color(1.0, 0.0);
        let pressed = interaction_overlay_color(1.0, 1.0);

        assert_eq!(hidden, Color32::TRANSPARENT);
        assert!(hovered.a() > hidden.a());
        assert!(pressed.a() > hovered.a());
    }

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
    fn scrollbar_handle_tracks_the_visible_share_with_a_minimum_size() {
        assert_eq!(scrollbar_handle(200.0, 200.0, 400.0), (100.0, 100.0));
        assert_eq!(
            scrollbar_handle(200.0, 200.0, 10_000.0).0,
            SCROLLBAR_MIN_HANDLE
        );
    }

    #[test]
    fn arrow_keys_walk_pages_in_order_without_wrapping() {
        assert_eq!(screen_in_order(Screen::Main, 1), Some(Screen::Settings));
        assert_eq!(screen_in_order(Screen::Settings, 1), Some(Screen::Pair));
        assert_eq!(screen_in_order(Screen::Pair, 1), None);
        assert_eq!(screen_in_order(Screen::Pair, -1), Some(Screen::Settings));
        assert_eq!(screen_in_order(Screen::Main, -1), None);
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
    fn hidden_launch_uses_lightweight_status_refresh_without_auto_mirror() {
        assert!(!needs_full_status_refresh(false, false));
        assert!(needs_full_status_refresh(true, false));
        assert!(needs_full_status_refresh(false, true));
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
