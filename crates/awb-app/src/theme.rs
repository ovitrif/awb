//! Design tokens and font setup extracted from DESIGN.pen.

use std::sync::atomic::{AtomicU8, Ordering};

use eframe::egui::{
    Color32, Context, FontData, FontDefinitions, FontFamily, FontId, RichText, TextStyle, Theme,
};

pub const WINDOW_WIDTH: f32 = 380.0;
pub const WINDOW_HEIGHT: f32 = 340.0;
/// The beak rises above the rounded body and points at the menu bar icon.
pub const BEAK_HEIGHT: f32 = 9.0;
pub const WINDOW_FULL_HEIGHT: f32 = WINDOW_HEIGHT + BEAK_HEIGHT;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Appearance {
    Day,
    Night,
}

#[derive(Debug, Clone, Copy)]
struct Palette {
    hairline: Color32,
    text_strong: Color32,
    text_bright: Color32,
    text_soft: Color32,
    text_muted: Color32,
    text_faint: Color32,
    text_label: Color32,
    text_check: Color32,
    green: Color32,
    green_ink: Color32,
    amber: Color32,
    red: Color32,
    surface: Color32,
    input_bg: Color32,
    input_stroke: Color32,
    input_hover_stroke: Color32,
    input_focus_stroke: Color32,
    check_stroke: Color32,
    control_shadow: Color32,
    segment_bg: Color32,
    segment_selected: Color32,
    segment_selected_stroke: Color32,
    scroll_fade_top: Color32,
    scroll_fade_bottom: Color32,
    scrollbar_thumb: Color32,
    qr_card: Color32,
    qr_ink: Color32,
}

const NIGHT: Palette = Palette {
    hairline: Color32::from_rgba_premultiplied(0x0D, 0x0D, 0x0D, 0x0D),
    text_strong: Color32::from_rgb(0xF2, 0xF3, 0xF7),
    text_bright: Color32::from_rgb(0xEC, 0xEE, 0xF4),
    text_soft: Color32::from_rgb(0x83, 0x87, 0x91),
    text_muted: Color32::from_rgb(0x90, 0x94, 0xA0),
    text_faint: Color32::from_rgb(0x70, 0x74, 0x7F),
    text_label: Color32::from_rgb(0x7C, 0x81, 0x90),
    text_check: Color32::from_rgb(0xC9, 0xCC, 0xD6),
    green: Color32::from_rgb(0x3D, 0xDC, 0x84),
    green_ink: Color32::from_rgb(0x0A, 0x2A, 0x1B),
    amber: Color32::from_rgb(0xF2, 0xC9, 0x4C),
    red: Color32::from_rgb(0xF8, 0x71, 0x71),
    surface: Color32::from_rgba_premultiplied(0x0D, 0x0D, 0x0D, 0x0D),
    input_bg: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x33),
    input_stroke: Color32::from_rgba_premultiplied(0x12, 0x12, 0x12, 0x12),
    input_hover_stroke: Color32::from_rgb(0x66, 0x6E, 0x82),
    input_focus_stroke: Color32::from_rgb(0x4D, 0x9F, 0xF5),
    check_stroke: Color32::from_rgba_premultiplied(0x26, 0x26, 0x26, 0x26),
    control_shadow: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x28),
    segment_bg: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x2E),
    segment_selected: Color32::from_rgba_premultiplied(0x20, 0x20, 0x20, 0x20),
    segment_selected_stroke: Color32::from_rgba_premultiplied(0x34, 0x34, 0x34, 0x34),
    scroll_fade_top: Color32::from_rgb(0x2C, 0x2F, 0x3E),
    scroll_fade_bottom: Color32::from_rgb(0x1E, 0x21, 0x2B),
    scrollbar_thumb: Color32::from_rgba_premultiplied(0x55, 0x57, 0x5C, 0x70),
    qr_card: Color32::WHITE,
    qr_ink: Color32::from_rgb(0x17, 0x18, 0x1C),
};

const DAY: Palette = Palette {
    hairline: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x0C),
    text_strong: Color32::from_rgb(0x17, 0x19, 0x23),
    text_bright: Color32::from_rgb(0x22, 0x24, 0x2E),
    text_soft: Color32::from_rgb(0x62, 0x68, 0x74),
    text_muted: Color32::from_rgb(0x54, 0x5B, 0x68),
    text_faint: Color32::from_rgb(0x75, 0x7D, 0x8A),
    text_label: Color32::from_rgb(0x65, 0x6D, 0x7A),
    text_check: Color32::from_rgb(0x2F, 0x33, 0x3D),
    green: Color32::from_rgb(0x14, 0x84, 0x47),
    green_ink: Color32::WHITE,
    amber: Color32::from_rgb(0x9A, 0x67, 0x00),
    red: Color32::from_rgb(0xD6, 0x3A, 0x3A),
    surface: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x08),
    input_bg: Color32::WHITE,
    input_stroke: Color32::from_rgb(0xB4, 0xC0, 0xCE),
    input_hover_stroke: Color32::from_rgb(0x88, 0x9A, 0xAF),
    input_focus_stroke: Color32::from_rgb(0x3E, 0x8F, 0xE8),
    check_stroke: Color32::from_rgb(0x89, 0x99, 0xAC),
    control_shadow: Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x16),
    segment_bg: Color32::from_rgb(0xE3, 0xE9, 0xF2),
    segment_selected: Color32::WHITE,
    segment_selected_stroke: Color32::from_rgb(0xC4, 0xCE, 0xDB),
    scroll_fade_top: Color32::WHITE,
    scroll_fade_bottom: Color32::from_rgb(0xDF, 0xE9, 0xF8),
    scrollbar_thumb: Color32::from_rgba_premultiplied(0x22, 0x27, 0x2D, 0x70),
    qr_card: Color32::WHITE,
    qr_ink: Color32::from_rgb(0x17, 0x18, 0x1C),
};

