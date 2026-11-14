//! The generic window frame ("gen" window): the tiled GEN.BMP frame at any size in 25 × 29 px
//! steps, the title drawn with GEN.BMP's own letters, the close button and the resize grip. The
//! media library uses it, and the Milkdrop window will too.
//!
//! Piece names, positions and the tiling rules follow Webamp's `css/gen-window.css`,
//! `js/components/GenWindow/index.tsx` and `js/skinSelectors.ts` (MIT, © Jordan Eldredge). The
//! letters follow Webamp's `getLetters` in `js/skinParser.js`: A to Z are the runs of coloured
//! pixels on one row of GEN.BMP, split by the background pixel at x = 0. The selected letters use
//! row 88 and the normal ones row 96. A skin without usable letters (or with an empty selected
//! row) falls back to the normal letters, and to TEXT.BMP when it has none at all.

use tubeamp_skin::{Rgba, Sheet, Skin};

use crate::canvas::{Canvas, Rect};
use crate::text::draw_text;

/// Width of the frame at the base size (1×), in skin pixels.
pub const BASE_WIDTH: u32 = 275;
/// Height of the frame at the base size (1×), in skin pixels.
pub const BASE_HEIGHT: u32 = 116;
/// Width added per horizontal resize step.
pub const STEP_W: u32 = 25;
/// Height added per vertical resize step.
pub const STEP_H: u32 = 29;
/// Largest number of resize steps in either direction (a sanity bound, as in the playlist).
pub const MAX_STEPS: u32 = 40;

/// Height of the title bar (the top row of pieces).
pub const TITLE_H: i32 = 20;
/// Height of the bottom bar.
pub const BOTTOM_H: i32 = 14;
/// Width of the left and right frame columns.
pub const LEFT_W: i32 = 11;
pub const RIGHT_W: i32 = 8;
/// Width of a top-bar corner or end piece, and of the bottom corners.
const CORNER_W: i32 = 25;
const BOTTOM_CORNER_W: i32 = 125;
/// The resize grip: the bottom-right square.
pub const GRIP: i32 = 20;

/// Height of a GEN letter sprite.
const LETTER_H: u32 = 7;
/// Rows of GEN.BMP that hold the selected and the normal letters.
const ROW_SELECTED: u32 = 88;
const ROW_NORMAL: u32 = 96;
/// Widest letter accepted from a scan. A wider run means the row is not a letter row.
const MAX_LETTER_W: u32 = 16;
/// Width of a space in the title, and of the TEXT.BMP fallback cell.
const SPACE_W: i32 = 5;
/// Title box padding: 4 px before the text and 3 px after, as Webamp's `.gen-top-title`.
const TITLE_PAD_L: i32 = 4;
const TITLE_PAD_R: i32 = 3;
/// The title's top within the title bar (Webamp's `margin-top: 2px`).
const TITLE_Y: i32 = 2;

/// Window size in skin pixels for resize steps `(sx, sy)`.
pub fn window_size(steps: (u32, u32)) -> (u32, u32) {
    (
        BASE_WIDTH + STEP_W * steps.0,
        BASE_HEIGHT + STEP_H * steps.1,
    )
}

/// Resize steps for a frame of `w` × `h` skin pixels, rounded to the nearest step and never
/// below the base size.
pub fn steps_for_size(w: i32, h: i32) -> (u32, u32) {
    let sx = ((w - BASE_WIDTH as i32) as f64 / f64::from(STEP_W)).round();
    let sy = ((h - BASE_HEIGHT as i32) as f64 / f64::from(STEP_H)).round();
    (
        sx.clamp(0.0, f64::from(MAX_STEPS)) as u32,
        sy.clamp(0.0, f64::from(MAX_STEPS)) as u32,
    )
}

/// The client area: what sits inside the frame, in window coordinates.
pub fn client_rect(steps: (u32, u32)) -> Rect {
    let (w, h) = window_size(steps);
    Rect::new(
        LEFT_W,
        TITLE_H,
        w as i32 - LEFT_W - RIGHT_W,
        h as i32 - TITLE_H - BOTTOM_H,
    )
}

