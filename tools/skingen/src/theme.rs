//! Skin styles. `TubeAmp Classic` is the default and draws exactly as before. `Crimson Edge`
//! draws the same sheets with an angular look:
//!
//! - Drawing helpers (keys, pips, title bars, the wordmark) check [`crimson`] and add chamfers,
//!   red rules and slashes, and an italic wordmark.
//! - [`recolour`] runs over every finished sheet. It maps each classic colour family to the
//!   Crimson palette: green LCD to red LCD (ghosts dark red), blue-grey metal and neutral silver to
//!   a carbon-to-gunmetal ramp, and red stays red. It also lays a deterministic carbon weave over
//!   the metal, so the sheets keep their geometry and only change colour.

use crate::paint::{hex, ramp, Rgb, KEY};
use image::RgbImage;
use std::sync::OnceLock;

/// Which skin is being generated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    Classic,
    CrimsonEdge,
}

static STYLE: OnceLock<Style> = OnceLock::new();

/// Sets the style for this run. Only the first call takes effect.
pub fn set_style(style: Style) {
    let _ = STYLE.set(style);
}

/// The style in effect (Classic unless [`set_style`] chose otherwise).
pub fn style() -> Style {
    STYLE.get().copied().unwrap_or(Style::Classic)
}

/// Whether the Crimson Edge style is in effect.
pub fn crimson() -> bool {
    style() == Style::CrimsonEdge
}

/// Crimson Edge accents.
pub const ACCENT: Rgb = hex(0xE8102E);
pub const ACCENT_HI: Rgb = hex(0xFF3B4E);
pub const ACCENT_DARK: Rgb = hex(0x7A0716);
/// Lit LCD red and its near-invisible ghost segments.
pub const LCD_RED: Rgb = hex(0xFF2A3D);
pub const LCD_GHOST: Rgb = hex(0x2A0509);

/// Carbon-to-gunmetal ramp for neutral and blue-grey pixels, by luma.
const METAL_STOPS: [(f32, Rgb); 5] = [
    (0.0, hex(0x0B0B0E)),
    (0.375, hex(0x1E1E24)),
    (0.63, hex(0x6E727A)),
    (0.78, hex(0x8E939C)),
    (1.0, hex(0xF2F2F5)),
];

/// Maps a finished sheet to the Crimson Edge palette, in place. Transparent (magenta) pixels stay
/// transparent.
pub fn recolour(img: &mut RgbImage) {
    let (w, h) = img.dimensions();
    for y in 0..h {
        for x in 0..w {
            let px = img.get_pixel_mut(x, y);
            let [r, g, b] = px.0;
            if [r, g, b] == KEY {
                continue;
            }
            px.0 = map_pixel(x, y, [r, g, b]);
        }
    }
}

/// The Crimson Edge colour for one classic pixel at `(x, y)`.
fn map_pixel(x: u32, y: u32, [r, g, b]: Rgb) -> Rgb {
    let (r32, g32, b32) = (i32::from(r), i32::from(g), i32::from(b));
    // Green-dominant pixels are LCD: scale the lit red by brightness, ghosts stay dark.
    if g32 >= r32 && g32 > b32 + 15 {
        let t = ((g32 - 30) as f32 / 200.0).clamp(0.0, 1.0);
        let t = t * t;
        return mix(LCD_GHOST, LCD_RED, t);
    }
    // Red-dominant pixels (accents, the level meter's top) keep their red.
    if r32 > g32 + 25 && r32 > b32 + 25 {
        return [r, g, b];
    }
    // Everything else is metal: luma through the carbon ramp, plus the weave.
    let luma = 0.299 * f32::from(r) + 0.587 * f32::from(g) + 0.114 * f32::from(b);
    let base = ramp(luma / 255.0, &METAL_STOPS);
    let off = weave(x, y);
    base.map(|v| (i32::from(v) + off).clamp(0, 255) as u8)
}

/// Linear blend of two colours, `t` from `a` (0) to `b` (1).
fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    std::array::from_fn(|i| {
        (f32::from(a[i]) + (f32::from(b[i]) - f32::from(a[i])) * t).round() as u8
    })
}

/// Deterministic diagonal carbon weave, a small offset per pixel (-2..=3).
fn weave(x: u32, y: u32) -> i32 {
    let (x, y) = (i64::from(x), i64::from(y));
    let d = ((x + y) >> 1) & 1;
    let e = ((x + 65536 - y) >> 1) & 1;
    match (d, e) {
        (1, 1) => 3,
        (0, 0) => -2,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn green_lcd_maps_to_red_and_ghosts_stay_dark() {
        assert_eq!(map_pixel(0, 0, hex(0x1EE65A)), LCD_RED);
        let ghost = map_pixel(0, 0, hex(0x0B3A1C));
        assert!(ghost[0] < 60 && ghost[1] < 12, "ghost {ghost:?} stays dark");
    }

    #[test]
    fn red_accents_and_metal_are_kept_or_darkened() {
        assert_eq!(map_pixel(0, 0, hex(0xE8102E)), hex(0xE8102E));
        let body = map_pixel(0, 0, hex(0x2A2F45));
        assert!(
            body.iter().all(|&v| v < 40),
            "blue-grey body becomes carbon: {body:?}"
        );
    }

    #[test]
    fn the_weave_is_deterministic() {
        assert_eq!(weave(3, 5), weave(3, 5));
        assert!((0..16).any(|i| weave(i, 0) != weave(0, 0)));
    }
}
