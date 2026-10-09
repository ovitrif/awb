//! Rasterizes the awb glyph (bugdroid dome + Wi-Fi waves) and the popover
//! shell (beak + gradient) from the SVG path geometry in DESIGN.pen.

use kurbo::{BezPath, PathEl};
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Mask, Paint, PathBuilder, Pixmap, PixmapPaint,
    Point, PremultipliedColorU8, RadialGradient, Shader, SpreadMode, Stroke, Transform,
};

use crate::theme::Appearance;

/// Visual bounds of the glyph inside the 100-unit viewBox: [x, y, w, h]. The
/// dome bottom sits at y=75 and the outer Wi-Fi wave peaks at y=24.
const GLYPH_BOUNDS: [f32; 4] = [9.81, 24.0, 80.38, 51.0];

/// Colored logo: green head with punched eyes, blue Wi-Fi waves.
const LOGO_HEAD: &str = "M25 75a25 25 0 0 1 50 0z m13.5-12a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0z m16 0a3.5 3.5 0 1 0 7 0 3.5 3.5 0 1 0-7 0z";
const LOGO_WAVE_1: &str = "M20.06 51.6a38 38 0 0 1 59.88 0l-5.51 4.31a31 31 0 0 0-48.86 0z";
const LOGO_WAVE_2: &str = "M9.81 43.6a51 51 0 0 1 80.38 0l-5.52 4.31a44 44 0 0 0-69.34 0z";
const DISCONNECTED_WAVE_1: &str = "M20.06 51.6a38 38 0 0 1 59.88 0l-3.94 3.08a33 33 0 0 0-52.02 0z";
const DISCONNECTED_WAVE_2: &str = "M9.81 43.6a51 51 0 0 1 80.38 0l-3.94 3.08a46 46 0 0 0-72.5 0z";

/// Popover shell: a 380-wide rounded body with a beak pointing up at the menu
/// bar icon. Coordinates match the `Shell Shape` path in DESIGN.pen; the body
/// height follows the window height.
const SHELL_W: f32 = 380.0;

fn shell_path(height: f32) -> String {
    let side = height - 9.0 - 28.0;
    format!(
        "M14 9l161 0c5 0 8.6-1.2 11-4.8 1.2-1.8 2.2-2.7 4-2.7 1.8 0 2.8 0.9 4 2.7 2.4 3.6 6 4.8 11 4.8l161 0a14 14 0 0 1 14 14l0 {side}a14 14 0 0 1-14 14l-352 0a14 14 0 0 1-14-14l0-{side}a14 14 0 0 1 14-14z"
    )
}

const VIEWBOX: f32 = 100.0;
const APP_ICON_GLYPH_SCALE: f32 = 0.98;

pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

/// Menu bar icon: the glyph as a black template image, tightly fit to its
/// bounds and rendered taller than its slot so macOS downscales it into a crisp
/// white (dark mode) or black (light mode) status item. The disconnected state
/// uses narrower, fully opaque Wi-Fi arcs while the connected state retains the
/// original wave weight, making connection state readable without a faint icon.
pub fn menubar_icon(height: u32, connected: bool) -> Raster {
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
    fill_path(&mut pixmap, LOGO_HEAD, Color::BLACK, transform);
    let (wave_1, wave_2) = if connected {
        (LOGO_WAVE_1, LOGO_WAVE_2)
    } else {
        (DISCONNECTED_WAVE_1, DISCONNECTED_WAVE_2)
    };
    fill_path(&mut pixmap, wave_1, Color::BLACK, transform);
    fill_path(&mut pixmap, wave_2, Color::BLACK, transform);

    Raster {
        width,
        height,
        rgba: pixmap.take(),
    }
}

/// Preview of the disconnected (top) and connected (bottom) weights as macOS
/// template images on dark and light menu bars. For `awb-app --render-menubar`.
pub fn menubar_preview_png() -> Vec<u8> {
    let disconnected = menubar_icon(44, false);
    let connected = menubar_icon(44, true);
    let zoom = 6;
    let inset = 28;
    let cell_w = connected.width * zoom + inset * 2;
    let cell_h = connected.height * zoom + inset * 2;

    let mut pixmap = Pixmap::new(cell_w * 2, cell_h * 2).expect("preview pixmap");
    fill_cell(
        &mut pixmap,
        0.0,
        cell_w as f32,
        (cell_h * 2) as f32,
        "#1D1D1F",
    );
    fill_cell(
        &mut pixmap,
        cell_w as f32,
        cell_w as f32,
        (cell_h * 2) as f32,
        "#ECECEE",
    );

    let paint = PixmapPaint::default();
    let place = |dx: u32, dy: u32| {
        Transform::from_scale(zoom as f32, zoom as f32).post_translate(dx as f32, dy as f32)
    };
    for (row, icon) in [&disconnected, &connected].into_iter().enumerate() {
        let y = row as u32 * cell_h + inset;
        pixmap.draw_pixmap(
            0,
            0,
            recolor(icon, 0xFF).as_ref(),
            &paint,
            place(inset, y),
            None,
        );
        pixmap.draw_pixmap(
            0,
            0,
            recolor(icon, 0x00).as_ref(),
            &paint,
            place(cell_w + inset, y),
            None,
        );
    }

    pixmap.encode_png().expect("preview png")
}