/// Close button rectangle: 2 px in from the right, 3 px down, 9 × 9.
fn close_rect(steps: (u32, u32)) -> Rect {
    let (w, _) = window_size(steps);
    Rect::new(w as i32 - 11, 3, 9, 9)
}

/// The resize grip rectangle at the bottom-right corner.
fn grip_rect(steps: (u32, u32)) -> Rect {
    let (w, h) = window_size(steps);
    Rect::new(w as i32 - GRIP, h as i32 - GRIP, GRIP, GRIP)
}

/// What a point on the frame belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GenHit {
    /// The title bar, away from the close button: drags the window.
    TitleBar,
    Close,
    /// The bottom-right grip: resizes the window.
    Resize,
    /// Inside the client area (window coordinates are in [`client_rect`]).
    Client,
    /// The side or bottom frame, outside the client area.
    Frame,
}

/// Which frame piece is at window point `(x, y)`, or `None` outside the window.
pub fn hit_test_gen(steps: (u32, u32), x: i32, y: i32) -> Option<GenHit> {
    let (w, h) = window_size(steps);
    if x < 0 || y < 0 || x >= w as i32 || y >= h as i32 {
        return None;
    }
    if close_rect(steps).contains(x, y) {
        return Some(GenHit::Close);
    }
    if grip_rect(steps).contains(x, y) {
        return Some(GenHit::Resize);
    }
    if y < TITLE_H {
        return Some(GenHit::TitleBar);
    }
    if client_rect(steps).contains(x, y) {
        return Some(GenHit::Client);
    }
    Some(GenHit::Frame)
}

/// One GEN letter: its column range on the sheet, at the row the scan used.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Letter {
    pub x: u32,
    pub w: u32,
}

/// The 26 letters of one row, A to Z.
pub type Letters = [Letter; 26];

/// Webamp's `getLetters` for one row: starting at x = 1, each letter is the run of pixels that
/// differ from the background (the pixel at x = 0). Returns `None` when the row holds no letter
/// or a run is too wide to be one.
pub fn scan_letters(sheet: &Rgba, y: u32) -> Option<Letters> {
    if y >= sheet.h || sheet.w < 2 {
        return None;
    }
    let at = |x: u32| sheet.px[(y * sheet.w + x) as usize];
    let bg = at(0);
    let mut letters = [Letter { x: 0, w: 0 }; 26];
    let mut x = 1u32;
    for letter in letters.iter_mut() {
        if x >= sheet.w {
            *letter = Letter { x, w: 0 };
            continue;
        }
        let mut next = x;
        while next < sheet.w && at(next) != bg {
            next += 1;
        }
        let w = next - x;
        if w > MAX_LETTER_W {
            return None;
        }
        *letter = Letter { x, w };
        x = next + 1;
    }
    letters.iter().any(|l| l.w > 0).then_some(letters)
}

/// How the title is drawn for a skin: with GEN letters from one row, or with TEXT.BMP.
// The letter table makes the `Gen` variant large, but there is only ever one of these per frame.
#[allow(clippy::large_enum_variant)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitleFont {
    Gen { row: u32, letters: Letters },
    Text,
}

/// The title font for `skin`: the selected letters when focused, then the normal ones.
fn title_font(skin: &Skin, focused: bool) -> TitleFont {
    let Some(sheet) = skin.sheet(Sheet::Gen) else {
        return TitleFont::Text;
    };
    let rows: &[u32] = if focused {
        &[ROW_SELECTED, ROW_NORMAL]
    } else {
        &[ROW_NORMAL]
    };
    for &row in rows {
        if let Some(letters) = scan_letters(sheet, row) {
            return TitleFont::Gen { row, letters };
        }
    }
    TitleFont::Text
}

/// Width of one title character in the GEN font. Space and unknown characters are 5 px.
fn gen_char_width(letters: &Letters, c: char) -> i32 {
    match c.to_ascii_uppercase() {
        ch @ 'A'..='Z' => letters[(ch as u8 - b'A') as usize].w as i32,
        _ => SPACE_W,
    }
}

