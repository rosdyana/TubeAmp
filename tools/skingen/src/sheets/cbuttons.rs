//! CBUTTONS.BMP (136×36): the transport keys, normal on row 0 and pressed below.
//! Each key is a silver raised face with a dark glyph; pressed keys sink.

use crate::paint::{glyph, key, Canvas, INK, SILVER};

/// One transport key: its x, normal width, pressed width, height and glyph.
/// Pressed copies sit directly below the normal copy (`y = height`).
struct Slot {
    x: i32,
    w: i32,
    w_pressed: i32,
    h: i32,
    icon: &'static [&'static str],
}

const PREV: &[&str] = &[
    "##.....#", "##....##", "##...###", "##..####", "##.#####", "##..####", "##...###", "##....##",
    "##.....#",
];
const PLAY: &[&str] = &[
    "#....", "##...", "###..", "####.", "#####", "####.", "###..", "##...", "#....",
];
const PAUSE: &[&str] = &["##.##"; 9];
const STOP: &[&str] = &["#######"; 7];
const NEXT: &[&str] = &[
    "#.....##", "##....##", "###...##", "####..##", "#####.##", "####..##", "###...##", "##....##",
    "#.....##",
];
const EJECT: &[&str] = &[
    "...#...", "..###..", ".#####.", "#######", ".......", "#######", "#######",
];

const SLOTS: [Slot; 6] = [
    Slot {
        x: 0,
        w: 23,
        w_pressed: 23,
        h: 18,
        icon: PREV,
    },
    Slot {
        x: 23,
        w: 23,
        w_pressed: 23,
        h: 18,
        icon: PLAY,
    },
    Slot {
        x: 46,
        w: 23,
        w_pressed: 23,
        h: 18,
        icon: PAUSE,
    },
    Slot {
        x: 69,
        w: 23,
        w_pressed: 23,
        h: 18,
        icon: STOP,
    },
    Slot {
        x: 92,
        w: 23,
        w_pressed: 22,
        h: 18,
        icon: NEXT,
    },
    Slot {
        x: 114,
        w: 22,
        w_pressed: 22,
        h: 16,
        icon: EJECT,
    },
];

pub fn render() -> Canvas {
    let mut c = Canvas::new(136, 36);
    for slot in &SLOTS {
        key_icon(&mut c, slot.x, 0, slot.w, slot.h, slot.icon, false);
        key_icon(
            &mut c,
            slot.x,
            slot.h,
            slot.w_pressed,
            slot.h,
            slot.icon,
            true,
        );
    }
    c
}

/// A silver key with its icon centred. Pressed keys nudge the icon 1 px down-right.
fn key_icon(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, icon: &[&str], pressed: bool) {
    key(c, x, y, w, h, SILVER, pressed);
    let gw = icon.iter().map(|r| r.len() as i32).max().unwrap_or(0);
    let gh = icon.len() as i32;
    let p = i32::from(pressed);
    let gx = x + (w - gw) / 2 + p;
    let gy = y + (h - gh) / 2 + p;
    glyph(c, gx, gy, icon, INK);
}
