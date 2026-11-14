//! Shared drawing helpers for the generated sheets.
//!
//! Everything is drawn on a `tiny_skia::Pixmap` with anti-aliasing off so the
//! art stays pixel-crisp. Sheets start out as the transparency key (magenta),
//! so any pixel a renderer does not paint stays transparent in the skin.

use crate::font;
use image::RgbImage;
use tiny_skia::{
    Color, GradientStop, LinearGradient, Paint, Pixmap, Point, PremultipliedColorU8, Rect,
    SpreadMode, Transform,
};

/// An 8-bit RGB triple.
pub type Rgb = [u8; 3];

/// Packs `0xRRGGBB` into an [`Rgb`] at compile time.
pub const fn hex(v: u32) -> Rgb {
    [(v >> 16) as u8, (v >> 8) as u8, v as u8]
}

/// Transparency key (`#FF00FF`) for pixels no sprite uses.
pub const KEY: Rgb = hex(0xFF00FF);

// Body metal and its bevels.
pub const BODY_TOP: Rgb = hex(0x2A2F45);
pub const BODY_BOT: Rgb = hex(0x1E2233);
pub const BODY_FLAT: Rgb = hex(0x262B3F);
pub const METAL_HI: Rgb = hex(0x59607E);
pub const METAL_LO: Rgb = hex(0x0E101A);

// LCD palette.
pub const LCD_BG: Rgb = hex(0x05070A);
pub const LCD_RIM: Rgb = hex(0x1C2333);
pub const LCD_ON: Rgb = hex(0x1EE65A);
pub const LCD_OFF: Rgb = hex(0x0B3A1C);
/// Near-invisible "off" segment colour for seven-segment digits (about 12% of LCD_ON).
pub const LCD_GHOST: Rgb = hex(0x0A2414);
pub const GROOVE: Rgb = hex(0x0A0D14);

// Silver keys (CBUTTONS, thumbs, pips).
pub const SILVER_TOP: Rgb = hex(0xC9CED8);
pub const SILVER_BOT: Rgb = hex(0x9298A8);
pub const SILVER_HI: Rgb = hex(0xE6EAF2);
pub const SILVER_LO: Rgb = hex(0x5E6476);
pub const INK: Rgb = hex(0x1A1D29);

// Steel keys (SHUFREP, EQ and playlist buttons).
pub const STEEL_TOP: Rgb = hex(0x4A5068);
pub const STEEL_BOT: Rgb = hex(0x2E3449);
pub const STEEL_HI: Rgb = hex(0x6E7790);
pub const STEEL_TEXT: Rgb = hex(0xC9CED8);
pub const WHITE: Rgb = hex(0xF4F7FC);

/// Colours of a grooved title strip (title bars, playlist and gen tiles).
#[derive(Clone, Copy)]
pub struct Strip {
    pub top: Rgb,
    pub bottom: Rgb,
    pub hi: Rgb,
    pub lo: Rgb,
    pub rail_dark: Rgb,
    pub rail_light: Rgb,
    pub text: Rgb,
    pub shadow: Rgb,
}

/// Active cool silver-blue title strip.
pub const TITLE_ACTIVE: Strip = Strip {
    top: hex(0xD3DBEA),
    bottom: hex(0x5E6E8E),
    hi: hex(0xF0F4FA),
    lo: hex(0x1A2133),
    rail_dark: hex(0x1A2133),
    rail_light: hex(0xE6EEF8),
    text: WHITE,
    shadow: hex(0x1A2133),
};

/// Inactive (desaturated, darker) title strip.
pub const TITLE_INACTIVE: Strip = Strip {
    top: hex(0x8A929F),
    bottom: hex(0x4A5060),
    hi: hex(0xA8AFBB),
    lo: hex(0x22262F),
    rail_dark: hex(0x22262F),
    rail_light: hex(0x7C8493),
    text: hex(0xB4BAC6),
    shadow: hex(0x22262F),
};

/// Active easter-egg (bronze) title strip.
pub const TITLE_BRONZE_ACTIVE: Strip = Strip {
    top: hex(0xE6CD8E),
    bottom: hex(0x7A5A2A),
    hi: hex(0xFFF0C0),
    lo: hex(0x2A1E0C),
    rail_dark: hex(0x2A1E0C),
    rail_light: hex(0xF0DCA0),
    text: hex(0xFFF4D0),
    shadow: hex(0x2A1E0C),
};

