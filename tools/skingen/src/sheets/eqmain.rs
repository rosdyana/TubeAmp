//! EQMAIN.BMP (275×315): the equaliser window background, title bars, buttons,
//! the 28 slider frames (14×63 cells on a 15 px by 65 px grid), the thumbs, the
//! response graph and the preamp line.

use crate::paint::{
    brush, hex, key, label, label_width, pip, ramp, recess, steel_button, wordmark_bar, Canvas,
    Rgb, BODY_BOT, BODY_TOP, GROOVE, ICON_BOX, ICON_CLOSE, INK, LCD_BG, LCD_OFF, METAL_HI,
    METAL_LO, SILVER, SILVER_TOP, TITLE_ACTIVE, TITLE_INACTIVE,
};

/// Slider colours from the handle outwards: red at the top, green at the bottom.
const EQ_STOPS: [(f32, Rgb); 3] = [
    (0.0, hex(0xE6321E)),
    (0.5, hex(0xE6E61E)),
    (1.0, hex(0x1EE65A)),
];

/// Graph curve colours: red at the top, green at the bottom.
const GRAPH_STOPS: [(f32, Rgb); 3] = [
    (0.0, hex(0xE6321E)),
    (0.5, hex(0xE6E61E)),
    (1.0, hex(0x1EE65A)),
];

pub fn render() -> Canvas {
    let mut c = Canvas::new(275, 315);

    // Window body (0,0) 275×116.
    c.vgrad(0, 0, 275, 116, BODY_TOP, BODY_BOT);
    brush(&mut c, 0, 0, 275, 116, 0x4551_0002, 2);
    c.bevel(0, 0, 275, 116, METAL_HI, METAL_LO);
    // Graph well behind the 113×19 curve, and the sunken band panel.
    recess(&mut c, 84, 15, 117, 23);
    recess(&mut c, 16, 34, 243, 72);
    // dB scale labels, centred in the 22 px column at x 45.
    for (text, y) in [("+12", 37), ("0", 65), ("-12", 96)] {
        let tw = label_width(text, 1, false);
        label(&mut c, 45 + (22 - tw) / 2, y, text, SILVER_TOP, 1, false);
    }

    // Title bars (selected at y 134, normal at y 149).
    wordmark_bar(&mut c, 0, 134, 275, 14, &TITLE_ACTIVE, Some("EQUALIZER"));
    wordmark_bar(&mut c, 0, 149, 275, 14, &TITLE_INACTIVE, Some("EQUALIZER"));
    // The fallback maximise pip sits inside the title bar at (254,152).
    pip(&mut c, 254, 152, &ICON_BOX, false, INK);

    // Close pip (normal at y 116, pressed at y 125).
    pip(&mut c, 0, 116, &ICON_CLOSE, false, INK);
    pip(&mut c, 0, 125, &ICON_CLOSE, true, INK);

    // ON and AUTO toggles (on = green label), and PRESETS.
    steel_button(&mut c, (10, 119, 26, 12), "ON", false, false, false);
    steel_button(&mut c, (128, 119, 26, 12), "ON", false, true, false);
    steel_button(&mut c, (69, 119, 26, 12), "ON", true, false, false);
    steel_button(&mut c, (187, 119, 26, 12), "ON", true, true, false);
    steel_button(&mut c, (36, 119, 32, 12), "AUTO", false, false, false);
    steel_button(&mut c, (154, 119, 32, 12), "AUTO", false, true, false);
    steel_button(&mut c, (95, 119, 32, 12), "AUTO", true, false, false);
    steel_button(&mut c, (213, 119, 32, 12), "AUTO", true, true, false);
    steel_button(&mut c, (224, 164, 44, 12), "PRESETS", false, false, false);
    steel_button(&mut c, (224, 176, 44, 12), "PRESETS", false, true, false);

    // Slider thumbs 11×11: normal at (0,164), selected at (0,176).
    eq_thumb(&mut c, 0, 164, false);
    eq_thumb(&mut c, 0, 176, true);

    // 28 slider frames: 14 across (15 px apart) and 2 rows (65 px apart).
    for n in 0..28 {
        let (col, row) = (n % 14, n / 14);
        slider_frame(&mut c, 13 + 15 * col, 164 + 65 * row, n);
    }

    // Graph background (0,294) 113×19, its line colour column (115,294), preamp line (0,314).
    graph_background(&mut c);
    for r in 0..19 {
        c.set(115, 294 + r, ramp(r as f32 / 18.0, &GRAPH_STOPS));
    }
    c.fill(0, 314, 113, 1, hex(0x2E4A6A));
    c
}

/// An 11×11 EQ slider thumb with a centre notch.
fn eq_thumb(c: &mut Canvas, x: i32, y: i32, pressed: bool) {
    key(c, x, y, 11, 11, SILVER, pressed);
    c.fill(x + 2, y + 5, 7, 1, INK);
}

/// One 14×63 slider frame at `(fx, fy)` for value index `n` (0 = minimum, 27 = maximum).
///
/// The handle's top row is `(27 - n) * 51 / 27`. The bar runs from the centre row
/// (31) to the handle's centre (top + 5).
fn slider_frame(c: &mut Canvas, fx: i32, fy: i32, n: i32) {
    c.fill(fx, fy, 14, 63, GROOVE);
    c.fill(fx + 6, fy, 2, 63, LCD_BG);
    let top = ((27 - n) * 51 + 13) / 27;
    let handle_mid = top + 5;
    let centre = 31;
    let (lo, hi) = if handle_mid < centre {
        (handle_mid, centre)
    } else {
        (centre, handle_mid)
    };
    for r in lo..=hi {
        c.fill(fx + 5, fy + r, 4, 1, ramp(r as f32 / 62.0, &EQ_STOPS));
    }
}

/// The 113×19 response graph: dark grid with dotted lines.
fn graph_background(c: &mut Canvas) {
    c.fill(0, 294, 113, 19, LCD_BG);
    for row in [4, 9, 14] {
        for x in (0..113).step_by(3) {
            c.set(x, 294 + row, LCD_OFF);
        }
    }
}
