//! EQMAIN.BMP (275×315): the equaliser window. The background carries the
//! recessed LCD panels, the dB ticks and the static labels (PREAMP, band
//! frequencies, +12 / 0 / -12). The 28 slider frames are full-height grooves
//! whose fill runs from the 0 dB centre toward the handle. Also the title bars,
//! the shade pip, toggles, thumbs, the response graph and the preamp line.

use crate::paint::{
    brush, hex, key, label, label_width, pip, ramp, recess, steel_button, wordmark_bar, Canvas,
    Rgb, BODY_BOT, BODY_TOP, GROOVE, ICON_CLOSE, ICON_SHADE, INK, LCD_BG, METAL_HI, METAL_LO,
    SILVER, SILVER_BOT, SILVER_TOP, TITLE_ACTIVE, TITLE_INACTIVE,
};

/// Slider fill colour by distance from the 0 dB centre: green at 0 dB, through yellow
/// and orange, to red at the extremes.
const LEVEL_STOPS: [(f32, Rgb); 4] = [
    (0.0, hex(0x1EE65A)),
    (0.45, hex(0xC8E61E)),
    (0.7, hex(0xFFB020)),
    (1.0, hex(0xF0391E)),
];

/// Response-line colours by graph row: red at +12 dB (top), green at -12 dB (bottom).
const GRAPH_STOPS: [(f32, Rgb); 5] = [
    (0.0, hex(0xFF3B1F)),
    (0.25, hex(0xFF8A1A)),
    (0.5, hex(0xFFE01A)),
    (0.75, hex(0x9BEA2A)),
    (1.0, hex(0x1EE65A)),
];

/// Band frequency labels, left to right.
const BAND_LABELS: [&str; 10] = [
    "60", "170", "310", "600", "1K", "3K", "6K", "12K", "14K", "16K",
];

/// Left edge of band 0 and the pitch between bands (window pixels).
const BAND_X: i32 = 78;
const BAND_PITCH: i32 = 18;
/// Row of the 0 dB centre inside a slider frame.
const CENTRE_ROW: i32 = 31;

pub fn render() -> Canvas {
    let mut c = Canvas::new(275, 315);

    // Window body (0,0) 275×116.
    c.vgrad(0, 0, 275, 116, BODY_TOP, BODY_BOT);
    brush(&mut c, 0, 0, 275, 116, 0x4551_0002, 2);
    c.bevel(0, 0, 275, 116, METAL_HI, METAL_LO);

    // Graph well behind the 113×19 curve.
    recess(&mut c, 84, 15, 117, 23);
    // One recessed panel for the whole slider bank: x 10..=258, y 34..=111.
    recess(&mut c, 10, 34, 249, 78);
    slider_scale(&mut c);

    // Title bars (selected at y 134, normal at y 149). The shade pip sits at window
    // (254,3) in both bars, so the bar carries it whether or not the window has focus.
    wordmark_bar(&mut c, 0, 134, 275, 14, &TITLE_ACTIVE, Some("EQUALIZER"));
    wordmark_bar(&mut c, 0, 149, 275, 14, &TITLE_INACTIVE, Some("EQUALIZER"));
    pip(&mut c, 254, 137, &ICON_SHADE, false, INK); // shade, selected bar
    pip(&mut c, 254, 152, &ICON_SHADE, false, INK); // EQ_MAXIMIZE_BUTTON_ACTIVE_FALLBACK

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

    // Response graph: background, one colour per row, and the preamp line.
    graph_background(&mut c);
    for r in 0..19 {
        c.set(115, 294 + r, ramp(r as f32 / 18.0, &GRAPH_STOPS));
    }
    for x in (0..113).step_by(2) {
        c.set(x, 314, hex(0x4A7AA8));
    }
    c
}