/// Inactive easter-egg (bronze) title strip.
pub const TITLE_BRONZE_INACTIVE: Strip = Strip {
    top: hex(0x9C8E6E),
    bottom: hex(0x4E4330),
    hi: hex(0xB4A584),
    lo: hex(0x22201A),
    rail_dark: hex(0x22201A),
    rail_light: hex(0x8A7E60),
    text: hex(0xC0B494),
    shadow: hex(0x22201A),
};

/// Icons for 9×9 window pips (5×5 ink).
pub const ICON_MENU: [&str; 5] = ["#####", ".....", "#####", ".....", "#####"];
pub const ICON_MIN: [&str; 5] = [".....", ".....", ".....", ".....", "#####"];
pub const ICON_SHADE: [&str; 5] = ["..#..", ".###.", "#####", ".....", "#####"];
pub const ICON_CLOSE: [&str; 5] = ["#...#", ".#.#.", "..#..", ".#.#.", "#...#"];
pub const ICON_BOX: [&str; 5] = ["#####", "#...#", "#...#", "#...#", "#####"];
pub const ICON_DOWN: [&str; 5] = [".....", "#####", ".###.", "..#..", "....."];
pub const ICON_UP: [&str; 5] = [".....", "..#..", ".###.", "#####", "....."];

/// Level-meter ramp for the volume and balance frames (low, mid, high).
pub const VU_STOPS: [(f32, Rgb); 3] = [
    (0.0, hex(0x2BD84F)),
    (0.55, hex(0xD8E81E)),
    (1.0, hex(0xF0391E)),
];

/// A drawing surface backed by a pixmap that starts out as the transparency key.
pub struct Canvas {
    pix: Pixmap,
}

impl Canvas {
    /// A `w`×`h` canvas filled with the transparency key.
    pub fn new(w: u32, h: u32) -> Canvas {
        let mut pix = Pixmap::new(w, h).expect("sheet dimensions are non-zero");
        pix.fill(color(KEY));
        Canvas { pix }
    }

    /// Converts the finished canvas to an RGB image (no alpha: BMP is 24-bit).
    pub fn into_rgb(self) -> RgbImage {
        let (w, h) = (self.pix.width(), self.pix.height());
        let mut buf = Vec::with_capacity((w * h * 3) as usize);
        for p in self.pix.pixels() {
            buf.extend_from_slice(&[p.red(), p.green(), p.blue()]);
        }
        RgbImage::from_raw(w, h, buf).expect("buffer matches the canvas size")
    }

    fn offset(&self, x: i32, y: i32) -> Option<usize> {
        let (w, h) = (self.pix.width() as i32, self.pix.height() as i32);
        (x >= 0 && y >= 0 && x < w && y < h).then(|| (y * w + x) as usize)
    }

    /// Writes one pixel (silently clipped to the canvas).
    pub fn set(&mut self, x: i32, y: i32, c: Rgb) {
        if let Some(i) = self.offset(x, y) {
            self.pix.pixels_mut()[i] = opaque(c);
        }
    }

    /// Reads one pixel. The point must lie inside the canvas.
    pub fn get(&self, x: i32, y: i32) -> Rgb {
        let i = self.offset(x, y).expect("pixel inside the sheet");
        let p = self.pix.pixels()[i];
        [p.red(), p.green(), p.blue()]
    }

    /// Solid rectangle, no anti-aliasing.
    pub fn fill(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        let mut paint = Paint::default();
        paint.set_color(color(c));
        paint.anti_alias = false;
        self.fill_with(x, y, w, h, &paint);
    }

    fn fill_with(&mut self, x: i32, y: i32, w: i32, h: i32, paint: &Paint) {
        if w <= 0 || h <= 0 {
            return;
        }
        if let Some(rect) = Rect::from_xywh(x as f32, y as f32, w as f32, h as f32) {
            self.pix.fill_rect(rect, paint, Transform::identity(), None);
        }
    }

