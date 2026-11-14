//! Generic-window colours from `GENEX.BMP`, read the way Webamp's `getGenExColors` does: the
//! colour of each entry is the pixel of row 0 at a fixed x. The media library draws its list,
//! text and background with these, so it follows the skin.

use crate::bitmap::Rgba;

/// Smallest GENEX width that holds every entry read here (the last one is at x = 90).
const MIN_WIDTH: u32 = 92;

/// Colours for the generic windows, as `0xAARRGGBB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GenExColors {
    /// x=48: background of edits and list views.
    pub item_background: u32,
    /// x=50: text of edits and list views.
    pub item_foreground: u32,
    /// x=52: window background.
    pub window_background: u32,
    /// x=54: button text.
    pub button_text: u32,
    /// x=56: window text.
    pub window_text: u32,
    /// x=58: dividers and sunken borders.
    pub divider: u32,
    /// x=60: selection colour for entries inside playlists.
    pub playlist_selection: u32,
    /// x=74: scrollbar colour one.
    pub scrollbar_one: u32,
    /// x=76: scrollbar colour two.
    pub scrollbar_two: u32,
    /// x=82: scrollbar dead area.
    pub scrollbar_dead_area: u32,
    /// x=84: list text, highlighted.
    pub list_text_highlighted: u32,
    /// x=86: list background, highlighted.
    pub list_text_highlighted_background: u32,
    /// x=88: list text, selected.
    pub list_text_selected: u32,
    /// x=90: list background, selected.
    pub list_text_selected_background: u32,
}

impl GenExColors {
    /// Reads the colours from a GENEX bitmap. `None` when it is too narrow to hold them.
    pub fn from_sheet(img: &Rgba) -> Option<GenExColors> {
        if img.w < MIN_WIDTH || img.h == 0 {
            return None;
        }
        let at = |x: u32| img.px[x as usize] | 0xFF00_0000;
        Some(GenExColors {
            item_background: at(48),
            item_foreground: at(50),
            window_background: at(52),
            button_text: at(54),
            window_text: at(56),
            divider: at(58),
            playlist_selection: at(60),
            scrollbar_one: at(74),
            scrollbar_two: at(76),
            scrollbar_dead_area: at(82),
            list_text_highlighted: at(84),
            list_text_highlighted_background: at(86),
            list_text_selected: at(88),
            list_text_selected_background: at(90),
        })
    }
}

impl Default for GenExColors {
    /// Used when a skin has no usable GENEX.BMP. These are TubeAmp Classic's own values.
    fn default() -> Self {
        GenExColors {
            item_background: 0xFF05_070A,
            item_foreground: 0xFF1E_E65A,
            window_background: 0xFF1E_2233,
            button_text: 0xFFC9_CED8,
            window_text: 0xFFE6_EAF2,
            divider: 0xFF0E_101A,
            playlist_selection: 0xFF1E_3A8A,
            scrollbar_one: 0xFF4A_5068,
            scrollbar_two: 0xFF2A_2F45,
            scrollbar_dead_area: 0xFF1E_2233,
            list_text_highlighted: 0xFFF4_F7FC,
            list_text_highlighted_background: 0xFF1E_3A8A,
            list_text_selected: 0xFF1E_E65A,
            list_text_selected_background: 0xFF05_070A,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_row_zero_at_the_webamp_positions() {
        let mut px = vec![0u32; (MIN_WIDTH * 2) as usize];
        px[48] = 0x0011_2233;
        px[50] = 0x0044_5566;
        px[90] = 0x0077_8899;
        let img = Rgba {
            w: MIN_WIDTH,
            h: 2,
            px,
        };
        let c = GenExColors::from_sheet(&img).expect("wide enough");
        assert_eq!(c.item_background, 0xFF11_2233);
        assert_eq!(c.item_foreground, 0xFF44_5566);
        assert_eq!(c.list_text_selected_background, 0xFF77_8899);
    }

    #[test]
    fn narrow_or_missing_genex_gives_the_defaults() {
        let small = Rgba {
            w: 16,
            h: 16,
            px: vec![0; 256],
        };
        assert_eq!(GenExColors::from_sheet(&small), None);
        let skin = crate::testkit::synthetic_skin_without(&[crate::Sheet::Genex]);
        assert_eq!(skin.genex, GenExColors::default());
    }
}
