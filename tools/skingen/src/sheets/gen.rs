//! GEN.BMP (194×109): the generic (gen) window frame: title tiles selected (y 0)
//! and normal (y 21), bottom bars, side pieces and the close pip.

use crate::font;
use crate::paint::{
    hex, pip, title_strip, Canvas, Rgb, Strip, BODY_BOT, BODY_FLAT, BODY_TOP, ICON_CLOSE, INK,
    METAL_HI, METAL_LO, TITLE_ACTIVE, TITLE_INACTIVE,
};

/// Title letters: row 88 holds the selected (bright) A–Z, row 96 the normal (dim) ones. A
/// reader splits a row into letters at the single background column between them, so each
/// letter has a solid top bar across its width, with its glyph below.
const LETTER_ROWS: [(i32, Rgb); 2] = [(88, hex(0xFFFFFF)), (96, hex(0xB4BAC6))];
/// The sheet width, which the letter row must fit in.
const SHEET_W: i32 = 194;

pub fn render() -> Canvas {
    let mut c = Canvas::new(194, 109);

    // Top tiles, selected at y 0 and normal at y 21 (25×20 each).
    top_row(&mut c, 0, &TITLE_ACTIVE);
    top_row(&mut c, 21, &TITLE_INACTIVE);

    // Bottom bars (14 px tall): left (0,42) and right (0,57) 125 wide, fill (127,72) 25 wide.
    bottom_bar(&mut c, 0, 42, 125, false);
    bottom_bar(&mut c, 0, 57, 125, true);
    bottom_bar(&mut c, 127, 72, 25, false);

    // Side pieces (vertical, repeating): middle left 11×29 / 11×24 and right 8×29 / 8×24.
    side_left(&mut c, 127, 42, 29, false);
    side_left(&mut c, 158, 42, 24, true);
    side_right(&mut c, 139, 42, 29, false);
    side_right(&mut c, 170, 42, 24, true);

    // Close pip (selected) at (148,42).
    pip(&mut c, 148, 42, &ICON_CLOSE, true, INK);

    title_letters(&mut c);
    c
}

/// Draws the A–Z title letters on rows 88 (selected) and 96 (normal).
fn title_letters(c: &mut Canvas) {
    for (row, colour) in LETTER_ROWS {
        let mut x = 1;
        for ch in 'A'..='Z' {
            let rows = font::glyph(ch);
            let w = glyph_width(&rows) + 1;
            c.fill(x, row, w, 1, colour); // solid top bar keeps the letter one run
            for (ry, line) in rows.iter().take(5).enumerate() {
                for (cx, b) in line.chars().enumerate() {
                    if b == '#' {
                        c.set(x + cx as i32, row + 1 + ry as i32, colour);
                    }
                }
            }
            x += w + 1; // one background column before the next letter
        }
        assert!(x - 1 <= SHEET_W, "the 26 letters fit in the sheet width");
    }
}

/// Ink width of a glyph in columns.
fn glyph_width(rows: &font::Glyph) -> i32 {
    rows.iter()
        .map(|r| r.rfind('#').map_or(0, |i| i as i32 + 1))
        .max()
        .unwrap_or(0)
}

/// One row of top tiles: left corner, left end, centre fill, right end, the fill that
/// joins both ends, and the right corner (each 25×20).
fn top_row(c: &mut Canvas, y: i32, s: &Strip) {
    for x in [0, 26, 52, 78, 104] {
        title_strip(c, x, y, 25, 20, s);
    }
    c.fill(0, y, 1, 20, METAL_LO); // outer left edge of the left corner
    title_strip(c, 130, y, 25, 20, s); // top-right corner
    c.fill(154, y, 1, 20, METAL_LO); // outer right edge
}

/// A 14 px bottom bar: gradient body with a highlight on top and a shadow below.
/// `grip` adds the resize grip in its bottom-right corner.
fn bottom_bar(c: &mut Canvas, x: i32, y: i32, w: i32, grip: bool) {
    c.vgrad(x, y + 1, w, 12, BODY_TOP, BODY_BOT);
    c.fill(x, y, w, 1, METAL_HI);
    c.fill(x, y + 13, w, 1, METAL_LO);
    if grip {
        // Three diagonal lines in the bottom-right corner.
        for d in 0..3 {
            for t in 0..=(d * 3 + 2) {
                c.set(x + w - 2 - t, y + 12 - (d * 3 + 2 - t), METAL_HI);
            }
        }
    }
}

/// Left side piece: outer dark edge, bevel, body, inner dark edge.
fn side_left(c: &mut Canvas, x: i32, y: i32, h: i32, bottom: bool) {
    c.fill(x, y, 11, h, BODY_FLAT);
    c.fill(x, y, 1, h, METAL_LO);
    c.fill(x + 1, y, 1, h, METAL_HI);
    c.fill(x + 10, y, 1, h, METAL_LO);
    if bottom {
        c.fill(x, y + h - 1, 11, 1, METAL_LO);
    }
}

/// Right side piece: inner dark edge, body, bevel, outer dark edge.
fn side_right(c: &mut Canvas, x: i32, y: i32, h: i32, bottom: bool) {
    c.fill(x, y, 8, h, BODY_FLAT);
    c.fill(x, y, 1, h, METAL_LO);
    c.fill(x + 6, y, 1, h, METAL_HI);
    c.fill(x + 7, y, 1, h, METAL_LO);
    if bottom {
        c.fill(x, y + h - 1, 8, 1, METAL_LO);
    }
}
