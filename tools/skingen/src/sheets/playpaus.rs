//! PLAYPAUS.BMP (42×9): play/pause/stop indicators on transparency, plus the
//! working / not-working bars (3 px wide each, at x 36 and x 39).

use crate::paint::{glyph, Canvas, LCD_OFF, LCD_ON};

const PLAYING: [&str; 9] = [
    "..#......",
    "..##.....",
    "..###....",
    "..####...",
    "..#####..",
    "..####...",
    "..###....",
    "..##.....",
    "..#......",
];
const PAUSED: [&str; 9] = [
    ".........",
    "..##.##..",
    "..##.##..",
    "..##.##..",
    "..##.##..",
    "..##.##..",
    "..##.##..",
    "..##.##..",
    ".........",
];
const STOPPED: [&str; 9] = [
    ".........",
    ".........",
    "..#####..",
    "..#####..",
    "..#####..",
    "..#####..",
    "..#####..",
    ".........",
    ".........",
];

pub fn render() -> Canvas {
    let mut c = Canvas::new(42, 9);
    glyph(&mut c, 0, 0, &PLAYING, LCD_ON);
    glyph(&mut c, 9, 0, &PAUSED, LCD_ON);
    glyph(&mut c, 18, 0, &STOPPED, LCD_ON);
    // NOT_WORKING (dim) and WORKING (lit) bars.
    c.fill(36, 0, 3, 9, LCD_OFF);
    c.fill(39, 0, 3, 9, LCD_ON);
    c
}