fn recolor(icon: &Raster, level: u8) -> Pixmap {
    let mut pixmap = Pixmap::new(icon.width, icon.height).expect("recolor pixmap");
    for (pixel, chunk) in pixmap
        .pixels_mut()
        .iter_mut()
        .zip(icon.rgba.as_chunks::<4>().0)
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

struct ShellStyle {
    base: [Color; 3],
    /// Color the bottom edge is lit with, as an ellipse fading up the body.
    lift: Color,
    /// Soft bloom and hairline along the lit bottom edge.
    glow: Color,
    highlight: Color,
    border: Color,
}

fn shell_style(appearance: Appearance) -> ShellStyle {
    match appearance {
        Appearance::Night => ShellStyle {
            base: [
                Color::from_rgba8(0x2A, 0x2D, 0x39, 0xFF),
                Color::from_rgba8(0x22, 0x24, 0x2E, 0xFF),
                Color::from_rgba8(0x1B, 0x1D, 0x25, 0xFF),
            ],
            lift: Color::from_rgba8(0xB4, 0xBE, 0xEC, 0x1C),
            glow: Color::from_rgba8(0xB8, 0xC2, 0xF2, 0x30),
            highlight: Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x16),
            border: Color::from_rgba8(0xFF, 0xFF, 0xFF, 0x17),
        },
        Appearance::Day => ShellStyle {
            base: [
                Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xFF),
                Color::from_rgba8(0xF7, 0xF8, 0xFB, 0xFF),
                Color::from_rgba8(0xEE, 0xF1, 0xF7, 0xFF),
            ],
            lift: Color::from_rgba8(0xC4, 0xD4, 0xF6, 0x58),
            glow: Color::from_rgba8(0x7E, 0x9C, 0xE6, 0x2E),
            highlight: Color::from_rgba8(0xFF, 0xFF, 0xFF, 0xD0),
            border: Color::from_rgba8(0x1B, 0x22, 0x3A, 0x1E),
        },
    }
}

fn transparent(color: Color) -> Color {
    let mut color = color;
    color.set_alpha(0.0);
    color
}

/// An elliptical radial fade centered on `(cx, cy)` with radii `rx`/`ry`,
/// opaque at the center and transparent from `reach` (0-1) outward.
fn ellipse_glow(cx: f32, cy: f32, rx: f32, ry: f32, color: Color, reach: f32) -> Shader<'static> {
    RadialGradient::new(
        Point::from_xy(0.0, 0.0),
        0.0,
        Point::from_xy(0.0, 0.0),
        rx,
        vec![
            GradientStop::new(0.0, color),
            GradientStop::new(reach, transparent(color)),
        ],
        SpreadMode::Pad,
        Transform::from_translate(cx, cy).pre_scale(1.0, ry / rx),
    )
    .expect("shell glow gradient")
}