/// Width of the title text (the drawn letters, without padding).
fn title_text_width(font: TitleFont, title: &str) -> i32 {
    match font {
        TitleFont::Gen { letters, .. } => title.chars().map(|c| gen_char_width(&letters, c)).sum(),
        TitleFont::Text => title.chars().count() as i32 * SPACE_W,
    }
}

/// Width of the title box: the text plus Webamp's padding.
fn title_box_width(font: TitleFont, title: &str) -> i32 {
    title_text_width(font, title) + TITLE_PAD_L + TITLE_PAD_R
}

/// The title box width for `title` on `skin`, for layout and tests.
pub fn title_width(skin: &Skin, title: &str, focused: bool) -> i32 {
    title_box_width(title_font(skin, focused), title)
}

/// Draws the whole frame at the size of `steps` into `canvas`, which must be [`window_size`] in
/// size. The client area is left as it was (the caller draws into [`client_rect`]).
pub fn render_frame(
    skin: &Skin,
    steps: (u32, u32),
    title: &str,
    focused: bool,
    canvas: &mut Canvas,
) {
    let (w, h) = window_size(steps);
    let (w, h) = (w as i32, h as i32);
    let sel = |normal: &'static str, selected: &'static str| -> &'static str {
        if focused {
            selected
        } else {
            normal
        }
    };

    // Top bar: corners and end pieces, the fills between them, and the title box in the middle.
    let font = title_font(skin, focused);
    let box_w = title_box_width(font, title);
    let remaining = (w - 4 * CORNER_W - box_w).max(0);
    let fill_l = remaining / 2;
    let fill_r = remaining - fill_l;
    let left_end_x = CORNER_W + fill_l;
    let title_x = left_end_x + CORNER_W;
    let right_end_x = title_x + box_w;
    let right_fill_x = right_end_x + CORNER_W;
    let right_x = w - CORNER_W;

    gen_piece(
        skin,
        canvas,
        sel("GEN_TOP_LEFT", "GEN_TOP_LEFT_SELECTED"),
        0,
        0,
    );
    tile_x(
        skin,
        canvas,
        sel(
            "GEN_TOP_LEFT_RIGHT_FILL",
            "GEN_TOP_LEFT_RIGHT_FILL_SELECTED",
        ),
        CORNER_W,
        left_end_x,
        0,
        false,
    );
    gen_piece(
        skin,
        canvas,
        sel("GEN_TOP_LEFT_END", "GEN_TOP_LEFT_END_SELECTED"),
        left_end_x,
        0,
    );
    gen_piece(
        skin,
        canvas,
        sel("GEN_TOP_RIGHT_END", "GEN_TOP_RIGHT_END_SELECTED"),
        right_end_x,
        0,
    );
    tile_x(
        skin,
        canvas,
        sel(
            "GEN_TOP_LEFT_RIGHT_FILL",
            "GEN_TOP_LEFT_RIGHT_FILL_SELECTED",
        ),
        right_fill_x,
        right_fill_x + fill_r,
        0,
        true,
    );
    gen_piece(
        skin,
        canvas,
        sel("GEN_TOP_RIGHT", "GEN_TOP_RIGHT_SELECTED"),
        right_x,
        0,
    );
    // The title box sits on the centre fill (Webamp puts GEN_TOP_CENTER_FILL behind the whole
    // `.gen-top` row), so the title is not drawn over black.
    tile_x(
        skin,
        canvas,
        sel("GEN_TOP_CENTER_FILL", "GEN_TOP_CENTER_FILL_SELECTED"),
        title_x,
        right_end_x,
        0,
        false,
    );
    draw_title(skin, canvas, font, title, title_x + TITLE_PAD_L);
    gen_piece(skin, canvas, "GEN_CLOSE_SELECTED", w - 11, 3);

    // Sides: the middle tile runs down the frame, and the bottom 24 px are drawn over it.
    let middle_top = TITLE_H;
    let middle_bottom = h - BOTTOM_H;
    tile_y(
        skin,
        canvas,
        "GEN_MIDDLE_LEFT",
        0,
        middle_top,
        middle_bottom,
    );
    gen_piece(
        skin,
        canvas,
        "GEN_MIDDLE_LEFT_BOTTOM",
        0,
        middle_bottom - 24,
    );
    tile_y(
        skin,
        canvas,
        "GEN_MIDDLE_RIGHT",
        w - RIGHT_W,
        middle_top,
        middle_bottom,
    );
    gen_piece(
        skin,
        canvas,
        "GEN_MIDDLE_RIGHT_BOTTOM",
        w - RIGHT_W,
        middle_bottom - 24,
    );

    // Bottom bar: the fill runs under both corners, which hold the resize grip.
    tile_x(skin, canvas, "GEN_BOTTOM_FILL", 0, w, middle_bottom, false);
    gen_piece(skin, canvas, "GEN_BOTTOM_LEFT", 0, middle_bottom);
    gen_piece(
        skin,
        canvas,
        "GEN_BOTTOM_RIGHT",
        w - BOTTOM_CORNER_W,
        middle_bottom,
    );
}

