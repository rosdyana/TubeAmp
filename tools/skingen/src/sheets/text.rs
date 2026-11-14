//! TEXT.BMP (155×74): the 5×6 LCD font. Every cell in the loader's glyph table is
//! LCD green on LCD black; unused cells stay transparent.

use crate::font;
use crate::paint::{glyph, Canvas, LCD_BG, LCD_ON};

pub fn render() -> Canvas {
    let mut c = Canvas::new(155, 74);
    let mut drawn: Vec<(u32, u32)> = Vec::new();
    for &(ch, (row, col)) in font::CELLS {
        // Several characters share a cell (`[`, `<`, `{`); the first one wins.
        if drawn.contains(&(row, col)) {
            continue;
        }
        drawn.push((row, col));
        let (x, y) = (col as i32 * 5, row as i32 * 6);
        c.fill(x, y, 5, 6, LCD_BG);
        glyph(&mut c, x, y, &font::glyph(ch), LCD_ON);
    }
    c
}
