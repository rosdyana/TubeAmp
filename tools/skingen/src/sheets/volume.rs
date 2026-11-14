//! VOLUME.BMP (68×433): 28 background frames, 15 px apart (13 px used), whose
//! fill grows dark → green → yellow → red as the value rises, plus thumbs.

use crate::paint::{bevel_sunken, ramp, slider_thumb, Canvas, GROOVE, VU_STOPS};

/// Frames in the sprite sheet.
const FRAMES: i32 = 28;
/// Vertical distance between frames.
const STRIDE: i32 = 15;
/// Fill width (px) at full volume.
const FILL_MAX: i32 = 62;

pub fn render() -> Canvas {
    let mut c = Canvas::new(68, 433);
    for k in 0..FRAMES {
        frame(&mut c, k * STRIDE, fill_len(k));
    }
    slider_thumb(&mut c, 15, 422, false); // MAIN_VOLUME_THUMB
    slider_thumb(&mut c, 0, 422, true); // MAIN_VOLUME_THUMB_SELECTED
    c
}

/// Lit width for frame `k` (0 = empty, 27 = full).
fn fill_len(k: i32) -> i32 {
    (k * FILL_MAX + 13) / 27
}

/// One 68×13 frame at row `y` with `len` px of segmented fill.
fn frame(c: &mut Canvas, y: i32, len: i32) {
    bevel_sunken(c, 0, y, 68, 13);
    c.fill(1, y + 1, 66, 11, GROOVE);
    for x in 0..len {
        if x % 4 == 3 {
            continue; // segment gaps
        }
        c.fill(
            3 + x,
            y + 3,
            1,
            7,
            ramp(x as f32 / (FILL_MAX - 1) as f32, &VU_STOPS),
        );
    }
}
