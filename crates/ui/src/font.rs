//! Liberation Sans text for the windows that show track names and labels (the playlist editor and
//! the media library). Glyphs are rasterised once with fontdue and blended onto the canvas, so the
//! strokes keep their anti-aliased edges. Liberation Sans is SIL OFL (see `assets/fonts/`).

use std::sync::OnceLock;

use fontdue::{Font, FontSettings};

use crate::canvas::{Canvas, Rect};

/// Pixel size of the track font. 10 px keeps Liberation Sans's strokes (the t crossbar, the
/// n in Lanterns) at 9 px-ish x-heights while still fitting the 13 px row pitch; 9 px dropped them.
pub const FONT_PX: f32 = 10.0;
/// Liberation Sans Regular, SIL Open Font License (see `assets/fonts/`).
const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/LiberationSans-Regular.ttf");

struct Face {
    /// One rasterised glyph per Latin-1 code point (0..=255).
    glyphs: Vec<Glyph>,
}

#[derive(Clone)]
struct Glyph {
    advance: f32,
    xmin: i32,
    ymin: i32,
    w: usize,
    h: usize,
    coverage: Vec<u8>,
}

fn face() -> &'static Face {
    static FACE: OnceLock<Face> = OnceLock::new();
    FACE.get_or_init(|| {
        let font = Font::from_bytes(FONT_BYTES, FontSettings::default())
            .expect("the bundled Liberation Sans parses");
        let glyphs = (0u8..=255)
            .map(|byte| {
                let (m, coverage) = font.rasterize(char::from(byte), FONT_PX);
                Glyph {
                    advance: m.advance_width,
                    xmin: m.xmin,
                    ymin: m.ymin,
                    w: m.width,
                    h: m.height,
                    coverage,
                }
            })
            .collect();
        Face { glyphs }
    })
}

/// The glyph for `c`. Characters outside Latin-1 draw as `?`.
fn glyph_for(c: char) -> &'static Glyph {
    let code = if (c as u32) < 256 {
        c as usize
    } else {
        '?' as usize
    };
    &face().glyphs[code]
}

/// Width of `text` in pixels at the track font size.
pub fn text_width(text: &str) -> f32 {
    text.chars().map(|c| glyph_for(c).advance).sum()
}

/// `text`, cut down with an ellipsis so it fits in `max_w` pixels.
pub fn fit_text(text: &str, max_w: f32) -> String {
    if text_width(text) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (0..chars.len()).rev() {
        let candidate: String = chars[..keep].iter().collect::<String>() + "...";
        if text_width(&candidate) <= max_w {
            return candidate;
        }
    }
    String::new()
}

/// Draws `text` with its baseline at `baseline`, starting at `x`, clipped to `clip`. Each glyph's
/// coverage is blended over the pixels already on the canvas (the row background), so strokes keep
/// their anti-aliased edges.
pub fn draw_track_text(
    canvas: &mut Canvas,
    text: &str,
    x: i32,
    baseline: i32,
    color: u32,
    clip: Rect,
) {
    let mut pen = x as f32;
    for c in text.chars() {
        let g = glyph_for(c);
        let gx = pen.round() as i32 + g.xmin;
        let gy = baseline - g.ymin - g.h as i32;
        for row in 0..g.h {
            for col in 0..g.w {
                let a = g.coverage[row * g.w + col];
                if a == 0 {
                    continue;
                }
                let px = gx + col as i32;
                let py = gy + row as i32;
                if clip.contains(px, py)
                    && px >= 0
                    && py >= 0
                    && (px as u32) < canvas.w
                    && (py as u32) < canvas.h
                {
                    let i = (py as u32 * canvas.w + px as u32) as usize;
                    canvas.px[i] = blend(canvas.px[i], color, a);
                }
            }
        }
        pen += g.advance;
    }
}

/// `fg` over `bg` with coverage `a` (0..=255), channel by channel; the result is opaque.
pub fn blend(bg: u32, fg: u32, a: u8) -> u32 {
    let a = u32::from(a);
    let mix = |shift: u32| {
        let b = (bg >> shift) & 0xFF;
        let f = (fg >> shift) & 0xFF;
        (f * a + b * (255 - a) + 127) / 255
    };
    0xFF00_0000 | (mix(16) << 16) | (mix(8) << 8) | mix(0)
}
