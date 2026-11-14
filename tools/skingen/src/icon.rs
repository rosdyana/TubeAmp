//! The TubeAmp app icon, drawn procedurally with anti-aliasing (unlike the skin sheets, which are
//! pixel-crisp): a steel-blue bevelled squircle with a green LCD and five equalizer bars.
//!
//! Every shape is laid out on a 1024-unit master grid and scaled to each output size, so the
//! icon is the same picture at 16 px and at 1024 px. Output is deterministic: the same code
//! always produces the same bytes.
//!
//! No Winamp logo or lightning bolt, and no YouTube logo.

use image::RgbaImage;
use std::error::Error;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use tiny_skia::{
    Color, FillRule, GradientStop, LinearGradient, Paint, PathBuilder, Pixmap, Point, Shader,
    SpreadMode, Stroke, Transform,
};

/// Side of the master grid. Every coordinate below is in these units.
const GRID: f32 = 1024.0;
/// Centre of the squircle.
const CENTRE: f32 = 512.0;
/// Half the squircle's side (824 px across, on a 1024 canvas, as Apple's icon grid).
const HALF: f32 = 412.0;
/// Superellipse exponent: higher is squarer. 5 gives a squircle-like outline.
const SQUIRCLE_N: f32 = 5.0;

/// The LCD bezel and the LCD itself.
const BEZEL: (f32, f32, f32, f32) = (178.0, 318.0, 668.0, 388.0);
const LCD: (f32, f32, f32, f32) = (192.0, 332.0, 640.0, 360.0);

/// Equalizer bars: width, gap, bottom edge, tallest height, and their height fractions.
const BAR_W: f32 = 76.0;
const BAR_GAP: f32 = 32.0;
const BAR_BOTTOM: f32 = 632.0;
const BAR_MAX_H: f32 = 220.0;
const BAR_FRACTIONS: [f32; 5] = [0.52, 0.86, 0.66, 1.0, 0.40];
/// Horizontal LCD segment pitch and the thickness of the dark separator lines.
const SEGMENT_PITCH: f32 = 22.0;
const SEGMENT_LINE: f32 = 5.0;

/// Palette (steel blue metal, green LCD). Colours are `0xRRGGBB` plus alpha.
const STEEL_TOP: [u8; 3] = [0x7D, 0x96, 0xBA];
const STEEL_MID: [u8; 3] = [0x3F, 0x5A, 0x80];
const STEEL_BOT: [u8; 3] = [0x14, 0x20, 0x39];
const RIM_DARK: [u8; 3] = [0x07, 0x0B, 0x14];
const BEZEL_TOP: [u8; 3] = [0x5E, 0x75, 0x94];
const BEZEL_BOT: [u8; 3] = [0x0E, 0x15, 0x24];
const LCD_BLACK: [u8; 3] = [0x05, 0x07, 0x0A];
const LCD_RIM: [u8; 3] = [0x1C, 0x23, 0x33];
const LCD_ON: [u8; 3] = [0x1E, 0xE6, 0x5A];
const LCD_HOT: [u8; 3] = [0xC8, 0xFF, 0xD8];
const LCD_DEEP: [u8; 3] = [0x0B, 0x7A, 0x3A];

/// The iconset members as `(file name, pixel size)`, in Apple's naming.
pub const ICONSET_SIZES: [(&str, u32); 10] = [
    ("icon_16x16.png", 16),
    ("icon_16x16@2x.png", 32),
    ("icon_32x32.png", 32),
    ("icon_32x32@2x.png", 64),
    ("icon_128x128.png", 128),
    ("icon_128x128@2x.png", 256),
    ("icon_256x256.png", 256),
    ("icon_256x256@2x.png", 512),
    ("icon_512x512.png", 512),
    ("icon_512x512@2x.png", 1024),
];

/// Writes `<out_dir>/TubeAmp.iconset/*.png` and `<out_dir>/TubeAmp.icns` (built with `iconutil`).
/// Returns the written paths.
pub fn write(out_dir: &Path) -> Result<Vec<PathBuf>, Box<dyn Error>> {
    let iconset = out_dir.join("TubeAmp.iconset");
    fs::create_dir_all(&iconset)?;
    let mut written = Vec::new();
    for (name, size) in ICONSET_SIZES {
        let path = iconset.join(name);
        render(size).save(&path)?;
        written.push(path);
    }
    let icns = out_dir.join("TubeAmp.icns");
    let status = Command::new("iconutil")
        .arg("-c")
        .arg("icns")
        .arg("-o")
        .arg(&icns)
        .arg(&iconset)
        .status()
        .map_err(|e| format!("iconutil could not run (macOS only): {e}"))?;
    if !status.success() {
        return Err(format!("iconutil failed with {status}").into());
    }
    written.push(icns);
    Ok(written)
}

