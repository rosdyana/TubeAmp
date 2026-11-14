//! NUMBERS.BMP (99×13) and NUMS_EX.BMP (108×13): seven-segment time digits in LCD
//! green with dim "off" segments.
//!
//! NUMBERS keeps the 5×1 minus pair on the middle row of its digit cells; NUMS_EX
//! has full 9×13 blank-minus and minus cells, as the `_EX` sprite table expects.

use crate::paint::{seven_seg, Canvas, LCD_GHOST, LCD_ON};

/// Lit segment masks for 0–9 (bit 0 = a, 1 = b, … 6 = g).
const DIGITS: [u8; 10] = [63, 6, 91, 79, 102, 109, 125, 7, 127, 111];
/// Every segment.
const ALL: u8 = 127;
/// The middle segment alone (the minus sign).
const MIDDLE: u8 = 64;

pub fn render_numbers() -> Canvas {
    let mut c = Canvas::new(99, 13);
    for (cell, &lit) in DIGITS.iter().enumerate() {
        seven_seg(&mut c, 9 * cell as i32, 0, lit, ALL & !lit);
    }
    // NO_MINUS_SIGN (dim) and MINUS_SIGN (lit): 5×1 bars on the middle row.
    c.fill(9, 6, 5, 1, LCD_GHOST);
    c.fill(20, 6, 5, 1, LCD_ON);
    c
}

pub fn render_nums_ex() -> Canvas {
    let mut c = Canvas::new(108, 13);
    for (cell, &lit) in DIGITS.iter().enumerate() {
        seven_seg(&mut c, 9 * cell as i32, 0, lit, ALL & !lit);
    }
    // NO_MINUS_SIGN_EX: only the dim middle segment. MINUS_SIGN_EX: lit middle segment.
    seven_seg(&mut c, 90, 0, 0, MIDDLE);
    seven_seg(&mut c, 99, 0, MIDDLE, 0);
    c
}