    /// Vertical gradient: `top` at the first row, `bottom` at the last.
    pub fn vgrad(&mut self, x: i32, y: i32, w: i32, h: i32, top: Rgb, bottom: Rgb) {
        if h <= 1 {
            self.fill(x, y, w, h, top);
            return;
        }
        let stops = vec![
            GradientStop::new(0.0, color(top)),
            GradientStop::new(1.0, color(bottom)),
        ];
        let shader = LinearGradient::new(
            Point::from_xy(0.0, y as f32),
            Point::from_xy(0.0, (y + h - 1) as f32),
            stops,
            SpreadMode::Pad,
            Transform::identity(),
        );
        match shader {
            Some(shader) => {
                let paint = Paint {
                    shader,
                    anti_alias: false,
                    ..Paint::default()
                };
                self.fill_with(x, y, w, h, &paint);
            }
            None => self.fill(x, y, w, h, top),
        }
    }

    /// One-pixel outline of a rectangle.
    pub fn frame(&mut self, x: i32, y: i32, w: i32, h: i32, c: Rgb) {
        self.fill(x, y, w, 1, c);
        self.fill(x, y + h - 1, w, 1, c);
        self.fill(x, y, 1, h, c);
        self.fill(x + w - 1, y, 1, h, c);
    }

    /// One-pixel bevel: `tl` on the top and left edges, `br` on the bottom and right.
    pub fn bevel(&mut self, x: i32, y: i32, w: i32, h: i32, tl: Rgb, br: Rgb) {
        self.fill(x, y, w, 1, tl);
        self.fill(x, y, 1, h, tl);
        self.fill(x, y + h - 1, w, 1, br);
        self.fill(x + w - 1, y, 1, h, br);
    }
}

fn color(c: Rgb) -> Color {
    Color::from_rgba8(c[0], c[1], c[2], 255)
}

fn opaque(c: Rgb) -> PremultipliedColorU8 {
    PremultipliedColorU8::from_rgba(c[0], c[1], c[2], 255).expect("opaque pixel is valid")
}

fn hash3(a: u32, b: u32, c: u32) -> u32 {
    let mut h =
        a.wrapping_mul(0x9E37_79B1) ^ b.wrapping_mul(0x85EB_CA77) ^ c.wrapping_mul(0xC2B2_AE3D);
    h ^= h >> 15;
    h = h.wrapping_mul(0x2C1B_3C6D);
    h ^ (h >> 12)
}

/// Deterministic horizontal brushed-metal noise. Each 9 px run of a row gets one
/// offset in `-amp..=amp`, added to every channel.
pub fn brush(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, seed: u32, amp: i32) {
    for yy in y..y + h {
        for xx in x..x + w {
            let run = (xx / 9) as u32;
            let n = (hash3(seed, yy as u32, run) % (2 * amp as u32 + 1)) as i32 - amp;
            let px = c.get(xx, yy);
            c.set(xx, yy, px.map(|v| (i32::from(v) + n).clamp(0, 255) as u8));
        }
    }
}

/// Colours of an outlined, bevelled key.
#[derive(Clone, Copy)]
pub struct Key {
    pub top: Rgb,
    pub bottom: Rgb,
    pub hi: Rgb,
    pub lo: Rgb,
    pub outline: Rgb,
}

/// Raised silver key.
pub const SILVER: Key = Key {
    top: SILVER_TOP,
    bottom: SILVER_BOT,
    hi: SILVER_HI,
    lo: SILVER_LO,
    outline: INK,
};

/// Raised steel key.
pub const STEEL: Key = Key {
    top: STEEL_TOP,
    bottom: STEEL_BOT,
    hi: STEEL_HI,
    lo: INK,
    outline: METAL_LO,
};

/// Darkens a colour to ~86% for pressed faces.
fn dim(c: Rgb) -> Rgb {
    c.map(|v| (u32::from(v) * 86 / 100) as u8)
}

/// A keyed button: 1 px outline, 1 px bevel and a gradient face. Pressed keys
/// swap the bevel and darken the face (inset look).
pub fn key(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, style: Key, pressed: bool) {
    c.frame(x, y, w, h, style.outline);
    let (tl, br) = if pressed {
        (style.lo, style.hi)
    } else {
        (style.hi, style.lo)
    };
    c.bevel(x + 1, y + 1, w - 2, h - 2, tl, br);
    let (top, bottom) = if pressed {
        (dim(style.top), dim(style.bottom))
    } else {
        (style.top, style.bottom)
    };
    c.vgrad(x + 2, y + 2, w - 4, h - 4, top, bottom);
}

