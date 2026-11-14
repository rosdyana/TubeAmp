//! GENEX.BMP (130×75): the colour pixels the classic skin format reads from row 0.
//!
//! Each colour is one pixel at an even x from 48 to 90, in this order: item
//! background, item foreground, window background, button text, window text,
//! divider, playlist selection, list header background, list header text, header
//! frame (top-left, bottom-right, pressed), header dead area, scrollbar (two
//! colours, pressed two colours, dead area), highlighted text and background,
//! selected text and background. The rest of the sheet is transparent.

use crate::paint::{
    hex, Canvas, Rgb, BODY_BOT, BODY_TOP, LCD_BG, LCD_ON, METAL_HI, METAL_LO, SILVER_HI,
    SILVER_TOP, STEEL_BOT, STEEL_TOP, WHITE,
};

const COLOURS: [Rgb; 22] = [
    LCD_BG,        // 48: item background
    LCD_ON,        // 50: item foreground
    BODY_BOT,      // 52: window background
    SILVER_TOP,    // 54: button text
    SILVER_HI,     // 56: window text
    METAL_LO,      // 58: dividers and sunken borders
    hex(0x1E3A8A), // 60: playlist selection
    BODY_TOP,      // 62: list header background
    SILVER_TOP,    // 64: list header text
    METAL_HI,      // 66: header frame, top and left
    METAL_LO,      // 68: header frame, bottom and right
    METAL_LO,      // 70: header frame, pressed
    BODY_BOT,      // 72: header dead area
    STEEL_TOP,     // 74: scrollbar colour 1
    BODY_TOP,      // 76: scrollbar colour 2
    STEEL_BOT,     // 78: pressed scrollbar colour 1
    hex(0x252A3D), // 80: pressed scrollbar colour 2
    BODY_BOT,      // 82: scrollbar dead area
    WHITE,         // 84: highlighted text
    hex(0x1E3A8A), // 86: highlighted background
    LCD_ON,        // 88: selected text
    LCD_BG,        // 90: selected background
];

pub fn render() -> Canvas {
    let mut c = Canvas::new(130, 75);
    for (i, colour) in COLOURS.iter().enumerate() {
        c.set(48 + 2 * i as i32, 0, *colour);
    }
    c
}
