//! Rasterizes the awb glyph (bugdroid dome + Wi-Fi waves) and the popover
//! shell (beak + gradient) from the SVG path geometry in DESIGN.pen.

use kurbo::{BezPath, PathEl};
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, PixmapPaint, Point,
    PremultipliedColorU8, RadialGradient, Shader, SpreadMode, Stroke, Transform,
};

/// Visual bounds of the glyph inside the 100-unit viewBox: [x, y, w, h]. The
/// dome bottom sits at y=75 and the outer Wi-Fi wave peaks at y=24.
const GLYPH_BOUNDS: [f32; 4] = [9.81, 24.0, 80.38, 51.0];

/// Monochrome menu bar glyph: dome with punched-out eyes under two Wi-Fi arcs.
const MENUBAR_GLYPH: &str = "M25 75a25 25 0 0 1 50 0z m-4.94-23.4a38 38 0 0 1 59.88 0l-5.51 4.31a31 31 0 0 0-48.86 0z m-10.25-8a51 51 0 0 1 80.38 0l-5.52 4.31a44 44 0 0 0-69.34 0z m28.69 19.4a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0z m16 0a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0z";

/// Colored logo: green head with punched eyes, blue Wi-Fi waves.
const LOGO_HEAD: &str = "M25 75a25 25 0 0 1 50 0z m13.5-12a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0z m16 0a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0z";
const LOGO_WAVE_1: &str = "M20.06 51.6a38 38 0 0 1 59.88 0l-5.51 4.31a31 31 0 0 0-48.86 0z";
const LOGO_WAVE_2: &str = "M9.81 43.6a51 51 0 0 1 80.38 0l-5.52 4.31a44 44 0 0 0-69.34 0z";

/// Popover shell: a 380x349 rounded body with a beak pointing up at the menu
/// bar icon. Coordinates match the `Shell Shape` path in DESIGN.pen.
const SHELL_PATH: &str = "M14 9l161 0c5 0 8.6-1.2 11-4.8 1.2-1.8 2.2-2.7 4-2.7 1.8 0 2.8 0.9 4 2.7 2.4 3.6 6 4.8 11 4.8l161 0a14 14 0 0 1 14 14l0 312a14 14 0 0 1-14 14l-352 0a14 14 0 0 1-14-14l0-312a14 14 0 0 1 14-14z";
const SHELL_W: f32 = 380.0;
const SHELL_H: f32 = 349.0;

const VIEWBOX: f32 = 100.0;

pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Menu bar icon: the glyph as a black template image, tightly fit to its
/// bounds and rendered taller than its slot so macOS downscales it into a crisp
/// white (dark mode) or black (light mode) status item. Width follows the
/// glyph's aspect ratio so the Wi-Fi waves are not squeezed.
pub fn menubar_icon(height: u32) -> Raster {
    let [gx, gy, gw, gh] = GLYPH_BOUNDS;
    let h = height as f32;
    let pad = h * 0.12;
    let scale = (h - 2.0 * pad) / gh;
    let width = (gw * scale + 2.0 * pad).ceil() as u32;

    // Fill only (no stroke), matching the design's light Wi-Fi arcs. Rendered
    // larger than its slot so macOS downscales it into a crisp template rather
    // than the upscaled gray a small bitmap would produce.
    let mut pixmap = Pixmap::new(width, height).expect("menu bar icon pixmap");
    let transform =
        Transform::from_scale(scale, scale).post_translate(pad - gx * scale, pad - gy * scale);
    fill_path(&mut pixmap, MENUBAR_GLYPH, Color::BLACK, transform);

    Raster {
        width,
        height,
        rgba: pixmap.take(),
    }
}