/// A flat steel key (outline and gradient, no bevel) for labelled buttons.
pub fn flat_key(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, pressed: bool) {
    c.frame(x, y, w, h, METAL_LO);
    let (top, bottom) = if pressed {
        (dim(STEEL_TOP), dim(STEEL_BOT))
    } else {
        (STEEL_TOP, STEEL_BOT)
    };
    c.vgrad(x + 1, y + 1, w - 2, h - 2, top, bottom);
    c.fill(
        x + 1,
        y + 1,
        w - 2,
        1,
        if pressed { METAL_LO } else { STEEL_HI },
    );
}

/// A labelled steel button (bevelled). `led` adds a 2×2 indicator in the top-right
/// corner, lit when `on`. `area` is `(x, y, w, h)`.
pub fn steel_button(
    c: &mut Canvas,
    area: (i32, i32, i32, i32),
    text: &str,
    on: bool,
    pressed: bool,
    led: bool,
) {
    let (x, y, w, h) = area;
    key(c, x, y, w, h, STEEL, pressed);
    let p = i32::from(pressed);
    let ink = if on { LCD_ON } else { STEEL_TEXT };
    let tw = label_width(text, 1, false);
    label(
        c,
        x + (w - tw) / 2 + p,
        y + (h - 5) / 2 + p,
        text,
        ink,
        1,
        false,
    );
    if led {
        let lit = if on { LCD_ON } else { LCD_OFF };
        c.fill(x + w - 5 + p, y + 3 + p, 2, 2, lit);
    }
}

/// A 9×9 silver window pip with a 5×5 icon in the middle.
pub fn pip(c: &mut Canvas, x: i32, y: i32, icon: &[&str], pressed: bool, ink: Rgb) {
    key(c, x, y, 9, 9, SILVER, pressed);
    glyph(c, x + 2, y + 2, icon, ink);
}

/// A 1 px sunken frame: dark on the top and left, light on the bottom and right.
pub fn bevel_sunken(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
    c.bevel(x, y, w, h, METAL_LO, METAL_HI);
}

/// A sunken LCD well: dark outer bevel, a lighter inner rim and LCD black inside.
pub fn recess(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32) {
    bevel_sunken(c, x, y, w, h);
    c.frame(x + 1, y + 1, w - 2, h - 2, LCD_RIM);
    c.fill(x + 2, y + 2, w - 4, h - 4, LCD_BG);
}

/// A silver slider thumb, 14×11 (the volume and balance knobs).
pub fn slider_thumb(c: &mut Canvas, x: i32, y: i32, pressed: bool) {
    key(c, x, y, 14, 11, SILVER, pressed);
    for gy in [3, 5, 7] {
        c.fill(x + 4, y + gy, 6, 1, INK);
    }
}

/// Draws a bitmap glyph. `#` marks ink; `.` (and anything else) is skipped.
pub fn glyph(c: &mut Canvas, x: i32, y: i32, rows: &[&str], ink: Rgb) {
    for (ry, row) in rows.iter().enumerate() {
        for (cx, ch) in row.chars().enumerate() {
            if ch == '#' {
                c.set(x + cx as i32, y + ry as i32, ink);
            }
        }
    }
}

/// Columns a glyph's ink spans (0 for a blank glyph).
fn ink_width(rows: &[&str]) -> i32 {
    rows.iter()
        .map(|r| r.rfind('#').map_or(0, |i| i as i32 + 1))
        .max()
        .unwrap_or(0)
}

/// Blank advance for the space character, in pixels.
const SPACE_ADVANCE: i32 = 3;

fn advance(ch: char, gap: i32, bold: bool) -> i32 {
    let w = ink_width(&font::glyph(ch));
    if w == 0 {
        SPACE_ADVANCE + gap
    } else {
        w + i32::from(bold) + gap
    }
}

/// Width of `text` drawn with [`label`] (no trailing gap).
pub fn label_width(text: &str, gap: i32, bold: bool) -> i32 {
    if text.is_empty() {
        return 0;
    }
    let total: i32 = text.chars().map(|ch| advance(ch, gap, bold)).sum();
    total - gap
}

/// Proportional text for labels: each glyph is as wide as its ink, `gap` px apart.
/// `bold` smears every glyph one pixel to the right for chunkier strokes.
pub fn label(c: &mut Canvas, x: i32, y: i32, text: &str, ink: Rgb, gap: i32, bold: bool) {
    let mut cx = x;
    for ch in text.chars() {
        let rows = font::glyph(ch);
        glyph(c, cx, y, &rows, ink);
        if bold {
            glyph(c, cx + 1, y, &rows, ink);
        }
        cx += advance(ch, gap, bold);
    }
}