static ACTIVE_APPEARANCE: AtomicU8 = AtomicU8::new(Appearance::Night as u8);

pub fn apply(ctx: &Context, appearance: Appearance) {
    ACTIVE_APPEARANCE.store(appearance as u8, Ordering::Relaxed);
    ctx.set_theme(match appearance {
        Appearance::Day => Theme::Light,
        Appearance::Night => Theme::Dark,
    });
}

fn palette() -> &'static Palette {
    match ACTIVE_APPEARANCE.load(Ordering::Relaxed) {
        value if value == Appearance::Day as u8 => &DAY,
        _ => &NIGHT,
    }
}

macro_rules! color_token {
    ($name:ident, $field:ident) => {
        pub fn $name() -> Color32 {
            palette().$field
        }
    };
}

color_token!(hairline, hairline);
color_token!(text_strong, text_strong);
color_token!(text_bright, text_bright);
color_token!(text_soft, text_soft);
color_token!(text_muted, text_muted);
color_token!(text_faint, text_faint);
color_token!(text_label, text_label);
color_token!(text_check, text_check);
color_token!(green, green);
color_token!(green_ink, green_ink);
color_token!(amber, amber);
color_token!(red, red);
color_token!(surface, surface);
color_token!(input_bg, input_bg);
color_token!(input_stroke, input_stroke);
color_token!(input_hover_stroke, input_hover_stroke);
color_token!(input_focus_stroke, input_focus_stroke);
color_token!(check_stroke, check_stroke);
color_token!(control_shadow, control_shadow);
color_token!(segment_bg, segment_bg);
color_token!(segment_selected, segment_selected);
color_token!(segment_selected_stroke, segment_selected_stroke);
color_token!(scroll_fade_top, scroll_fade_top);
color_token!(scroll_fade_bottom, scroll_fade_bottom);
color_token!(scrollbar_thumb, scrollbar_thumb);
color_token!(qr_card, qr_card);
color_token!(qr_ink, qr_ink);

pub const MEDIUM: &str = "inter-medium";
pub const SEMIBOLD: &str = "inter-semibold";
pub const BOLD: &str = "inter-bold";
pub const ICONS: &str = "icons";

pub fn install_fonts(ctx: &Context) {
    let mut fonts = FontDefinitions::default();

    let faces: [(&str, &[u8]); 4] = [
        ("inter", include_bytes!("../assets/fonts/Inter-Regular.ttf")),
        (MEDIUM, include_bytes!("../assets/fonts/Inter-Medium.ttf")),
        (
            SEMIBOLD,
            include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
        ),
        (BOLD, include_bytes!("../assets/fonts/Inter-Bold.ttf")),
    ];

    for (name, bytes) in faces {
        fonts
            .font_data
            .insert(name.to_owned(), FontData::from_static(bytes).into());
    }

    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);

    // Icons get a dedicated family. Phosphor cannot sit in a text family's
    // fallback chain (it blanks lowercase Latin), and Inter cannot precede
    // it for icons (Inter defines private-use glyphs that shadow icons).
    let proportional = fonts.families.get_mut(&FontFamily::Proportional).unwrap();
    proportional.retain(|name| name != "phosphor");
    proportional.insert(0, "inter".to_owned());

    fonts
        .families
        .insert(FontFamily::Name(ICONS.into()), vec!["phosphor".to_owned()]);

    for name in [MEDIUM, SEMIBOLD, BOLD] {
        fonts.families.insert(
            FontFamily::Name(name.into()),
            vec![name.to_owned(), "inter".to_owned()],
        );
    }

    ctx.set_fonts(fonts);

    ctx.all_styles_mut(|style| {
        style
            .text_styles
            .insert(TextStyle::Body, FontId::new(12.5, FontFamily::Proportional));
        style.visuals.override_text_color = None;
        style.spacing.item_spacing = eframe::egui::vec2(0.0, 0.0);
        style.spacing.button_padding = eframe::egui::vec2(0.0, 0.0);
    });
}

pub fn regular(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text).size(size).color(color)
}

pub fn medium(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text)
        .size(size)
        .color(color)
        .family(FontFamily::Name(MEDIUM.into()))
}

pub fn semibold(text: impl Into<String>, size: f32, color: Color32) -> RichText {
    RichText::new(text)
        .size(size)
        .color(color)
        .family(FontFamily::Name(SEMIBOLD.into()))
}

pub fn icon(glyph: &str, size: f32, color: Color32) -> RichText {
    RichText::new(glyph)
        .size(size)
        .color(color)
        .family(FontFamily::Name(ICONS.into()))
}

pub fn icon_font(size: f32) -> FontId {
    FontId::new(size, FontFamily::Name(ICONS.into()))
}