/// Renders the icon at `size` by `size` pixels, as straight (non-premultiplied) RGBA.
pub fn render(size: u32) -> RgbaImage {
    let mut pm = Pixmap::new(size, size).expect("icon size is non-zero");
    let t = Transform::from_scale(size as f32 / GRID, size as f32 / GRID);

    // Steel body: the squircle filled with a top-to-bottom gradient, then a gloss on top.
    let body = squircle(CENTRE, CENTRE, HALF);
    fill(
        &mut pm,
        &body,
        grad(
            100.0,
            924.0,
            &[
                (0.0, STEEL_TOP, 255),
                (0.45, STEEL_MID, 255),
                (1.0, STEEL_BOT, 255),
            ],
        ),
        t,
    );
    fill(
        &mut pm,
        &body,
        grad(100.0, 512.0, &[(0.0, [255; 3], 56), (1.0, [255; 3], 0)]),
        t,
    );

    // Bevel: a dark outer rim, a light inner edge at the top, a shadow inner edge at the bottom.
    stroke(
        &mut pm,
        &squircle(CENTRE, CENTRE, HALF - 6.0),
        solid(RIM_DARK, 220),
        12.0,
        t,
    );
    stroke(
        &mut pm,
        &squircle(CENTRE, CENTRE, HALF - 18.0),
        grad(
            100.0,
            924.0,
            &[(0.0, [255; 3], 200), (0.5, [255; 3], 26), (1.0, [0; 3], 0)],
        ),
        6.0,
        t,
    );
    stroke(
        &mut pm,
        &squircle(CENTRE, CENTRE, HALF - 26.0),
        grad(100.0, 924.0, &[(0.0, [0; 3], 0), (1.0, [0; 3], 110)]),
        6.0,
        t,
    );

    // Bezel around the LCD, then the black LCD glass with its rim.
    let (bx, by, bw, bh) = BEZEL;
    fill(
        &mut pm,
        &rrect(bx, by, bw, bh, 56.0),
        grad(by, by + bh, &[(0.0, BEZEL_TOP, 255), (1.0, BEZEL_BOT, 255)]),
        t,
    );
    let (lx, ly, lw, lh) = LCD;
    fill(
        &mut pm,
        &rrect(lx, ly, lw, lh, 40.0),
        solid(LCD_BLACK, 255),
        t,
    );
    stroke(
        &mut pm,
        &rrect(lx, ly, lw, lh, 40.0),
        solid(LCD_RIM, 255),
        8.0,
        t,
    );

    // Equalizer bars: a soft halo, the bar, then dark segment lines over it, and a peak cap.
    for (i, fraction) in BAR_FRACTIONS.iter().enumerate() {
        let x = bar_x(i);
        let h = BAR_MAX_H * fraction;
        let top = BAR_BOTTOM - h;
        for (grow, alpha) in [(22.0, 26u8), (12.0, 36), (5.0, 48)] {
            fill(
                &mut pm,
                &rrect(
                    x - grow,
                    top - grow,
                    BAR_W + 2.0 * grow,
                    h + 2.0 * grow,
                    14.0 + grow,
                ),
                solid(LCD_ON, alpha),
                t,
            );
        }
        fill(
            &mut pm,
            &rrect(x, top, BAR_W, h, 10.0),
            grad(
                top,
                BAR_BOTTOM,
                &[
                    (0.0, LCD_HOT, 255),
                    (0.35, LCD_ON, 255),
                    (1.0, LCD_DEEP, 255),
                ],
            ),
            t,
        );
        let mut y = BAR_BOTTOM - SEGMENT_PITCH;
        while y > top {
            fill(
                &mut pm,
                &rect(x, y, BAR_W, SEGMENT_LINE),
                solid(LCD_BLACK, 230),
                t,
            );
            y -= SEGMENT_PITCH;
        }
        fill(
            &mut pm,
            &rrect(x, top - 26.0, BAR_W, 9.0, 4.0),
            solid(LCD_HOT, 255),
            t,
        );
    }

    unpremultiply(pm.data(), size)
}

/// X of bar `i`: the bars are centred in the LCD.
fn bar_x(i: usize) -> f32 {
    let total = BAR_FRACTIONS.len() as f32 * BAR_W + (BAR_FRACTIONS.len() as f32 - 1.0) * BAR_GAP;
    CENTRE - total / 2.0 + i as f32 * (BAR_W + BAR_GAP)
}

/// A superellipse centred on `(cx, cy)` with half-size `r`.
fn squircle(cx: f32, cy: f32, r: f32) -> tiny_skia::Path {
    const STEPS: usize = 720;
    let mut pb = PathBuilder::new();
    for k in 0..STEPS {
        let t = std::f32::consts::TAU * k as f32 / STEPS as f32;
        let (s, c) = t.sin_cos();
        let x = cx + r * signed_pow(c, 2.0 / SQUIRCLE_N);
        let y = cy + r * signed_pow(s, 2.0 / SQUIRCLE_N);
        if k == 0 {
            pb.move_to(x, y);
        } else {
            pb.line_to(x, y);
        }
    }
    pb.close();
    pb.finish().expect("squircle path")
}

