//! POSBAR.BMP (307×10): the seek track (248 px) and the silver thumb in normal
//! (x 248) and pressed (x 278) states.

use crate::paint::{hex, key, Canvas, GROOVE, INK, METAL_HI, METAL_LO, SILVER, SILVER_HI};

pub fn render() -> Canvas {
    let mut c = Canvas::new(307, 10);

    // Track: sunken 248×10 groove with a centre line.
    c.bevel(0, 0, 248, 10, METAL_LO, METAL_HI);
    c.vgrad(1, 1, 246, 8, GROOVE, hex(0x151A28));
    c.fill(2, 4, 244, 1, hex(0x2A3550));

    thumb(&mut c, 248, false);
    thumb(&mut c, 278, true);
    c
}

/// A 29×10 silver thumb with three grooves.
fn thumb(c: &mut Canvas, x: i32, pressed: bool) {
    key(c, x, 0, 29, 10, SILVER, pressed);
    for gx in [11, 14, 17] {
        c.fill(x + gx, 3, 1, 5, INK);
        c.fill(x + gx + 1, 3, 1, 5, SILVER_HI);
    }
}
