//! EQ_EX.BMP (275×82): shade-mode equaliser strips, the shade volume and balance
//! slider pieces, and the small window pips.

use crate::paint::{
    pip, wordmark_bar, Canvas, ICON_BOX, ICON_CLOSE, ICON_MIN, INK, SILVER_BOT, SILVER_HI,
    SILVER_LO, SILVER_TOP, TITLE_ACTIVE, TITLE_INACTIVE,
};

pub fn render() -> Canvas {
    let mut c = Canvas::new(275, 82);

    // Shade strips: selected at (0,0), normal at (0,15). No text: the app draws it.
    wordmark_bar(&mut c, 0, 0, 275, 14, &TITLE_ACTIVE, None);
    wordmark_bar(&mut c, 0, 15, 275, 14, &TITLE_INACTIVE, None);

    // Shade volume slider pieces (left, centre, right) at x 1, 4, 7 and balance at 11, 14, 17.
    for x in [1, 11] {
        slider_piece(&mut c, x, 30, true, false);
        slider_piece(&mut c, x + 3, 30, false, false);
        slider_piece(&mut c, x + 6, 30, false, true);
    }

    // Pips: maximise (1,38), minimise (1,47), shade close (11,38) and its pressed copy (11,47).
    pip(&mut c, 1, 38, &ICON_BOX, false, INK);
    pip(&mut c, 1, 47, &ICON_MIN, false, INK);
    pip(&mut c, 11, 38, &ICON_CLOSE, false, INK);
    pip(&mut c, 11, 47, &ICON_CLOSE, true, INK);
    c
}

/// A 3×7 slider piece: silver with a highlight and shadow line, and an ink edge on
/// its outer side (left piece = left edge, right piece = right edge).
fn slider_piece(c: &mut Canvas, x: i32, y: i32, left_edge: bool, right_edge: bool) {
    c.vgrad(x, y, 3, 7, SILVER_TOP, SILVER_BOT);
    c.fill(x, y, 3, 1, SILVER_HI);
    c.fill(x, y + 6, 3, 1, SILVER_LO);
    if left_edge {
        c.fill(x, y, 1, 7, INK);
    }
    if right_edge {
        c.fill(x + 2, y, 1, 7, INK);
    }
}
