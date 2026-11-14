//! BALANCE.BMP (68×433): 28 frames, 15 px apart, each a 38 px groove (at x 9)
//! whose fill grows outward from the centre with the absolute balance, plus thumbs.
//! Frame 0 is centred (neutral); the edges are the warmest colours.

use crate::paint::{bevel_sunken, ramp, slider_thumb, Canvas, GROOVE, SILVER_BOT, VU_STOPS};

/// Frames in the sprite sheet.
const FRAMES: i32 = 28;
/// Vertical distance between frames.
const STRIDE: i32 = 15;
/// Left x of the 38 px groove inside the sheet.
const GROOVE_X: i32 = 9;
/// Fill length per side at full balance (px).
const ARM_MAX: i32 = 17;

pub fn render() -> Canvas {
    let mut c = Canvas::new(68, 433);
    for k in 0..FRAMES {
        frame(&mut c, k * STRIDE, arm_len(k));
    }
    slider_thumb(&mut c, 15, 422, false); // MAIN_BALANCE_THUMB
    slider_thumb(&mut c, 0, 422, true); // MAIN_BALANCE_THUMB_ACTIVE
    c
}

/// Fill length on each side of the centre for frame `k` (0 = centred, 27 = edge).
fn arm_len(k: i32) -> i32 {
    (k * ARM_MAX + 13) / 27
}

/// One 38×13 frame at row `y`.
fn frame(c: &mut Canvas, y: i32, arm: i32) {
    let x0 = GROOVE_X;
    bevel_sunken(c, x0, y, 38, 13);
    c.fill(x0 + 1, y + 1, 36, 11, GROOVE);
    for d in 0..arm {
        if d % 4 == 3 {
            continue; // segment gaps
        }
        let colour = ramp(d as f32 / (ARM_MAX - 1) as f32, &VU_STOPS);
        c.fill(x0 + 17 - d, y + 3, 1, 7, colour); // left arm
        c.fill(x0 + 20 + d, y + 3, 1, 7, colour); // right arm
    }
    // Neutral centre tick, always visible.
    c.fill(x0 + 18, y + 2, 2, 9, SILVER_BOT);
}
