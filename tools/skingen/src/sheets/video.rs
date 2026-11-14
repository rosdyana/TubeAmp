//! VIDEO.BMP (234×119): the classic video window pieces, laid out as `docs/VIDEO-BMP.md`
//! describes. Title pieces at y 0 (focused) and y 21 (unfocused), the sides, the bottom
//! left piece with the five normal buttons and the info well, the bottom fill, the bottom
//! right piece with the resize grip, and the pressed buttons and close.

use crate::paint::{
    centred_text, flat_key, label, label_width, pip, title_strip, Canvas, Strip, BODY_BOT,
    BODY_FLAT, BODY_TOP, ICON_CLOSE, INK, LCD_BG, LCD_RIM, METAL_HI, METAL_LO, STEEL_TEXT,
    TITLE_ACTIVE, TITLE_INACTIVE, WHITE,
};

/// Icons on the five bottom buttons, in classic order: fullscreen, 1×, 2×, TV, options.
const FULLSCREEN: [&str; 7] = [
    "##...##", "#.....#", ".......", ".......", ".......", "#.....#", "##...##",
];
const TV: [&str; 7] = [
    "#######", "#.....#", "#.....#", "#.....#", "#######", "..#.#..", ".#...#.",
];
const OPTIONS: [&str; 5] = ["#######", ".......", "#######", ".......", "#######"];

/// Bottom button x offsets within the left piece (`9 + 15·i`), and the pressed copies.
const BUTTON_X: i32 = 9;
const BUTTON_PITCH: i32 = 15;

pub fn render() -> Canvas {
    let mut c = Canvas::new(234, 119);

    // Title pieces: focused at y 0, unfocused at y 21.
    title_row(&mut c, 0, &TITLE_ACTIVE);
    title_row(&mut c, 21, &TITLE_INACTIVE);

    // Sides: left (127,42) 11×29 and right (139,42) 8×29.
    side_left(&mut c, 127, 42, 29);
    side_right(&mut c, 139, 42, 29);

    // Pressed close at (148,42).
    pip(&mut c, 148, 42, &ICON_CLOSE, true, INK);

    // Bottom left piece (0,42) 125×38: panel, five normal buttons, info well.
    bottom_panel(&mut c, 0, 42, 125, 38, true);
    let icons = [
        Icon::Glyph(&FULLSCREEN),
        Icon::Text("1X"),
        Icon::Text("2X"),
        Icon::Glyph(&TV),
        Icon::Glyph(&OPTIONS),
    ];
    for (i, icon) in icons.into_iter().enumerate() {
        button(&mut c, BUTTON_X + BUTTON_PITCH * i as i32, 51, icon, false);
        button(&mut c, 158 + BUTTON_PITCH * i as i32, 42, icon, true);
    }
    well(&mut c, 92, 124, 52, 67, true, false);

    // Bottom fill (127,81) 25×38 and the well across it.
    bottom_panel(&mut c, 127, 81, 25, 38, false);
    well(&mut c, 127, 151, 91, 106, false, false);

    // Bottom right piece (0,81) 125×38: the well's end, and the resize grip.
    bottom_panel(&mut c, 0, 81, 125, 38, true);
    well(&mut c, 0, 100, 91, 106, false, true);
    resize_grip(&mut c, 124, 118);
    c
}

/// One title row: left corner, filler, title piece with "VIDEO", filler, right corner with
/// the normal close button at its offset (14, 3).
fn title_row(c: &mut Canvas, y: i32, s: &Strip) {
    title_strip(c, 0, y, 25, 20, s);
    c.fill(0, y, 1, 20, METAL_LO); // outer left edge
    title_strip(c, 127, y, 25, 20, s); // filler
    title_strip(c, 26, y, 100, 20, s); // title piece
    centred_text(c, 26, y, 100, 20, "VIDEO", s);
    title_strip(c, 153, y, 25, 20, s); // right corner
    c.fill(177, y, 1, 20, METAL_LO); // outer right edge
    pip(c, 167, y + 3, &ICON_CLOSE, false, INK);
}

