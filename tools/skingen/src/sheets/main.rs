//! MAIN.BMP (275×116): the window body and its bevelled frame, the single LCD
//! panel that holds the play indicator, the time digits and the visualiser, the
//! marquee and bitrate wells, the time colon, and the static "kbps" / "kHz" labels.

use crate::paint::{
    brush, label, recess, Canvas, BODY_BOT, BODY_TOP, LCD_ON, METAL_HI, METAL_LO, SILVER_TOP,
};

pub fn render() -> Canvas {
    let mut c = Canvas::new(275, 116);
    c.vgrad(0, 0, 275, 116, BODY_TOP, BODY_BOT);
    brush(&mut c, 0, 0, 275, 116, 0x4D41_0001, 2);
    c.bevel(0, 0, 275, 116, METAL_HI, METAL_LO);

    // Groove between the title bar and the body.
    c.fill(1, 14, 273, 1, METAL_LO);
    c.fill(1, 15, 273, 1, METAL_HI);

    // One LCD panel: x 22..=101, y 22..=60. Its interior (24..=99 × 24..=58) holds the
    // play indicator (26,28), the time digits (48..=99 × 26..=38) and the visualiser
    // (24,43, 76×16).
    recess(&mut c, 22, 22, 80, 39);

    // Colon between the minute and second digits: the gap is x 69..=77, centred on 73.
    c.fill(72, 30, 2, 2, LCD_ON);
    c.fill(72, 35, 2, 2, LCD_ON);

    // Marquee well (111,24 154×6 text area) and the bitrate and sample-rate wells.
    recess(&mut c, 109, 22, 158, 14);
    recess(&mut c, 109, 41, 19, 10);
    recess(&mut c, 154, 41, 14, 10);

    // Static unit labels, just right of each readout well, vertically centred on it.
    label(&mut c, 130, 43, "kbps", SILVER_TOP, 1, false);
    label(&mut c, 170, 43, "kHz", SILVER_TOP, 1, false);
    c
}
