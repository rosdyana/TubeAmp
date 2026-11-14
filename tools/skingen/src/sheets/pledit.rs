//! PLEDIT.BMP (280×186): the playlist window art. The bottom corners carry the
//! bottom buttons (ADD, REM, SEL, MISC, LIST), the running-time and mini-time LCD
//! wells, the mini transport and the resize grip, painted where the window's CSS
//! offsets put them. The popups have normal and selected item faces with their bars.

use crate::paint::{
    centred_text, flat_key, glyph, hex, key, label, label_width, pip, recess, steel_button,
    title_strip, Canvas, Rgb, Strip, BODY_BOT, BODY_FLAT, BODY_TOP, ICON_CLOSE, ICON_DOWN, ICON_UP,
    INK, LCD_BG, LCD_RIM, METAL_HI, METAL_LO, SILVER, SILVER_TOP, STEEL, STEEL_TEXT, TITLE_ACTIVE,
    TITLE_INACTIVE, WHITE,
};

/// Visualiser grid dot colour.
const VIS_DOT: Rgb = hex(0x10261A);
/// Selected popup item: a steel-blue highlight.
const HILITE_TOP: Rgb = hex(0x3A5DAE);
const HILITE_BOT: Rgb = hex(0x1E3A8A);

/// Mini transport icons, 6 px wide, drawn on 10×10 keys: previous, play, pause, stop,
/// next, eject.
const PREVIOUS_ICON: [&str; 7] = ["##...#", "##..##", "##.###", "######", "##.###", "##..##", "##...#"];
const PLAY_ICON: [&str; 7] = ["#.....", "##....", "###...", "####..", "###...", "##....", "#....."];
const PAUSE_ICON: [&str; 7] = ["##.##.", "##.##.", "##.##.", "##.##.", "##.##.", "##.##.", "##.##."];
const STOP_ICON: [&str; 7] = ["#####.", "#####.", "#####.", "#####.", "#####.", "#####.", "#####."];
const NEXT_ICON: [&str; 7] = ["#...##", "##..##", "###.##", "######", "###.##", "##..##", "#...##"];
const EJECT_ICON: [&str; 6] = ["..##..", ".####.", "######", "......", "######", "######"];
const TRANSPORT_ICONS: [&[&str]; 6] = [
    &PREVIOUS_ICON,
    &PLAY_ICON,
    &PAUSE_ICON,
    &STOP_ICON,
    &NEXT_ICON,
    &EJECT_ICON,
];

/// The bottom-left and bottom-right corner panels (sheet origins (0,72) and (126,72)).
const CORNER_LEFT: (i32, i32) = (0, 72);
const CORNER_RIGHT: (i32, i32) = (126, 72);

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

    // Close, collapse, expand (pressed/selected versions).
    pip(&mut c, 52, 42, &ICON_CLOSE, true, INK);
    pip(&mut c, 62, 42, &ICON_DOWN, true, INK);
    pip(&mut c, 150, 42, &ICON_UP, true, INK);

    // Scroll handles 8×18: normal at (52,53), selected at (61,53).
    scroll_handle(&mut c, 52, 53, false);
    scroll_handle(&mut c, 61, 53, true);

    // Visualiser well 75×38 at (205,0).
    visualiser(&mut c, 205, 0);

    // Bottom panels: bottom tile (179,0) 25×38, then the two corners.
    bottom_panel(&mut c, 179, 0, 25, 38);
    bottom_left_corner(&mut c);
    bottom_right_corner(&mut c);

    // Menu separators (3 px wide).
    menu_bar(&mut c, 48, 111, 54);
    menu_bar(&mut c, 100, 111, 72);
    menu_bar(&mut c, 150, 111, 54);
    menu_bar(&mut c, 200, 111, 54);
    menu_bar(&mut c, 250, 111, 54);

    // Popup items, 22×18: normal at (x, y), selected at (x + 23, y).
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
        menu_item(&mut c, x, y, text, false);
        menu_item(&mut c, x + 23, y, text, true);
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

/// Bottom-left corner (125×38): the four popup buttons ADD, REM, SEL and MISC, 22×18
/// each at 14, 43, 72 and 101 px, 8 px above the bottom edge (Webamp's `bottom: 12px`).
fn bottom_left_corner(c: &mut Canvas) {
    let (ox, oy) = CORNER_LEFT;
    bottom_panel(c, ox, oy, 125, 38);
    c.fill(ox, oy, 1, 38, METAL_LO);
    for (x, text) in [(14, "ADD"), (43, "REM"), (72, "SEL"), (101, "MISC")] {
        steel_button(c, (ox + x, oy + 8, 22, 18), text, false, false, false);
    }
}

/// Bottom-right corner (150×38): the running-time well (7,10), the mini transport (3,22)
/// as six 10×10 keys, the mini-time well (66,23), the LIST button at (106,8) and the
/// resize grip in the bottom-right 20×20.
fn bottom_right_corner(c: &mut Canvas) {
    let (ox, oy) = CORNER_RIGHT;
    bottom_panel(c, ox, oy, 150, 38);
    c.fill(ox + 149, oy, 1, 38, METAL_LO);

    steel_button(c, (ox + 106, oy + 8, 22, 18), "LIST", false, false, false);

    // Running-time well: LCD interior x 7..=64, rows 10..=17 (text rows 10..=15).
    recess(c, ox + 5, oy + 8, 62, 12);
    // Mini-time well: LCD interior x 66..=93, rows 23..=28 (text rows 23..=28).
    recess(c, ox + 64, oy + 21, 32, 10);

    for (i, icon) in TRANSPORT_ICONS.iter().enumerate() {
        transport_key(c, ox + 3 + 10 * i as i32, oy + 22, icon);
    }
    resize_grip(c, ox, oy);
}

/// A 10×10 mini-transport key with its icon centred in silver.
fn transport_key(c: &mut Canvas, x: i32, y: i32, icon: &[&str]) {
    key(c, x, y, 10, 10, STEEL, false);
    let gw = icon.iter().map(|r| r.len() as i32).max().unwrap_or(0);
    let gh = icon.len() as i32;
    glyph(c, x + (10 - gw) / 2, y + (10 - gh) / 2, icon, SILVER_TOP);
}

/// Three diagonal grip lines in the bottom-right 20×20 of the corner.
fn resize_grip(c: &mut Canvas, ox: i32, oy: i32) {
    for k in 0..3 {
        let o = 3 + 3 * k;
        for t in 0..=o {
            c.set(ox + 149 - t, oy + 37 - (o - t), METAL_HI);
        }
    }
}

/// A 3 px popup-menu separator.
fn menu_bar(c: &mut Canvas, x: i32, y: i32, h: i32) {
    c.fill(x, y, 1, h, METAL_HI);
    c.fill(x + 1, y, 1, h, BODY_TOP);
    c.fill(x + 2, y, 1, h, METAL_LO);
}

/// One 22×18 popup item. Normal items are steel buttons; the selected (hovered) item is a
/// steel-blue highlight with white text.
fn menu_item(c: &mut Canvas, x: i32, y: i32, text: &str, selected: bool) {
    let tw = label_width(text, 1, false);
    let ty = y + (18 - 5) / 2;
    if !selected {
        flat_key(c, x, y, 22, 18, false);
        label(c, x + (22 - tw) / 2, ty, text, STEEL_TEXT, 1, false);
        return;
    }
    c.frame(x, y, 22, 18, METAL_LO);
    c.vgrad(x + 1, y + 1, 20, 16, HILITE_TOP, HILITE_BOT);
    label(c, x + (22 - tw) / 2, ty, text, WHITE, 1, false);
}
