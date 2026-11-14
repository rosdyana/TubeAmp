//! Liberation Sans text for the windows that show track names and labels (the playlist editor and
//! the media library). Glyphs are rasterised once with fontdue and blended onto the canvas, so the
//! strokes keep their anti-aliased edges. Liberation Sans is SIL OFL (see `assets/fonts/`).

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use fontdue::{Font, FontSettings};

use crate::canvas::{Canvas, Rect};

/// Pixel size of the track font. 10 px keeps Liberation Sans's strokes (the t crossbar, the
/// n in Lanterns) at 9 px-ish x-heights while still fitting the 13 px row pitch; 9 px dropped them.
pub const FONT_PX: f32 = 10.0;
/// Liberation Sans Regular, SIL Open Font License (see `assets/fonts/`).
const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/LiberationSans-Regular.ttf");
/// Truncation mark, used when the track font has it (Liberation Sans does).
pub const ELLIPSIS: char = '\u{2026}';
/// Truncation mark for a font without [`ELLIPSIS`].
const ASCII_ELLIPSIS: &str = "...";

/// The track font plus the glyphs rasterised so far, keyed by character. Glyphs are made on first
/// use, so every character the font maps is available (not only Latin-1).
struct Track {
    font: Font,
    cache: Mutex<HashMap<char, Arc<Glyph>>>,
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

fn track() -> &'static Track {
    static TRACK: OnceLock<Track> = OnceLock::new();
    TRACK.get_or_init(|| Track {
        font: Font::from_bytes(FONT_BYTES, FontSettings::default())
            .expect("the bundled Liberation Sans parses"),
        cache: Mutex::new(HashMap::new()),
    })
}

/// Whether the track font has a glyph for `c`. Characters it lacks are drawn as a replacement box.
pub fn has_glyph(c: char) -> bool {
    track().font.lookup_glyph_index(c) != 0
}

/// The glyph for `c`, rasterised once and then shared.
fn glyph_for(c: char) -> Arc<Glyph> {
    let track = track();
    let mut cache = track.cache.lock().unwrap_or_else(|e| e.into_inner());
    cache
        .entry(c)
        .or_insert_with(|| Arc::new(rasterise(&track.font, c)))
        .clone()
}

fn rasterise(font: &Font, c: char) -> Glyph {
    // Control characters (a stray tab or newline in a title) are blank, not boxes.
    if c.is_control() {
        return rasterise(font, ' ');
    }
    if font.lookup_glyph_index(c) == 0 {
        return replacement_box();
    }
    let (m, coverage) = font.rasterize(c, FONT_PX);
    Glyph {
        advance: m.advance_width,
        xmin: m.xmin,
        ymin: m.ymin,
        w: m.width,
        h: m.height,
        coverage,
    }
}

/// A hollow box for a character the font does not have: visible, unlike a blank or a `?` that
/// could be mistaken for a question mark in the title.
fn replacement_box() -> Glyph {
    const W: usize = 5;
    const H: usize = 8;
    let mut coverage = vec![0u8; W * H];
    for row in 0..H {
        for col in 0..W {
            if row == 0 || row == H - 1 || col == 0 || col == W - 1 {
                coverage[row * W + col] = 255;
            }
        }
    }
    Glyph {
        advance: 7.0,
        xmin: 0,
        ymin: 0,
        w: W,
        h: H,
        coverage,
    }
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
    let mark = if has_glyph(ELLIPSIS) {
        ELLIPSIS.to_string()
    } else {
        ASCII_ELLIPSIS.to_string()
    };
    let chars: Vec<char> = text.chars().collect();
    for keep in (0..chars.len()).rev() {
        let candidate: String = chars[..keep].iter().collect::<String>() + &mark;
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
        let g = &*g;
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

#[cfg(test)]
mod tests {
    use super::*;

    const INK: u32 = 0xFFFF_FFFF;

    fn drawn_pixels(text: &str) -> Vec<u32> {
        let mut canvas = Canvas::new(80, 14);
        draw_track_text(&mut canvas, text, 2, 11, INK, Rect::new(0, 0, 80, 14));
        canvas.px
    }

    #[test]
    fn typography_outside_latin_1_is_in_the_font() {
        for c in ['\u{2022}', ELLIPSIS, '\u{2014}', '\u{2013}', '\u{201C}', '\u{201D}', '\u{2019}', '\u{20AC}'] {
            assert!(has_glyph(c), "U+{:04X} should be in Liberation Sans", c as u32);
        }
    }

    #[test]
    fn characters_the_font_lacks_draw_a_box_not_a_question_mark() {
        for c in ['\u{65E5}', '\u{1F600}'] {
            assert!(!has_glyph(c), "U+{:04X} is not in Liberation Sans", c as u32);
        }
        assert_ne!(drawn_pixels("\u{65E5}"), drawn_pixels("?"));
        assert_ne!(drawn_pixels("\u{1F600}"), drawn_pixels("?"));
        // The box is visible ink, not an empty advance.
        assert!(drawn_pixels("\u{65E5}").iter().any(|&p| p == INK));
        assert_ne!(drawn_pixels("\u{65E5}"), drawn_pixels(" "));
    }

    #[test]
    fn bullet_and_dash_draw_their_own_shapes() {
        assert_ne!(drawn_pixels("\u{2022}"), drawn_pixels("?"));
        assert_ne!(drawn_pixels("\u{2014}"), drawn_pixels("-"));
        assert_ne!(drawn_pixels("\u{201C}a\u{201D}"), drawn_pixels("\"a\""));
    }

    #[test]
    fn control_characters_are_blank_not_boxes() {
        assert_eq!(drawn_pixels("\t"), drawn_pixels(" "));
    }

    #[test]
    fn truncation_uses_the_ellipsis_character() {
        let cut = fit_text("A very long track title indeed", 60.0);
        assert!(cut.ends_with(ELLIPSIS), "{cut}");
        assert!(text_width(&cut) <= 60.0);
        assert_eq!(fit_text("abc", 1000.0), "abc");
        assert_eq!(fit_text("abcdef", 0.0), "");
    }

    #[test]
    fn a_title_full_of_non_latin_text_still_fits_its_width() {
        let title = "\u{65E5}\u{672C}\u{8A9E} \u{2014} \u{1F3B5} \u{201C}mix\u{201D} \u{2022} live";
        let cut = fit_text(title, 50.0);
        assert!(text_width(&cut) <= 50.0, "{cut}");
    }
}