/// A grooved title strip: vertical gradient with a highlight and shadow line.
pub fn title_strip(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, s: &Strip) {
    c.vgrad(x, y + 1, w, h - 2, s.top, s.bottom);
    c.fill(x, y, w, 1, s.hi);
    c.fill(x, y + h - 1, w, 1, s.lo);
}

/// Bold text centred in a `w`×`h` box, with a one-pixel drop shadow.
pub fn centred_text(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, text: &str, s: &Strip) {
    let tw = label_width(text, 1, true);
    let tx = x + (w - tw) / 2;
    let ty = y + (h - 5) / 2;
    label(c, tx + 1, ty + 1, text, s.shadow, 1, true);
    label(c, tx, ty, text, s.text, 1, true);
}

/// A title bar with grooved rails either side of an optional centred wordmark.
pub fn wordmark_bar(c: &mut Canvas, x: i32, y: i32, w: i32, h: i32, s: &Strip, text: Option<&str>) {
    title_strip(c, x, y, w, h, s);
    for (x0, x1) in [(x + 30, x + 104), (x + w - 105, x + w - 31)] {
        c.fill(x0, y + 6, x1 - x0, 1, s.rail_dark);
        c.fill(x0, y + 7, x1 - x0, 1, s.rail_light);
    }
    if let Some(text) = text {
        centred_text(c, x, y, w, h, text, s);
    }
}

/// Piecewise-linear colour ramp over `t` in `0..=1`. Stops must be sorted by position.
pub fn ramp(t: f32, stops: &[(f32, Rgb)]) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    for pair in stops.windows(2) {
        let (t0, c0) = pair[0];
        let (t1, c1) = pair[1];
        if t <= t1 {
            let f = if t1 > t0 { (t - t0) / (t1 - t0) } else { 0.0 };
            return std::array::from_fn(|i| {
                (f32::from(c0[i]) + (f32::from(c1[i]) - f32::from(c0[i])) * f).round() as u8
            });
        }
    }
    stops.last().map_or(KEY, |s| s.1)
}

/// The 7 segments of a 9×13 LCD digit as `(x, y, w, h)`, bit `i` = segment `a + i`.
const SEGMENTS: [(i32, i32, i32, i32); 7] = [
    (1, 0, 7, 2),  // a: top
    (7, 2, 2, 4),  // b: upper right
    (7, 7, 2, 4),  // c: lower right
    (1, 11, 7, 2), // d: bottom
    (0, 7, 2, 4),  // e: lower left
    (0, 2, 2, 4),  // f: upper left
    (1, 6, 7, 1),  // g: middle
];

/// Draws a seven-segment digit cell (9×13). `lit` segments are LCD green, `ghost`
/// segments are the dim "off" colour. Bit `i` selects segment `a + i`.
pub fn seven_seg(c: &mut Canvas, x: i32, y: i32, lit: u8, ghost: u8) {
    c.fill(x, y, 9, 13, LCD_BG);
    for (i, &(sx, sy, sw, sh)) in SEGMENTS.iter().enumerate() {
        let bit = 1u8 << i;
        let colour = if lit & bit != 0 {
            LCD_ON
        } else if ghost & bit != 0 {
            LCD_GHOST
        } else {
            continue;
        };
        c.fill(x + sx, y + sy, sw, sh, colour);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ramp_hits_its_stops_and_interpolates() {
        let stops = [(0.0, hex(0x000000)), (1.0, hex(0x0000FF))];
        assert_eq!(ramp(0.0, &stops), hex(0x000000));
        assert_eq!(ramp(1.0, &stops), hex(0x0000FF));
        assert_eq!(ramp(0.5, &stops), hex(0x000080));
    }

    #[test]
    fn label_width_matches_drawn_extent() {
        assert_eq!(label_width("", 1, false), 0);
        assert_eq!(label_width("ab", 1, false), 4 + 1 + 4);
        assert_eq!(label_width("ab", 1, true), 5 + 1 + 5);
    }

    #[test]
    fn unpainted_pixels_stay_transparency_key() {
        let c = Canvas::new(4, 4);
        assert_eq!(c.get(0, 0), KEY);
        assert_eq!(c.get(3, 3), KEY);
    }
}