fn signed_pow(v: f32, e: f32) -> f32 {
    v.signum() * v.abs().powf(e)
}

/// A rounded rectangle; corners are polylines with 12 segments each.
fn rrect(x: f32, y: f32, w: f32, h: f32, r: f32) -> tiny_skia::Path {
    const SEG: usize = 12;
    let r = r.min(w / 2.0).min(h / 2.0);
    // Corner centres with their start angles (degrees): top-right, bottom-right, bottom-left,
    // top-left. Screen y points down, so these run clockwise.
    let corners = [
        (x + w - r, y + r, -90.0f32),
        (x + w - r, y + h - r, 0.0),
        (x + r, y + h - r, 90.0),
        (x + r, y + r, 180.0),
    ];
    let mut pb = PathBuilder::new();
    let mut first = true;
    for (cx, cy, start) in corners {
        for s in 0..=SEG {
            let a = (start + 90.0 * s as f32 / SEG as f32).to_radians();
            let (px, py) = (cx + r * a.cos(), cy + r * a.sin());
            if first {
                pb.move_to(px, py);
                first = false;
            } else {
                pb.line_to(px, py);
            }
        }
    }
    pb.close();
    pb.finish().expect("rounded rect path")
}

fn rect(x: f32, y: f32, w: f32, h: f32) -> tiny_skia::Path {
    rrect(x, y, w, h, 0.0)
}

fn solid(rgb: [u8; 3], alpha: u8) -> Shader<'static> {
    Shader::SolidColor(Color::from_rgba8(rgb[0], rgb[1], rgb[2], alpha))
}

/// A vertical gradient from `y0` to `y1`. Each stop is `(position 0..1, rgb, alpha)`.
fn grad(y0: f32, y1: f32, stops: &[(f32, [u8; 3], u8)]) -> Shader<'static> {
    let stops = stops
        .iter()
        .map(|&(pos, rgb, alpha)| {
            GradientStop::new(pos, Color::from_rgba8(rgb[0], rgb[1], rgb[2], alpha))
        })
        .collect();
    LinearGradient::new(
        Point::from_xy(CENTRE, y0),
        Point::from_xy(CENTRE, y1),
        stops,
        SpreadMode::Pad,
        Transform::identity(),
    )
    .expect("gradient has stops")
}

fn fill(pm: &mut Pixmap, path: &tiny_skia::Path, shader: Shader<'static>, t: Transform) {
    let paint = Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    };
    pm.fill_path(path, &paint, FillRule::Winding, t, None);
}

fn stroke(
    pm: &mut Pixmap,
    path: &tiny_skia::Path,
    shader: Shader<'static>,
    width: f32,
    t: Transform,
) {
    let paint = Paint {
        shader,
        anti_alias: true,
        ..Paint::default()
    };
    let stroke = Stroke {
        width,
        ..Stroke::default()
    };
    pm.stroke_path(path, &paint, &stroke, t, None);
}

/// Premultiplied pixmap bytes to straight RGBA.
fn unpremultiply(data: &[u8], size: u32) -> RgbaImage {
    let mut out = Vec::with_capacity(data.len());
    for px in data.as_chunks::<4>().0 {
        let a = u32::from(px[3]);
        // Fully transparent pixels (a == 0) become 0,0,0,0.
        for &c in &px[..3] {
            let v = (u32::from(c) * 255 + a / 2).checked_div(a).unwrap_or(0);
            out.push(v.min(255) as u8);
        }
        out.push(px[3]);
    }
    RgbaImage::from_raw(size, size, out).expect("pixel count matches size")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_are_transparent_and_the_centre_bar_glows_green() {
        let img = render(128);
        assert_eq!(
            img.get_pixel(0, 0)[3],
            0,
            "outside the squircle is transparent"
        );
        assert_eq!(img.get_pixel(127, 127)[3], 0);
        // The tallest bar (index 3), sampled in the gap under the lowest segment line.
        let x = (bar_x(3) + BAR_W / 2.0) * 128.0 / GRID;
        let y = (BAR_BOTTOM - 8.0) * 128.0 / GRID;
        let p = img.get_pixel(x as u32, y as u32);
        assert!(p[1] > p[0] + 60, "bar should be green, got {p:?}");
    }

    #[test]
    fn rendering_is_deterministic() {
        assert_eq!(render(64).as_raw(), render(64).as_raw());
    }

    #[test]
    fn iconset_names_follow_apple_convention() {
        assert_eq!(ICONSET_SIZES.len(), 10);
        assert_eq!(ICONSET_SIZES[9], ("icon_512x512@2x.png", 1024));
        assert_eq!(ICONSET_SIZES[0], ("icon_16x16.png", 16));
    }
}