/// The 11 px left side piece: outer dark edge, bevel, body, inner dark edge.
fn side_left(c: &mut Canvas, x: i32, y: i32, h: i32) {
    c.fill(x, y, 11, h, BODY_FLAT);
    c.fill(x, y, 1, h, METAL_LO);
    c.fill(x + 1, y, 1, h, METAL_HI);
    c.fill(x + 10, y, 1, h, METAL_LO);
}

/// The 8 px right side piece: inner dark edge, body, bevel, outer dark edge.
fn side_right(c: &mut Canvas, x: i32, y: i32, h: i32) {
    c.fill(x, y, 8, h, BODY_FLAT);
    c.fill(x, y, 1, h, METAL_LO);
    c.fill(x + 6, y, 1, h, METAL_HI);
    c.fill(x + 7, y, 1, h, METAL_LO);
}

/// A bottom panel: gradient body with a highlight on top and a shadow below. `edge` adds a dark
/// outer edge on the left (left piece) or right (right piece) side, chosen by `x`.
fn bottom_panel(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, edge: bool) {
    c.vgrad(x, y + 1, w, h - 2, BODY_TOP, BODY_BOT);
    c.fill(x, y, w, 1, METAL_HI);
    c.fill(x, y + h - 1, w, 1, METAL_LO);
    if edge {
        if x == 0 {
            c.fill(x, y, 1, h, METAL_LO);
        } else {
            c.fill(x + w - 1, y, 1, h, METAL_LO);
        }
    }
}

/// What a bottom button shows: a pixel icon or a short label.
#[derive(Clone, Copy)]
enum Icon {
    Glyph(&'static [&'static str]),
    Text(&'static str),
}

/// A 15×18 flat steel button with its icon or label centred. Pressed buttons show white ink.
fn button(c: &mut Canvas, x: i32, y: i32, icon: Icon, pressed: bool) {
    flat_key(c, x, y, 15, 18, pressed);
    let ink = if pressed { WHITE } else { STEEL_TEXT };
    match icon {
        Icon::Glyph(rows) => {
            let gw = rows.iter().map(|r| r.len() as i32).max().unwrap_or(0);
            let gh = rows.len() as i32;
            let (gx, gy) = (x + (15 - gw) / 2, y + (18 - gh) / 2);
            for (ry, line) in rows.iter().enumerate() {
                for (cx, ch) in line.chars().enumerate() {
                    if ch == '#' {
                        c.set(gx + cx as i32, gy + ry as i32, ink);
                    }
                }
            }
        }
        Icon::Text(text) => {
            let tw = label_width(text, 1, false);
            label(c, x + (15 - tw) / 2, y + (18 - 5) / 2, text, ink, 1, false);
        }
    }
}

/// The info well: a sunken LCD strip from `x0` to `x1` (inclusive) with its top ring at `top`
/// and bottom ring at `bottom`. The text rect sits between the rings. `lcap` and `rcap` close
/// the ends of the well.
fn well(c: &mut Canvas, x0: i32, x1: i32, top: i32, bottom: i32, lcap: bool, rcap: bool) {
    let w = x1 - x0 + 1;
    c.fill(x0, top, w, 1, METAL_LO);
    c.fill(x0, bottom, w, 1, METAL_HI);
    c.fill(x0, top + 1, w, bottom - top - 1, LCD_RIM);
    c.fill(x0, top + 2, w, bottom - top - 3, LCD_BG);
    if lcap {
        c.fill(x0, top, 1, bottom - top + 1, METAL_LO);
    }
    if rcap {
        c.fill(x1, top, 1, bottom - top + 1, METAL_LO);
    }
}

/// Three diagonal grip lines in the bottom-right 20×20 of a piece whose bottom-right pixel is
/// `(x, y)`.
fn resize_grip(c: &mut Canvas, x: i32, y: i32) {
    for k in 0..3 {
        let o = 3 + 3 * k;
        for t in 0..=o {
            c.set(x - t, y - (o - t), METAL_HI);
        }
    }
}