/// dB ticks across the slider bank and the static labels (PREAMP, band frequencies,
/// +12 / 0 / -12). Ticks sit at the handle extremes and the centre.
fn slider_scale(c: &mut Canvas) {
    // Tick rows: +12 (top of travel), 0 (centre of the groove), -12 (bottom of travel).
    let ticks = [(43, "+12"), (69, "0"), (94, "-12")];
    let tick = hex(0x4A5470);
    for (row, text) in ticks {
        for x in (19..43).step_by(4).chain((68..255).step_by(4)) {
            c.fill(x, row, 2, 1, tick);
        }
        let tw = label_width(text, 1, false);
        label(c, 45 + (22 - tw) / 2, row - 2, text, SILVER_TOP, 1, false);
    }
    // PREAMP under the preamp slider (x 21..34, centre 28).
    let pw = label_width("PREAMP", 1, false);
    label(c, 28 - pw / 2, 104, "PREAMP", SILVER_TOP, 1, false);
    // Band frequencies under each slider.
    for (i, text) in BAND_LABELS.iter().enumerate() {
        let centre = BAND_X + BAND_PITCH * i as i32 + 7;
        let tw = label_width(text, 1, false);
        label(c, centre - tw / 2, 104, text, SILVER_TOP, 1, false);
    }
}

/// An 11×11 EQ slider thumb with a centre notch.
fn eq_thumb(c: &mut Canvas, x: i32, y: i32, pressed: bool) {
    key(c, x, y, 11, 11, SILVER, pressed);
    c.fill(x + 2, y + 5, 7, 1, INK);
}

/// One 14×63 slider frame at `(fx, fy)` for value index `n` (0 = minimum, 27 = maximum).
///
/// The track is a full-height channel with a dim tint of the level colours, so every
/// band reads as a continuous track. The bright fill runs from the 0 dB centre (row 31)
/// to the handle centre (`top + 5`, with `top = (27 - n) * 51 / 27`).
fn slider_frame(c: &mut Canvas, fx: i32, fy: i32, n: i32) {
    c.fill(fx, fy, 14, 63, GROOVE);
    // Sunken channel: dark edges (cols 4 and 9) around a 4 px track (cols 5..=8).
    c.fill(fx + 4, fy, 6, 63, METAL_LO);
    c.fill(fx + 5, fy, 4, 63, LCD_BG);
    // Dim full-height tint and centre tick.
    for r in 0..63 {
        let dist = (r - CENTRE_ROW).abs() as f32 / CENTRE_ROW as f32;
        let tint = dim(ramp(dist, &LEVEL_STOPS), 22);
        c.fill(fx + 5, fy + r, 4, 1, tint);
    }
    c.fill(fx + 1, fy + CENTRE_ROW, 3, 1, SILVER_BOT);
    c.fill(fx + 10, fy + CENTRE_ROW, 3, 1, SILVER_BOT);

    // Bright fill between the centre and the handle.
    let top = ((27 - n) * 51 + 13) / 27;
    let handle_mid = top + 5;
    let (lo, hi) = if handle_mid < CENTRE_ROW {
        (handle_mid, CENTRE_ROW)
    } else {
        (CENTRE_ROW, handle_mid)
    };
    for r in lo..=hi {
        let dist = (r - CENTRE_ROW).abs() as f32 / CENTRE_ROW as f32;
        c.fill(fx + 5, fy + r, 4, 1, ramp(dist, &LEVEL_STOPS));
    }
}

/// Scales a colour to `percent` of its brightness.
fn dim(c: Rgb, percent: u32) -> Rgb {
    c.map(|v| (u32::from(v) * percent / 100) as u8)
}

/// The 113×19 response graph: LCD black with a faint vertical grid at each band,
/// a dotted centre (0 dB) line and two dotted reference lines.
fn graph_background(c: &mut Canvas) {
    let grid = hex(0x0C2118);
    let centre = hex(0x1A4A30);
    let reference = hex(0x0A1E14);
    c.fill(0, 294, 113, 19, LCD_BG);
    for i in 0..10 {
        let x = 2 + 12 * i;
        for row in 0..19 {
            c.set(x, 294 + row, grid);
        }
    }
    for x in (0..113).step_by(2) {
        c.set(x, 294 + 9, centre);
    }
    for row in [4, 14] {
        for x in (0..113).step_by(4) {
            c.set(x, 294 + row, reference);
        }
    }
}