/// Draws the title text inside its box, at `x` (the text's left edge) and [`TITLE_Y`].
fn draw_title(skin: &Skin, canvas: &mut Canvas, font: TitleFont, title: &str, x: i32) {
    match font {
        TitleFont::Gen { row, letters } => {
            let mut pen = x;
            for c in title.chars() {
                if let Some(letter) = letter_of(&letters, c).filter(|l| l.w > 0) {
                    if let Some(view) = skin.region(Sheet::Gen, letter.x, row, letter.w, LETTER_H) {
                        canvas.blit(&view, pen, TITLE_Y);
                    }
                }
                pen += gen_char_width(&letters, c);
            }
        }
        TitleFont::Text => {
            let clip = Rect::new(x, 0, canvas.w as i32 - x, TITLE_H);
            draw_text(
                canvas,
                skin,
                &title.to_uppercase(),
                x,
                (TITLE_H - 6) / 2,
                clip,
            );
        }
    }
}

/// The letter for character `c`, if it is A to Z (either case).
fn letter_of(letters: &Letters, c: char) -> Option<Letter> {
    match c.to_ascii_uppercase() {
        ch @ 'A'..='Z' => Some(letters[(ch as u8 - b'A') as usize]),
        _ => None,
    }
}

/// Draws a GEN sprite by name at `(x, y)`, if the skin has it.
fn gen_piece(skin: &Skin, canvas: &mut Canvas, name: &str, x: i32, y: i32) {
    if let Some(view) = skin.sprite(Sheet::Gen, name) {
        canvas.blit(&view, x, y);
    }
}

/// Repeats a GEN sprite horizontally across `[x0, x1)`, at row `y`. With `align_right` the tiles
/// line up on the right edge (Webamp's `background-position: right`).
fn tile_x(
    skin: &Skin,
    canvas: &mut Canvas,
    name: &str,
    x0: i32,
    x1: i32,
    y: i32,
    align_right: bool,
) {
    let Some(view) = skin.sprite(Sheet::Gen, name) else {
        return;
    };
    let w = view.w as i32;
    if w == 0 || x1 <= x0 {
        return;
    }
    let count = (x1 - x0 + w - 1) / w;
    let start = if align_right { x1 - count * w } else { x0 };
    let clip = Rect::new(x0, y, x1 - x0, view.h as i32);
    for k in 0..count {
        canvas.blit_clipped(&view, start + k * w, y, clip);
    }
}

