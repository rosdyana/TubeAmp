//! TEXT.BMP font rendering and the scrolling marquee.
//!
//! Marquee rules follow Webamp's `Marquee.tsx`: texts of 31+ characters scroll
//! with a `"  ***  "` separator, shorter texts are padded to 31 characters and
//! do not scroll. One step moves one 5 px character cell.

use crate::canvas::{Canvas, Rect};
use tubeamp_skin::{sprites, Sheet, Skin};

/// Glyph cell width in pixels.
pub const GLYPH_W: i32 = 5;
/// Glyph cell height in pixels.
pub const GLYPH_H: i32 = 6;
/// Separator placed between repetitions of a scrolling title.
pub const MARQUEE_SEPARATOR: &str = "  ***  ";
/// Titles at least this many characters long scroll (Webamp `MARQUEE_MAX_LENGTH`).
pub const MARQUEE_MIN_LEN: usize = 31;

/// Draws `text` with the TEXT.BMP font, top-left at `(x, y)`, clipped to `clip`.
/// Characters without a glyph are drawn as spaces (nothing visible).
pub fn draw_text(canvas: &mut Canvas, skin: &Skin, text: &str, x: i32, y: i32, clip: Rect) {
    let mut cx = x;
    for ch in text.chars() {
        let (row, col) = sprites::font_cell(ch);
        if let Some(glyph) = skin.region(
            Sheet::Text,
            col * GLYPH_W as u32,
            row * GLYPH_H as u32,
            GLYPH_W as u32,
            GLYPH_H as u32,
        ) {
            canvas.blit_clipped(&glyph, cx, y, clip);
        }
        cx += GLYPH_W;
    }
}

/// Width in pixels of one full scroll cycle, or 0 if `text` does not scroll.
pub fn marquee_period_px(text: &str) -> u32 {
    let n = text.chars().count();
    if n < MARQUEE_MIN_LEN {
        return 0;
    }
    ((n + MARQUEE_SEPARATOR.chars().count()) as u32) * GLYPH_W as u32
}

/// The characters to draw for a marquee showing `width_px` pixels of `text`
/// scrolled by `offset_px`.
///
/// The result starts at character `offset_px / 5` of the cycle and is long
/// enough to cover `width_px` plus one partial cell. Draw it at
/// `x = left - (offset_px % 5)` clipped to the marquee width. Long texts wrap
/// through the separator; short texts are padded and never scroll.
pub fn marquee_frame(text: &str, width_px: u32, offset_px: u32) -> String {
    let chars: Vec<char> = text.chars().collect();
    let scrolls = chars.len() >= MARQUEE_MIN_LEN;
    let cycle: Vec<char> = if scrolls {
        chars
            .iter()
            .copied()
            .chain(MARQUEE_SEPARATOR.chars())
            .collect()
    } else {
        let mut padded = chars;
        padded.resize(MARQUEE_MIN_LEN, ' ');
        padded
    };
    let period = cycle.len();
    let start = if scrolls {
        (offset_px / GLYPH_W as u32) as usize % period
    } else {
        0
    };
    let cells = width_px.div_ceil(GLYPH_W as u32) as usize + 2;
    (0..cells).map(|i| cycle[(start + i) % period]).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tubeamp_skin::testkit;

    #[test]
    fn short_titles_are_padded_and_never_scroll() {
        assert_eq!(marquee_period_px("abc"), 0);
        let frame = marquee_frame("abc", 154, 40);
        assert!(frame.starts_with("abc"));
        assert_eq!(frame.chars().nth(3), Some(' '));
        assert_eq!(frame, marquee_frame("abc", 154, 0));
    }

    #[test]
    fn long_titles_scroll_one_cell_per_step_and_wrap_through_the_separator() {
        let title = "0123456789012345678901234567890"; // 31 chars
        assert_eq!(marquee_period_px(title), (31 + 7) * 5);
        let at0 = marquee_frame(title, 154, 0);
        assert!(at0.starts_with(title));
        let at5 = marquee_frame(title, 154, 5);
        assert!(at5.starts_with(&title[1..]));
        // Scrolling by the full period returns to the start.
        let period = marquee_period_px(title);
        assert_eq!(marquee_frame(title, 154, period), at0);
        // The cell after the title is the separator.
        assert_eq!(marquee_frame(title, 154, 31 * 5).chars().next(), Some(' '));
    }

    #[test]
    fn glyphs_are_drawn_at_their_font_cells() {
        let skin = testkit::synthetic_skin();
        let mut c = Canvas::new(40, 10);
        draw_text(&mut c, &skin, "1", 2, 1, Rect::new(0, 0, 40, 10));
        // '1' is cell (row 1, col 1): TEXT sheet at (5, 6).
        assert_eq!(testkit::decode_coord(c.px[40 + 2]), (5, 6));
        assert_eq!(testkit::decode_coord(c.px[6 * 40 + 6]), (9, 11));
    }

    #[test]
    fn text_is_clipped_to_the_given_rectangle() {
        let skin = testkit::synthetic_skin();
        let mut c = Canvas::new(40, 10);
        let before = c.px.clone();
        draw_text(&mut c, &skin, "abcdefgh", 0, 0, Rect::new(0, 0, 12, 6));
        for y in 0..10 {
            for x in 0..40 {
                let i = y * 40 + x;
                if x >= 12 || y >= 6 {
                    assert_eq!(c.px[i], before[i], "pixel ({x},{y}) must stay untouched");
                }
            }
        }
    }
}
