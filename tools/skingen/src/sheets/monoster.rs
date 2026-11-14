//! MONOSTER.BMP (58×24): "stereo" and "mono" labels on small LCD plates.
//! Top row (y 0) is the lit state, bottom row (y 12) the unlit state.

use crate::paint::{label, label_width, Canvas, Rgb, LCD_BG, LCD_OFF, LCD_ON, LCD_RIM};

pub fn render() -> Canvas {
    let mut c = Canvas::new(58, 24);
    plate(&mut c, 0, 12, 29, "stereo", LCD_OFF); // STEREO
    plate(&mut c, 0, 0, 29, "stereo", LCD_ON); // STEREO_SELECTED
    plate(&mut c, 29, 12, 27, "mono", LCD_OFF); // MONO
    plate(&mut c, 29, 0, 27, "mono", LCD_ON); // MONO_SELECTED
    c
}

/// A 12-px-tall label plate with `text` centred, in `ink`.
fn plate(c: &mut Canvas, x: i32, y: i32, w: i32, text: &str, ink: Rgb) {
    c.fill(x, y, w, 12, LCD_BG);
    c.frame(x, y, w, 12, LCD_RIM);
    let tw = label_width(text, 1, false);
    label(c, x + (w - tw) / 2, y + 3, text, ink, 1, false);
}