/// Repeats a GEN sprite vertically across `[y0, y1)` at column `x`, top-aligned.
fn tile_y(skin: &Skin, canvas: &mut Canvas, name: &str, x: i32, y0: i32, y1: i32) {
    let Some(view) = skin.sprite(Sheet::Gen, name) else {
        return;
    };
    let h = view.h as i32;
    if h == 0 || y1 <= y0 {
        return;
    }
    let clip = Rect::new(x, y0, view.w as i32, y1 - y0);
    let mut y = y0;
    while y < y1 {
        canvas.blit_clipped(&view, x, y, clip);
        y += h;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tubeamp_skin::testkit;

    fn px(c: &Canvas, x: i32, y: i32) -> u32 {
        c.px[(y as u32 * c.w + x as u32) as usize]
    }

    fn render(skin: &Skin, steps: (u32, u32), title: &str, focused: bool) -> Canvas {
        let (w, h) = window_size(steps);
        let mut c = Canvas::new(w, h);
        render_frame(skin, steps, title, focused, &mut c);
        c
    }

    fn rgba(w: u32, h: u32, f: impl Fn(u32, u32) -> u32) -> Rgba {
        let mut px = Vec::with_capacity((w * h) as usize);
        for y in 0..h {
            for x in 0..w {
                px.push(f(x, y));
            }
        }
        Rgba { w, h, px }
    }

    #[test]
    fn sizes_follow_the_steps_and_never_go_below_the_base() {
        assert_eq!(window_size((0, 0)), (275, 116));
        assert_eq!(window_size((10, 8)), (525, 348));
        assert_eq!(steps_for_size(100, 50), (0, 0));
        assert_eq!(steps_for_size(525, 348), (10, 8));
        assert_eq!(steps_for_size(100_000, 100_000), (MAX_STEPS, MAX_STEPS));
    }

    #[test]
    fn client_area_sits_inside_the_frame() {
        assert_eq!(client_rect((0, 0)), Rect::new(11, 20, 256, 82));
        assert_eq!(client_rect((10, 8)), Rect::new(11, 20, 506, 314));
    }

    #[test]
    fn hit_test_finds_each_piece() {
        let steps = (0, 0);
        let hit = |x, y| hit_test_gen(steps, x, y);
        assert_eq!(hit(100, 5), Some(GenHit::TitleBar));
        assert_eq!(hit(266, 6), Some(GenHit::Close));
        assert_eq!(hit(270, 110), Some(GenHit::Resize));
        assert_eq!(hit(50, 50), Some(GenHit::Client));
        assert_eq!(hit(5, 50), Some(GenHit::Frame), "the left frame");
        assert_eq!(
            hit(100, 112),
            Some(GenHit::Frame),
            "the bottom edge, left of the grip"
        );
        assert_eq!(hit(-1, 5), None);
        assert_eq!(hit(275, 5), None);
        assert_eq!(hit(5, 116), None);
    }

    #[test]
    fn scan_finds_runs_split_by_the_background() {
        // Background 0x11 at x = 0; letters are runs of 0x22 separated by one background pixel.
        let bg = 0xFF11_1111;
        let fg = 0xFF22_2222;
        let mut row = vec![bg];
        let mut letter_widths = Vec::new();
        for k in 0..26u32 {
            let w = 3 + (k % 4);
            letter_widths.push(w);
            row.extend(std::iter::repeat_n(fg, w as usize));
            row.push(bg);
        }
        let img = Rgba {
            w: row.len() as u32,
            h: 1,
            px: row,
        };
        let letters = scan_letters(&img, 0).expect("letters");
        for (k, letter) in letters.iter().enumerate() {
            assert_eq!(letter.w, letter_widths[k], "letter {k}");
        }
        assert_eq!(letters[0].x, 1);
        assert_eq!(scan_letters(&img, 5), None, "row out of range");
    }

    #[test]
    fn scan_rejects_rows_without_letters_and_wide_runs() {
        let blank = rgba(60, 1, |_, _| 0xFFFF_00FF);
        assert_eq!(scan_letters(&blank, 0), None);
        let wide = rgba(60, 1, |x, _| if x == 0 { 1 } else { 2 });
        assert_eq!(
            scan_letters(&wide, 0),
            None,
            "one run is wider than a letter"
        );
    }

    #[test]
    fn title_uses_gen_letters_when_the_skin_has_them() {
        // A GEN sheet with letters on row 96 (3 px each) and a blank row 88.
        let bg = 0xFFFF_00FF;
        // Rows 96 to 102 hold the letters, so the sheet needs 103 rows.
        let mut px = vec![bg; (275 * 103) as usize];
        let mut x = 1u32;
        for _ in 0..26 {
            for dx in 0..3 {
                for dy in 0..7 {
                    px[((96 + dy) * 275 + x + dx) as usize] = 0xFF00_FF00;
                }
            }
            x += 4;
        }
        let gen = Rgba { w: 275, h: 103, px };
        let bytes = testkit::zip_bytes(&[
            (
                "MAIN.BMP",
                testkit::bmp_bytes(&rgba(275, 116, |_, _| 0xFF00_0000)),
            ),
            ("GEN.BMP", testkit::bmp_bytes(&gen)),
        ]);
        let skin = Skin::load(&bytes).expect("skin");
        // "AB" is 3 + 3 px of letters, plus the padding (4 + 3).
        assert_eq!(title_width(&skin, "AB", false), 6 + 7);
        // The selected row is blank, so a focused window falls back to the normal letters.
        assert_eq!(title_width(&skin, "AB", true), 6 + 7);
        // A space is 5 px.
        assert_eq!(title_width(&skin, "A B", false), 3 + 5 + 3 + 7);
    }

    #[test]
    fn title_falls_back_to_text_for_a_skin_without_gen_letters() {
        let skin = testkit::synthetic_skin_without(&[Sheet::Gen]);
        assert_eq!(title_width(&skin, "MEDIA", false), 5 * 5 + 7);
        let c = render(&skin, (0, 0), "MEDIA", true);
        let (w, _) = window_size((0, 0));
        assert_eq!(c.w, w);
    }

    #[test]
    fn corners_and_grip_are_placed_from_the_gen_sheet() {
        let skin = testkit::synthetic_skin();
        let steps = (0, 0);
        let c = render(&skin, steps, "MEDIA", true);
        let (w, h) = window_size(steps);
        let top_left = skin
            .sprite(Sheet::Gen, "GEN_TOP_LEFT_SELECTED")
            .expect("sprite");
        assert_eq!(px(&c, 0, 0), top_left.pixel(0, 0));
        let top_right = skin
            .sprite(Sheet::Gen, "GEN_TOP_RIGHT_SELECTED")
            .expect("sprite");
        assert_eq!(px(&c, w as i32 - 25, 0), top_right.pixel(0, 0));
        let grip = skin.sprite(Sheet::Gen, "GEN_BOTTOM_RIGHT").expect("sprite");
        assert_eq!(
            px(&c, w as i32 - 125, h as i32 - 14),
            grip.pixel(0, 0),
            "the bottom-right corner piece"
        );
    }

    #[test]
    fn title_box_is_tiled_with_the_centre_fill() {
        let skin = testkit::synthetic_skin();
        let title = "MEDIA";
        for focused in [true, false] {
            let c = render(&skin, (0, 0), title, focused);
            let box_w = title_width(&skin, title, focused);
            // The left fill takes what the title does not, split evenly; the left end is 25 px.
            let fill_l = (275 - 4 * 25 - box_w) / 2;
            let title_x = 25 + fill_l + 25;
            let name = if focused {
                "GEN_TOP_CENTER_FILL_SELECTED"
            } else {
                "GEN_TOP_CENTER_FILL"
            };
            let fill = skin.sprite(Sheet::Gen, name).expect("fill");
            // Row 0 is above the letters (they start at y 2), so the fill shows there, across
            // more than one tile.
            for x in [title_x, title_x + 25, title_x + box_w - 1] {
                assert_eq!(
                    px(&c, x, 0),
                    fill.pixel((x - title_x) as u32 % 25, 0),
                    "focused {focused}, x {x}"
                );
            }
        }
    }

    #[test]
    fn unfocused_frame_uses_the_normal_pieces() {
        let skin = testkit::synthetic_skin();
        let c = render(&skin, (0, 0), "MEDIA", false);
        let plain = skin.sprite(Sheet::Gen, "GEN_TOP_LEFT").expect("sprite");
        assert_eq!(px(&c, 0, 0), plain.pixel(0, 0));
    }

    #[test]
    fn render_survives_every_size_and_both_focus_states() {
        let skin = testkit::synthetic_skin();
        for sx in 0..=6 {
            for sy in 0..=6 {
                for focused in [true, false] {
                    let _ = render(&skin, (sx, sy), "MEDIA LIBRARY", focused);
                }
            }
        }
        let bare = testkit::synthetic_skin_without(&[Sheet::Gen]);
        let _ = render(&bare, (3, 3), "MEDIA LIBRARY", true);
    }
}