/// Side-by-side preview of how macOS recolors the menu bar template: white on a
/// dark bar, black on a light bar. For `awb-app --render-menubar`.
pub fn menubar_preview_png() -> Vec<u8> {
    let icon = menubar_icon(36);
    let zoom = 6;
    let inset = 28;
    let cell_w = icon.width * zoom + inset * 2;
    let cell_h = icon.height * zoom + inset * 2;

    let mut pixmap = Pixmap::new(cell_w * 2, cell_h).expect("preview pixmap");
    fill_cell(&mut pixmap, 0.0, cell_w as f32, cell_h as f32, "#1D1D1F");
    fill_cell(
        &mut pixmap,
        cell_w as f32,
        cell_w as f32,
        cell_h as f32,
        "#ECECEE",
    );

    let paint = PixmapPaint::default();
    let place = |dx: u32| {
        Transform::from_scale(zoom as f32, zoom as f32).post_translate(dx as f32, inset as f32)
    };
    pixmap.draw_pixmap(
        0,
        0,
        recolor(&icon, 0xFF).as_ref(),
        &paint,
        place(inset),
        None,
    );
    pixmap.draw_pixmap(
        0,
        0,
        recolor(&icon, 0x00).as_ref(),
        &paint,
        place(cell_w + inset),
        None,
    );

    pixmap.encode_png().expect("preview png")
}

fn recolor(icon: &Raster, level: u8) -> Pixmap {
    let mut pixmap = Pixmap::new(icon.width, icon.height).expect("recolor pixmap");
    for (pixel, chunk) in pixmap
        .pixels_mut()
        .iter_mut()
        .zip(icon.rgba.chunks_exact(4))
    {
        let alpha = chunk[3];
        let value = ((u16::from(level) * u16::from(alpha)) / 255) as u8;
        *pixel = PremultipliedColorU8::from_rgba(value, value, value, alpha)
            .expect("valid premultiplied gray");
    }
    pixmap
}