/// A horizontal line brightest in the middle and fading out toward both sides.
fn edge_line(color: Color) -> Shader<'static> {
    LinearGradient::new(
        Point::from_xy(0.0, 0.0),
        Point::from_xy(SHELL_W, 0.0),
        vec![
            GradientStop::new(0.08, transparent(color)),
            GradientStop::new(0.5, color),
            GradientStop::new(0.92, transparent(color)),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("shell edge gradient")
}

/// Rasterizes the popover shell (beak + rounded body): a vertical base
/// gradient lit from the bottom edge like the Agents Board cards, with a soft
/// bloom and a center-weighted hairline on that edge, an inner highlight along
/// the top and a hairline border. Drawn at `oversample` resolution.
fn render_shell(oversample: u32, appearance: Appearance, height: f32, gradients: bool) -> Pixmap {
    let w = (SHELL_W as u32) * oversample;
    let h = (height.round() as u32) * oversample;
    let mut pixmap = Pixmap::new(w, h).expect("shell pixmap");
    let transform = Transform::from_scale(oversample as f32, oversample as f32);

    let Some(path) = skia_path(&shell_path(height)) else {
        return pixmap;
    };
    let mut clip = Mask::new(w, h).expect("shell mask");
    clip.fill_path(&path, FillRule::Winding, true, transform);

    let style = shell_style(appearance);
    let body_top = 9.0;
    let body_height = height - body_top;
    let center = SHELL_W / 2.0;

    let base = LinearGradient::new(
        Point::from_xy(center, 0.0),
        Point::from_xy(center, height),
        vec![
            GradientStop::new(0.0, style.base[0]),
            GradientStop::new(0.5, style.base[1]),
            GradientStop::new(1.0, style.base[2]),
        ],
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("shell base gradient");

    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    let full = tiny_skia::Rect::from_xywh(0.0, 0.0, SHELL_W, height).expect("shell rect");
    let mut fill = |shader: Shader<'static>, rect: tiny_skia::Rect| {
        paint.shader = shader;
        pixmap.fill_rect(rect, &paint, transform, Some(&clip));
    };

    if !gradients {
        // Plain surface: one flat color under the hairline border.
        paint.set_color(style.base[1]);
        pixmap.fill_rect(full, &paint, transform, Some(&clip));
        stroke_border(&mut pixmap, &path, style.border, transform);
        return pixmap;
    }

    fill(base, full);
    fill(
        ellipse_glow(
            center,
            height,
            SHELL_W * 0.8,
            body_height * 1.1,
            style.lift,
            0.78,
        ),
        full,
    );
    fill(
        ellipse_glow(center, height, SHELL_W * 0.6, 34.0, style.glow, 0.72),
        full,
    );
    fill(
        edge_line(style.glow.with_alpha_scaled(2.4)),
        tiny_skia::Rect::from_xywh(0.0, height - 1.5, SHELL_W, 1.5).expect("glow line"),
    );
    fill(
        edge_line(style.highlight),
        tiny_skia::Rect::from_xywh(0.0, body_top, SHELL_W, 1.2).expect("highlight line"),
    );
    // A faint lift under the beak keeps the anchor point readable.
    fill(
        ellipse_glow(center, body_top, 70.0, 18.0, style.highlight, 1.0),
        full,
    );

    stroke_border(&mut pixmap, &path, style.border, transform);
    pixmap
}

fn stroke_border(pixmap: &mut Pixmap, path: &tiny_skia::Path, color: Color, transform: Transform) {
    let mut paint = Paint {
        anti_alias: true,
        ..Default::default()
    };
    paint.set_color(color);
    let stroke = Stroke {
        width: 1.0,
        ..Default::default()
    };
    pixmap.stroke_path(path, &paint, &stroke, transform, None);
}

trait ScaleAlpha {
    fn with_alpha_scaled(self, factor: f32) -> Self;
}

impl ScaleAlpha for Color {
    fn with_alpha_scaled(mut self, factor: f32) -> Self {
        self.set_alpha((self.alpha() * factor).min(1.0));
        self
    }
}

/// The shell as a premultiplied-RGBA raster for the in-window texture.
pub fn shell_background(
    oversample: u32,
    appearance: Appearance,
    height: f32,
    gradients: bool,
) -> Raster {
    let pixmap = render_shell(oversample, appearance, height, gradients);
    Raster {
        width: pixmap.width(),
        height: pixmap.height(),
        rgba: pixmap.take(),
    }
}

/// The shell as a PNG, for offline preview via `awb-app --render-shell`.
pub fn shell_background_png(oversample: u32, appearance: Appearance, height: f32) -> Vec<u8> {
    render_shell(oversample, appearance, height, true)
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
    let glyph_scale = tile * APP_ICON_GLYPH_SCALE / VIEWBOX;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_icon_glyph_uses_the_tile_footprint() {
        let width_fraction = GLYPH_BOUNDS[2] * APP_ICON_GLYPH_SCALE / VIEWBOX;
        let height_fraction = GLYPH_BOUNDS[3] * APP_ICON_GLYPH_SCALE / VIEWBOX;

        assert!(width_fraction >= 0.75);
        assert!(height_fraction >= 0.48);
    }

    #[test]
    fn day_and_night_shells_share_geometry_but_not_pixels() {
        let day = shell_background(1, Appearance::Day, 349.0, true);
        let night = shell_background(1, Appearance::Night, 349.0, true);

        assert_eq!((day.width, day.height), (380, 349));
        assert_eq!((night.width, night.height), (380, 349));
        assert_ne!(day.rgba, night.rgba);
    }

    #[test]
    fn disconnected_menu_icon_preserves_size_with_lighter_weight() {
        let disconnected = menubar_icon(44, false);
        let connected = menubar_icon(44, true);
        let alpha_coverage = |raster: &Raster| {
            raster
                .rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|pixel| u64::from(pixel[3]))
                .sum::<u64>()
        };

        assert_eq!(disconnected.width, connected.width);
        assert_eq!(disconnected.height, connected.height);
        let disconnected_coverage = alpha_coverage(&disconnected);
        let connected_coverage = alpha_coverage(&connected);
        assert!(disconnected_coverage < connected_coverage);
        assert!(disconnected_coverage * 4 >= connected_coverage * 3);
    }
}
