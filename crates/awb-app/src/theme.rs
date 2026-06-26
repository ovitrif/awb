//! Design tokens and font setup extracted from DESIGN.pen.

use eframe::egui::{
    Color32, Context, FontData, FontDefinitions, FontFamily, FontId, RichText, TextStyle,
};

pub const WINDOW_WIDTH: f32 = 380.0;
pub const WINDOW_HEIGHT: f32 = 340.0;
/// The beak rises above the rounded body and points at the menu bar icon.
pub const BEAK_HEIGHT: f32 = 9.0;
pub const WINDOW_FULL_HEIGHT: f32 = WINDOW_HEIGHT + BEAK_HEIGHT;

pub const HAIRLINE: Color32 = Color32::from_rgba_premultiplied(0x0D, 0x0D, 0x0D, 0x0D);

pub const TEXT_STRONG: Color32 = Color32::from_rgb(0xF2, 0xF3, 0xF7);
pub const TEXT_BRIGHT: Color32 = Color32::from_rgb(0xEC, 0xEE, 0xF4);
pub const TEXT_SOFT: Color32 = Color32::from_rgb(0x83, 0x87, 0x91);
pub const TEXT_MUTED: Color32 = Color32::from_rgb(0x90, 0x94, 0xA0);
pub const TEXT_FAINT: Color32 = Color32::from_rgb(0x70, 0x74, 0x7F);
pub const TEXT_LABEL: Color32 = Color32::from_rgb(0x7C, 0x81, 0x90);
pub const TEXT_CHECK: Color32 = Color32::from_rgb(0xC9, 0xCC, 0xD6);

pub const GREEN: Color32 = Color32::from_rgb(0x3D, 0xDC, 0x84);
pub const GREEN_INK: Color32 = Color32::from_rgb(0x0A, 0x2A, 0x1B);
pub const AMBER: Color32 = Color32::from_rgb(0xF2, 0xC9, 0x4C);
pub const RED: Color32 = Color32::from_rgb(0xF8, 0x71, 0x71);

pub const SURFACE: Color32 = Color32::from_rgba_premultiplied(0x0D, 0x0D, 0x0D, 0x0D);
pub const INPUT_BG: Color32 = Color32::from_rgba_premultiplied(0x00, 0x00, 0x00, 0x33);
pub const INPUT_STROKE: Color32 = Color32::from_rgba_premultiplied(0x12, 0x12, 0x12, 0x12);
pub const CHECK_STROKE: Color32 = Color32::from_rgba_premultiplied(0x26, 0x26, 0x26, 0x26);

pub const QR_CARD: Color32 = Color32::WHITE;
pub const QR_INK: Color32 = Color32::from_rgb(0x17, 0x18, 0x1C);

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