fn fill_cell(pixmap: &mut Pixmap, x: f32, w: f32, h: f32, hex: &str) {
    let bytes = u32::from_str_radix(hex.trim_start_matches('#'), 16).unwrap_or(0);
    let color = Color::from_rgba8((bytes >> 16) as u8, (bytes >> 8) as u8, bytes as u8, 0xFF);
    let mut paint = Paint::default();
    paint.set_color(color);
    if let Some(rect) = tiny_skia::Rect::from_xywh(x, 0.0, w, h) {
        pixmap.fill_rect(rect, &paint, Transform::identity(), None);
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

/// Rasterizes the popover shell (beak + rounded body) with the DESIGN.pen
/// background: a vertical slate gradient, a soft blue glow at the beak, and a
/// hairline stroke. Drawn at `oversample` resolution for a crisp texture.
fn render_shell(oversample: u32) -> Pixmap {
    let w = (SHELL_W as u32) * oversample;
    let h = (SHELL_H as u32) * oversample;
    let mut pixmap = Pixmap::new(w, h).expect("shell pixmap");
    let transform = Transform::from_scale(oversample as f32, oversample as f32);

    let Some(path) = skia_path(SHELL_PATH) else {
        return pixmap;
    };

    let base = LinearGradient::new(
        Point::from_xy(SHELL_W / 2.0, 0.0),
        Point::from_xy(SHELL_W / 2.0, SHELL_H),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(0x2F, 0x32, 0x42, 0xFF)),
            GradientStop::new(0.5, Color::from_rgba8(0x27, 0x2A, 0x35, 0xFF)),
            GradientStop::new(1.0, Color::from_rgba8(0x1C, 0x1F, 0x29, 0xFF)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("shell base gradient");

    // Elliptical glow centered on the beak. Squashing y keeps the center at
    // y=0, so the ellipse stays anchored to the top edge.
    let glow = RadialGradient::new(
        Point::from_xy(SHELL_W / 2.0, 0.0),
        0.0,
        Point::from_xy(SHELL_W / 2.0, 0.0),
        SHELL_W * 0.8,
        vec![
            GradientStop::new(0.0, Color::from_rgba8(0x9A, 0xA3, 0xD4, 0x1F)),
            GradientStop::new(1.0, Color::from_rgba8(0x9A, 0xA3, 0xD4, 0x00)),
        ],
        SpreadMode::Pad,
        Transform::from_scale(1.0, 0.459),
    )
    .expect("shell glow gradient");

    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.shader = base;
    pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
    paint.shader = glow;
    pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);

    let mut stroke_paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    stroke_paint.set_color(Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x14));
    let stroke = Stroke {
        width: 1.0,
        ..Default::default()
    };
    pixmap.stroke_path(&path, &stroke_paint, &stroke, transform, None);

    pixmap
}

/// The shell as a premultiplied-RGBA raster for the in-window texture.
pub fn shell_background(oversample: u32) -> Raster {
    let pixmap = render_shell(oversample);
    Raster {
        width: pixmap.width(),
        height: pixmap.height(),
        rgba: pixmap.take(),
    }
}

/// The shell as a PNG, for offline preview via `awb-app --render-shell`.
pub fn shell_background_png(oversample: u32) -> Vec<u8> {
    render_shell(oversample)
        .encode_png()
        .expect("shell png encoding")
}

/// App icon for the macOS bundle: the "Neon Glow" tile from DESIGN.pen, a deep
/// purple gradient with a lime glow behind the colored glyph.
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

    // Deep-space purple gradient tile.
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.shader = LinearGradient::new(
        Point::from_xy(tile * 0.15, 0.0),
        Point::from_xy(tile * 0.85, tile),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(0x2E, 0x16, 0x6A, 0xFF)),
            GradientStop::new(1.0, Color::from_rgba8(0x69, 0x0B, 0xAA, 0xFF)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("tile gradient");
    pixmap.fill_path(
        &rounded_tile,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );

    // Lime glow behind the glyph.
    let glow_center = Point::from_xy(tile * 0.5, tile * 0.46);
    paint.shader = RadialGradient::new(
        glow_center,
        0.0,
        glow_center,
        tile * 0.42,
        vec![
            GradientStop::new(0.0, Color::from_rgba8(0x8B, 0xEC, 0xB7, 0x66)),
            GradientStop::new(1.0, Color::from_rgba8(0x8B, 0xEC, 0xB7, 0x00)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("glow gradient");
    pixmap.fill_path(
        &rounded_tile,
        &paint,
        FillRule::Winding,
        Transform::identity(),
        None,
    );

    // Glyph: 100-unit viewBox (content spans x 10-90, y 24-75) optically
    // centered around the glow.
    let glyph_scale = tile * 0.62 / VIEWBOX;
    let offset_x = tile * 0.5 - 50.0 * glyph_scale;
    let offset_y = tile * 0.46 - 49.5 * glyph_scale;
    let transform =
        Transform::from_scale(glyph_scale, glyph_scale).post_translate(offset_x, offset_y);

    let glyph_top = offset_y + 24.0 * glyph_scale;
    let glyph_bottom = offset_y + 75.0 * glyph_scale;
    let head = LinearGradient::new(
        Point::from_xy(tile * 0.5, glyph_top),
        Point::from_xy(tile * 0.5, glyph_bottom),
        vec![
            GradientStop::new(0.0, Color::from_rgba8(0x8B, 0xEC, 0xB7, 0xFF)),
            GradientStop::new(1.0, Color::from_rgba8(0x49, 0x7B, 0x60, 0xFF)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("head gradient");
    fill_path_shader(&mut pixmap, LOGO_HEAD, head, transform);

    let wave1 = Color::from_rgba8(0x4B, 0xD0, 0x98, 0xFF);
    let wave2 = Color::from_rgba8(0x4D, 0x9F, 0xF5, 0xFF);
    fill_path(&mut pixmap, LOGO_WAVE_1, wave1, transform);
    fill_path(&mut pixmap, LOGO_WAVE_2, wave2, transform);

    pixmap.encode_png().expect("png encoding")
}

fn skia_path(svg_path: &str) -> Option<tiny_skia::Path> {
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

    builder.finish()
}

fn fill_path(pixmap: &mut Pixmap, svg_path: &str, color: Color, transform: Transform) {
    let mut paint = Paint::default();
    paint.set_color(color);
    paint.anti_alias = true;
    fill_with_paint(pixmap, svg_path, &paint, transform);
}

fn fill_path_shader(pixmap: &mut Pixmap, svg_path: &str, shader: Shader, transform: Transform) {
    let paint = Paint {
        shader,
        anti_alias: true,
        ..Default::default()
    };
    fill_with_paint(pixmap, svg_path, &paint, transform);
}

fn fill_with_paint(pixmap: &mut Pixmap, svg_path: &str, paint: &Paint, transform: Transform) {
    let Some(path) = skia_path(svg_path) else {
        return;
    };
    pixmap.fill_path(&path, paint, FillRule::EvenOdd, transform, None);
}
