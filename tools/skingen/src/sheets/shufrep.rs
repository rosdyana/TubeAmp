//! SHUFREP.BMP (92×85): SHUFFLE, REPEAT, EQ and PL toggles in all four states
//! (off/on × normal/pressed). Each has a small LED that is green when on.

use crate::paint::{steel_button, Canvas};

pub fn render() -> Canvas {
    let mut c = Canvas::new(92, 85);

    // Repeat, 28×15 at x 0: normal, pressed, on, on+pressed (rows 0, 15, 30, 45).
    // The 28 px key is too narrow for "REPEAT" in this font, so it reads "REP".
    for (row, on, pressed) in [
        (0, false, false),
        (15, false, true),
        (30, true, false),
        (45, true, true),
    ] {
        steel_button(&mut c, (0, row, 28, 15), "REP", on, pressed, true);
    }
    // Shuffle, 47×15 at x 28.
    for (row, on, pressed) in [
        (0, false, false),
        (15, false, true),
        (30, true, false),
        (45, true, true),
    ] {
        steel_button(&mut c, (28, row, 47, 15), "SHUFFLE", on, pressed, true);
    }
    // EQ, 23×12 at x 0 (rows 61, 73) and x 46 (depressed, rows 61 and 73).
    steel_button(&mut c, (0, 61, 23, 12), "EQ", false, false, true);
    steel_button(&mut c, (0, 73, 23, 12), "EQ", true, false, true);
    steel_button(&mut c, (46, 61, 23, 12), "EQ", false, true, true);
    steel_button(&mut c, (46, 73, 23, 12), "EQ", true, true, true);
    // PL, 23×12 at x 23 (rows 61, 73) and x 69 (depressed, rows 61 and 73).
    steel_button(&mut c, (23, 61, 23, 12), "PL", false, false, true);
    steel_button(&mut c, (23, 73, 23, 12), "PL", true, false, true);
    steel_button(&mut c, (69, 61, 23, 12), "PL", false, true, true);
    steel_button(&mut c, (69, 73, 23, 12), "PL", true, true, true);
    c
}
