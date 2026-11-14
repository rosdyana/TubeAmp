//! TITLEBAR.BMP (344×87): the main title bars, window pips, the clutter bar and
//! the shade-mode strip with its position slider.

use crate::font;
use crate::paint::{
    flat_key, glyph, hex, pip, wordmark_bar, Canvas, BODY_BOT, BODY_TOP, GROOVE, ICON_CLOSE,
    ICON_MENU, ICON_MIN, ICON_SHADE, INK, LCD_ON, METAL_LO, SILVER_BOT, SILVER_HI, SILVER_LO,
    SILVER_TOP, TITLE_ACTIVE, TITLE_BRONZE_ACTIVE, TITLE_BRONZE_INACTIVE, TITLE_INACTIVE,
};

pub fn render() -> Canvas {
    let mut c = Canvas::new(344, 87);

    // Main title bars: active at (27,0), inactive at (27,15); easter-egg pair below.
    wordmark_bar(&mut c, 27, 0, 275, 14, &TITLE_ACTIVE, Some("TUBEAMP"));
    wordmark_bar(&mut c, 27, 15, 275, 14, &TITLE_INACTIVE, Some("TUBEAMP"));
    wordmark_bar(
        &mut c,
        27,
        57,
        275,
        14,
        &TITLE_BRONZE_ACTIVE,
        Some("TUBEAMP"),
    );
    wordmark_bar(
        &mut c,
        27,
        72,
        275,
        14,
        &TITLE_BRONZE_INACTIVE,
        Some("TUBEAMP"),
    );

    // Window pips: options, minimise, close (rows 0 and 9 = normal and pressed),
    // then shade (rows 18 and 9 pressed), and the lit shade-mode pair on row 27.
    pip(&mut c, 0, 0, &ICON_MENU, false, INK);
    pip(&mut c, 0, 9, &ICON_MENU, true, INK);
    pip(&mut c, 9, 0, &ICON_MIN, false, INK);
    pip(&mut c, 9, 9, &ICON_MIN, true, INK);
    pip(&mut c, 18, 0, &ICON_CLOSE, false, INK);
    pip(&mut c, 18, 9, &ICON_CLOSE, true, INK);
    pip(&mut c, 0, 18, &ICON_SHADE, false, INK);
    pip(&mut c, 9, 18, &ICON_SHADE, true, INK);
    pip(&mut c, 0, 27, &ICON_SHADE, false, LCD_ON);
    pip(&mut c, 9, 27, &ICON_SHADE, true, LCD_ON);

    // Shade-mode strips: selected at (27,29), normal at (27,42).
    wordmark_bar(&mut c, 27, 29, 275, 14, &TITLE_ACTIVE, None);
    wordmark_bar(&mut c, 27, 42, 275, 14, &TITLE_INACTIVE, None);

    // Shade position track (0,36) 17×7 and its three thumb pieces.
    c.frame(0, 36, 17, 7, METAL_LO);
    c.vgrad(1, 37, 15, 5, GROOVE, hex(0x1A2030));
    c.fill(8, 38, 1, 3, SILVER_BOT);
    shade_thumb(&mut c, 17, 36, true, false);
    shade_thumb(&mut c, 20, 36, false, false);
    shade_thumb(&mut c, 23, 36, false, true);

    // Clutter bar background (8×43) at (304,0), disabled copy at (312,0).
    clutter_background(&mut c, 304, true);
    clutter_background(&mut c, 312, false);
    // Lit (selected) keys: O, A, I, D, V.
    lit_key(&mut c, 304, 47, 8, 8, 'o');
    lit_key(&mut c, 312, 55, 8, 7, 'a');
    lit_key(&mut c, 320, 62, 8, 7, 'i');
    lit_key(&mut c, 328, 69, 8, 8, 'd');
    lit_key(&mut c, 336, 77, 8, 7, 'v');
    c
}

/// One 3×7 piece of the shade-mode thumb: silver with a highlight and shadow line,
/// and an ink outline on whichever outer edge it owns.
fn shade_thumb(c: &mut Canvas, x: i32, y: i32, left_edge: bool, right_edge: bool) {
    c.vgrad(x, y, 3, 7, SILVER_TOP, SILVER_BOT);
    c.fill(x, y, 3, 1, SILVER_HI);
    c.fill(x, y + 6, 3, 1, SILVER_LO);
    if left_edge {
        c.fill(x, y, 1, 7, INK);
    }
    if right_edge {
        c.fill(x + 2, y, 1, 7, INK);
    }
}

/// The clutter bar strip: a dark groove with the five letter keys, drawn dim.
fn clutter_background(c: &mut Canvas, x: i32, enabled: bool) {
    c.vgrad(x, 0, 8, 43, BODY_TOP, BODY_BOT);
    c.frame(x, 0, 8, 43, METAL_LO);
    let ink = if enabled { SILVER_TOP } else { SILVER_LO };
    for (letter, y, h) in [
        ('o', 3, 8),
        ('a', 11, 7),
        ('i', 18, 7),
        ('d', 25, 8),
        ('v', 33, 7),
    ] {
        flat_key(c, x, y, 8, h, false);
        glyph(c, x + 2, y + (h - 5) / 2, &font::glyph(letter), ink);
    }
}

/// A lit (pressed) clutter key: silver face with a dark letter.
fn lit_key(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, letter: char) {
    c.frame(x, y, w, h, INK);
    c.vgrad(x + 1, y + 1, w - 2, h - 2, SILVER_TOP, SILVER_BOT);
    glyph(c, x + 2, y + (h - 5) / 2, &font::glyph(letter), INK);
}
