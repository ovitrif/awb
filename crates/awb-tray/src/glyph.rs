//! Rasterizes the awb glyph (bugdroid dome + Wi-Fi waves) from the SVG path
//! geometry in DESIGN.pen, for the tray icon and the in-window logo.

use kurbo::{BezPath, PathEl};
use tiny_skia::{Color, FillRule, Paint, PathBuilder, Pixmap, Transform};

/// Monochrome tray glyph: dome with punched-out eyes under two Wi-Fi arcs.
const TRAY_GLYPH: &str = "M25 75a25 25 0 0 1 50 0z m-4.94-23.4a38 38 0 0 1 59.88 0l-5.51 4.31a31 31 0 0 0-48.86 0z m-10.25-8a51 51 0 0 1 80.38 0l-5.52 4.31a44 44 0 0 0-69.34 0z m28.69 19.4a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0z m16 0a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0z";

/// Colored logo: green head with punched eyes, blue Wi-Fi waves.
const LOGO_HEAD: &str = "M25 75a25 25 0 0 1 50 0z m13.5-12a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0z m16 0a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0z";
const LOGO_WAVE_1: &str = "M20.06 51.6a38 38 0 0 1 59.88 0l-5.51 4.31a31 31 0 0 0-48.86 0z";
const LOGO_WAVE_2: &str = "M9.81 43.6a51 51 0 0 1 80.38 0l-5.52 4.31a44 44 0 0 0-69.34 0z";

const VIEWBOX: f32 = 100.0;

pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Tray icon glyph as a black template image; macOS recolors it for the
/// current menu bar appearance.
pub fn tray_icon(size: u32) -> Raster {
    let mut pixmap = Pixmap::new(size, size).expect("tray icon pixmap");
    let transform = Transform::from_scale(size as f32 / VIEWBOX, size as f32 / VIEWBOX);
    fill_path(&mut pixmap, TRAY_GLYPH, Color::BLACK, transform);

    Raster {
        width: size,
        height: size,
        rgba: pixmap.take(),
    }
}

/// Window logo as drawn in DESIGN.pen: a 100-unit viewBox scaled to
/// `glyph_size` and offset by `offset` inside a `frame_size` square.
pub fn window_logo(frame_size: u32, glyph_size: f32, offset: f32, oversample: u32) -> Raster {
    let px = frame_size * oversample;
    let mut pixmap = Pixmap::new(px, px).expect("logo pixmap");
    let scale = glyph_size * oversample as f32 / VIEWBOX;
    let shift = offset * oversample as f32;
    let transform = Transform::from_scale(scale, scale).post_translate(shift, shift);

    let green = Color::from_rgba8(0x3D, 0xDC, 0x84, 0xFF);
    let blue = Color::from_rgba8(0x4D, 0x9F, 0xF5, 0xFF);

    fill_path(&mut pixmap, LOGO_HEAD, green, transform);
    fill_path(&mut pixmap, LOGO_WAVE_1, blue, transform);
    fill_path(&mut pixmap, LOGO_WAVE_2, blue, transform);

    Raster {
        width: px,
        height: px,
        rgba: pixmap.take(),
    }
}

/// App icon for the macOS bundle: dark rounded tile with the colored glyph,
/// per the `componentBg` tile in DESIGN.pen.
pub fn app_icon_png(size: u32) -> Vec<u8> {
    let mut pixmap = Pixmap::new(size, size).expect("icon pixmap");
    let tile = size as f32;
    let radius = tile * 0.2237;

    let mut builder = PathBuilder::new();
    builder.push_circle(radius, radius, radius);
    builder.push_circle(tile - radius, radius, radius);
    builder.push_circle(radius, tile - radius, radius);
    builder.push_circle(tile - radius, tile - radius, radius);
    builder.push_rect(tiny_skia::Rect::from_ltrb(radius, 0.0, tile - radius, tile).unwrap());
    builder.push_rect(tiny_skia::Rect::from_ltrb(0.0, radius, tile, tile - radius).unwrap());
    let rounded_tile = builder.finish().expect("tile path");

    let mut paint = Paint::default();
    paint.set_color(Color::from_rgba8(0x1C, 0x1C, 0x1E, 0xFF));
    paint.anti_alias = true;
    pixmap.fill_path(
        &rounded_tile,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );

    // Center the 100-unit glyph viewBox at ~78% tile size; the glyph's
    // visual bounds sit between y=43 and y=75, so nudge it upward to
    // optically center.
    let glyph_scale = tile * 0.78 / VIEWBOX;
    let offset_x = tile * 0.11;
    let offset_y = tile * 0.5 - (59.0 * glyph_scale);
    let transform =
        Transform::from_scale(glyph_scale, glyph_scale).post_translate(offset_x, offset_y);

    let green = Color::from_rgba8(0x3D, 0xDC, 0x84, 0xFF);
    let blue = Color::from_rgba8(0x4D, 0x9F, 0xF5, 0xFF);
    fill_path(&mut pixmap, LOGO_HEAD, green, transform);
    fill_path(&mut pixmap, LOGO_WAVE_1, blue, transform);
    fill_path(&mut pixmap, LOGO_WAVE_2, blue, transform);

    pixmap.encode_png().expect("png encoding")
}

fn fill_path(pixmap: &mut Pixmap, svg_path: &str, color: Color, transform: Transform) {
    let bez = BezPath::from_svg(svg_path).expect("valid design path");
    let mut builder = PathBuilder::new();

    for element in bez.elements() {
        match *element {
            PathEl::MoveTo(p) => builder.move_to(p.x as f32, p.y as f32),
            PathEl::LineTo(p) => builder.line_to(p.x as f32, p.y as f32),
            PathEl::QuadTo(c, p) => builder.quad_to(c.x as f32, c.y as f32, p.x as f32, p.y as f32),
            PathEl::CurveTo(c1, c2, p) => builder.cubic_to(
                c1.x as f32,
                c1.y as f32,
                c2.x as f32,
                c2.y as f32,
                p.x as f32,
                p.y as f32,
            ),
            PathEl::ClosePath => builder.close(),
        }
    }

    let Some(path) = builder.finish() else {
        return;
    };

    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;

    pixmap.fill_path(&path, &paint, FillRule::EvenOdd, transform, None);
}
