//! PLEDIT.BMP (280×186): the playlist window frame (title tiles, side and bottom
//! panels, the visualiser well), list buttons, scroll handle and shade strips.

use crate::paint::{
    centred_text, flat_key, hex, key, label, label_width, pip, title_strip, Canvas, Rgb, Strip,
    BODY_BOT, BODY_FLAT, BODY_TOP, ICON_CLOSE, ICON_DOWN, ICON_UP, INK, LCD_BG, LCD_RIM, METAL_HI,
    METAL_LO, SILVER, STEEL_TEXT, TITLE_ACTIVE, TITLE_INACTIVE, WHITE,
};

/// Visualiser grid dot colour.
const VIS_DOT: Rgb = hex(0x10261A);

pub fn render() -> Canvas {
    let mut c = Canvas::new(280, 186);

    // Title row: selected (active) at y 0, normal (inactive) at y 21.
    title_row(&mut c, 0, &TITLE_ACTIVE);
    title_row(&mut c, 21, &TITLE_INACTIVE);

    // Side tiles: left (0,42) 12×29 and right (31,42) 20×29.
    side_tile(&mut c, 0, 42, 12, 29, true);
    side_tile(&mut c, 31, 42, 20, 29, false);

    // Shade-mode strips: selected row at y 42, normal row at y 57.
    title_strip(&mut c, 72, 42, 25, 14, &TITLE_ACTIVE);
    title_strip(&mut c, 99, 42, 50, 14, &TITLE_ACTIVE);
    title_strip(&mut c, 72, 57, 25, 14, &TITLE_INACTIVE);
    title_strip(&mut c, 99, 57, 50, 14, &TITLE_INACTIVE);

    // Close, collapse, expand (all pressed/selected versions).
    pip(&mut c, 52, 42, &ICON_CLOSE, true, INK);
    pip(&mut c, 62, 42, &ICON_DOWN, true, INK);
    pip(&mut c, 150, 42, &ICON_UP, true, INK);

    // Scroll handles 8×18: normal at (52,53), selected at (61,53).
    scroll_handle(&mut c, 52, 53, false);
    scroll_handle(&mut c, 61, 53, true);

    // Visualiser well 75×38 at (205,0).
    visualiser(&mut c, 205, 0);

    // Bottom panels: bottom tile (179,0) 25×38, bottom-left (0,72) 125×38, bottom-right (126,72) 150×38.
    bottom_panel(&mut c, 179, 0, 25, 38);
    bottom_panel(&mut c, 0, 72, 125, 38);
    c.fill(0, 72, 1, 38, METAL_LO);
    bottom_panel(&mut c, 126, 72, 150, 38);
    c.fill(275, 72, 1, 38, METAL_LO);

    // Menu separators (3 px wide).
    menu_bar(&mut c, 48, 111, 54);
    menu_bar(&mut c, 100, 111, 72);
    menu_bar(&mut c, 150, 111, 54);
    menu_bar(&mut c, 200, 111, 54);
    menu_bar(&mut c, 250, 111, 54);

    // List buttons: normal at (x, y), selected at (x + 23, y), 22×18.
    for (x, y, text) in [
        (0, 111, "URL"),
        (0, 130, "DIR"),
        (0, 149, "FILE"),
        (54, 111, "ALL"),
        (54, 130, "CROP"),
        (54, 149, "SEL"),
        (54, 168, "MISC"),
        (104, 111, "INV"),
        (104, 130, "CLR"),
        (104, 149, "ALL"),
        (154, 111, "SORT"),
        (154, 130, "INFO"),
        (154, 149, "OPT"),
        (204, 111, "NEW"),
        (204, 130, "SAVE"),
        (204, 149, "LOAD"),
    ] {
        list_button(&mut c, x, y, text);
    }
    c
}

/// The title row: corners, the 100 px title tile with "PLAYLIST", and the fills.
fn title_row(c: &mut Canvas, y: i32, s: &Strip) {
    title_strip(c, 0, y, 25, 20, s); // top-left corner
    c.fill(0, y, 1, 20, METAL_LO);
    title_strip(c, 26, y, 100, 20, s); // title tile
    centred_text(c, 26, y, 100, 20, "PLAYLIST", s);
    title_strip(c, 127, y, 25, 20, s); // top fill
    title_strip(c, 153, y, 25, 20, s); // top-right corner
    c.fill(177, y, 1, 20, METAL_LO);
}

/// A 1 px-edged side tile. `outer_left` puts the dark outer edge on the left.
fn side_tile(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, outer_left: bool) {
    c.fill(x, y, w, h, BODY_FLAT);
    if outer_left {
        c.fill(x, y, 1, h, METAL_LO);
        c.fill(x + 1, y, 1, h, METAL_HI);
        c.fill(x + w - 1, y, 1, h, METAL_LO);
    } else {
        c.fill(x, y, 1, h, METAL_LO);
        c.fill(x + w - 1, y, 1, h, METAL_LO);
    }
}

/// A silver 8×18 scroll handle with three grips.
fn scroll_handle(c: &mut Canvas, x: i32, y: i32, pressed: bool) {
    key(c, x, y, 8, 18, SILVER, pressed);
    for gy in [6, 9, 12] {
        c.fill(x + 2, y + gy, 4, 1, INK);
    }
}

/// The visualiser well: LCD black with a faint dot grid and a dark rim.
fn visualiser(c: &mut Canvas, x: i32, y: i32) {
    c.fill(x, y, 75, 38, LCD_BG);
    for dy in (2..38).step_by(4) {
        for dx in (2..75).step_by(4) {
            c.set(x + dx, y + dy, VIS_DOT);
        }
    }
    c.frame(x, y, 75, 38, LCD_RIM);
}

/// A bottom frame panel: gradient body with a highlight on top and a shadow below.
fn bottom_panel(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
    c.vgrad(x, y + 1, w, h - 2, BODY_TOP, BODY_BOT);
    c.fill(x, y, w, 1, METAL_HI);
    c.fill(x, y + h - 1, w, 1, METAL_LO);
}

/// A 3 px popup-menu separator.
fn menu_bar(c: &mut Canvas, x: i32, y: i32, h: i32) {
    c.fill(x, y, 1, h, METAL_HI);
    c.fill(x + 1, y, 1, h, BODY_TOP);
    c.fill(x + 2, y, 1, h, METAL_LO);
}

/// A 22×18 labelled button at `(x, y)` (normal) and `(x + 23, y)` (selected).
fn list_button(c: &mut Canvas, x: i32, y: i32, text: &str) {
    let tw = label_width(text, 1, false);
    let ty = y + (18 - 5) / 2;
    flat_key(c, x, y, 22, 18, false);
    label(c, x + (22 - tw) / 2, ty, text, STEEL_TEXT, 1, false);
    flat_key(c, x + 23, y, 22, 18, true);
    label(c, x + 23 + (22 - tw) / 2, ty + 1, text, WHITE, 1, false);
}
