//! The classic video window (Winamp's VIDEO.BMP): the title bar, the side and bottom frame, the
//! five bottom buttons, the close button and the info text under the video.
//!
//! The content area is left untouched, because the bridge's webview covers it. Geometry follows
//! `docs/VIDEO-BMP.md`. W and H are the window size in skin pixels. When the skin has no
//! VIDEO.BMP the frame is drawn as a plain bevel instead, so the window still works.

use tubeamp_skin::{Sheet, Skin, SpriteView};

use crate::canvas::{Canvas, Rect};
use crate::font::draw_track_text;
use crate::text::draw_text;

/// Default window size in skin pixels.
pub const DEFAULT_W: u32 = 275;
pub const DEFAULT_H: u32 = 232;
/// Smallest window the frame can be drawn at.
pub const MIN_W: u32 = 275;
pub const MIN_H: u32 = 116;

/// Frame insets: the content area starts at (11, 20) and is `W - 19` × `H - 58`.
const INSET_LEFT: i32 = 11;
const INSET_TOP: i32 = 20;
const INSET_RIGHT: i32 = 8;
const INSET_BOTTOM: i32 = 38;

/// Title bar height, and the 25 px corner and filler pieces along it.
const TITLE_H: i32 = 20;
const CORNER_W: i32 = 25;
/// Width of the title piece between the fillers.
const TITLE_W: i32 = 100;
/// Side tiles are 29 px high; the right one is 8 px wide. The left one is 11 px wide.
const SIDE_TILE_H: i32 = 29;
const LEFT_W: i32 = 11;
const RIGHT_W: i32 = 8;
/// Bottom bar: 38 px high, with 125 px corner pieces and a 25 px fill.
const BOTTOM_H: i32 = 38;
const BOTTOM_CORNER_W: i32 = 125;
const BOTTOM_FILL_W: i32 = 25;

/// Close button: 9 × 9, 11 px in from the right edge and 3 px down.
const CLOSE_SIZE: i32 = 9;
/// Bottom buttons: five of 15 × 18, from x 9, 15 px apart, 29 px above the bottom edge.
pub const BUTTON_COUNT: usize = 5;
const BUTTON_X: i32 = 9;
const BUTTON_STEP: i32 = 15;
const BUTTON_W: i32 = 15;
const BUTTON_H: i32 = 18;
/// Resize corner: the bottom-right 20 × 20 square.
const GRIP: i32 = 20;
/// Info text: x from 92 to W - 25, y from H - 27 to H - 13 (14 px), drawn on the playlist font.
const INFO_X: i32 = 92;
const INFO_RIGHT_PAD: i32 = 25;
const INFO_Y_FROM_BOTTOM: i32 = 27;
const INFO_H: i32 = 14;
/// Baseline of the info text, from the top of the info rectangle.
const INFO_BASELINE: i32 = 10;

/// The pressed art of the five bottom buttons, in the classic order: fullscreen, 1×, 2×,
/// TV/misc, options. Their normal art is part of `VIDEO_BOTTOM_LEFT`.
pub const BUTTON_SPRITES: [&str; BUTTON_COUNT] = [
    "VIDEO_BUTTON_FULLSCREEN_PRESSED",
    "VIDEO_BUTTON_1X_PRESSED",
    "VIDEO_BUTTON_2X_PRESSED",
    "VIDEO_BUTTON_MISC_PRESSED",
    "VIDEO_BUTTON_OPTIONS_PRESSED",
];

/// Colours for the plain bevel used when the skin has no VIDEO.BMP.
const BEVEL_FACE: u32 = 0xFF2A_3038;
const BEVEL_LIGHT: u32 = 0xFF6B_7585;
const BEVEL_DARK: u32 = 0xFF10_141A;
const BEVEL_TITLE_ON: u32 = 0xFF3B_5B7F;
const BEVEL_TITLE_OFF: u32 = 0xFF3A_4048;

/// What a point of the window belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VideoHit {
    /// The title bar, away from the close button: drags the window.
    TitleBar,
    Close,
    /// Bottom button `i` (0..[`BUTTON_COUNT`]), in the classic order.
    Button(usize),
    /// The bottom-right corner: resizes the window.
    Resize,
    /// The content area, which the webview covers.
    Content,
    /// The side or bottom frame.
    Frame,
}

/// Everything the frame needs to draw.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoWindowState {
    /// Window size in skin pixels.
    pub w: u32,
    pub h: u32,
    pub focused: bool,
    /// The control under a press that has not been released yet.
    pub pressed: Option<VideoHit>,
    /// The track shown under the video, "Artist - Title".
    pub info_text: String,
}

impl Default for VideoWindowState {
    fn default() -> Self {
        VideoWindowState {
            w: DEFAULT_W,
            h: DEFAULT_H,
            focused: false,
            pressed: None,
            info_text: String::new(),
        }
    }
}

/// `(w, h)` raised to the minimum size the frame can be drawn at.
pub fn clamp_size(w: i32, h: i32) -> (u32, u32) {
    (w.max(MIN_W as i32) as u32, h.max(MIN_H as i32) as u32)
}

/// The window size (skin pixels) that gives a content area of `cw` × `ch`.
pub fn window_for_content(cw: u32, ch: u32) -> (u32, u32) {
    (
        cw + (INSET_LEFT + INSET_RIGHT) as u32,
        ch + (INSET_TOP + INSET_BOTTOM) as u32,
    )
}

/// The content area in skin pixels: where the webview goes.
pub fn content_rect(w: u32, h: u32) -> Rect {
    Rect::new(
        INSET_LEFT,
        INSET_TOP,
        (w as i32 - INSET_LEFT - INSET_RIGHT).max(0),
        (h as i32 - INSET_TOP - INSET_BOTTOM).max(0),
    )
}

fn close_rect(w: i32) -> Rect {
    Rect::new(w - 11, 3, CLOSE_SIZE, CLOSE_SIZE)
}

fn button_rect(h: i32, i: usize) -> Rect {
    Rect::new(
        BUTTON_X + BUTTON_STEP * i as i32,
        h - 29,
        BUTTON_W,
        BUTTON_H,
    )
}

fn resize_rect(w: i32, h: i32) -> Rect {
    Rect::new(w - GRIP, h - GRIP, GRIP, GRIP)
}

/// The info text rectangle: x 92 to W - 25, y H - 27 to H - 13.
fn info_rect(w: i32, h: i32) -> Rect {
    Rect::new(
        INFO_X,
        h - INFO_Y_FROM_BOTTOM,
        (w - INFO_RIGHT_PAD - INFO_X).max(0),
        INFO_H,
    )
}

/// Which part of the frame is at window point `(x, y)`, or `None` outside the window.
pub fn hit_test_video(x: i32, y: i32, w: u32, h: u32) -> Option<VideoHit> {
    let (w, h) = (w as i32, h as i32);
    if x < 0 || y < 0 || x >= w || y >= h {
        return None;
    }
    if close_rect(w).contains(x, y) {
        return Some(VideoHit::Close);
    }
    for i in 0..BUTTON_COUNT {
        if button_rect(h, i).contains(x, y) {
            return Some(VideoHit::Button(i));
        }
    }
    if resize_rect(w, h).contains(x, y) {
        return Some(VideoHit::Resize);
    }
    if y < TITLE_H {
        return Some(VideoHit::TitleBar);
    }
    if content_rect(w as u32, h as u32).contains(x, y) {
        return Some(VideoHit::Content);
    }
    Some(VideoHit::Frame)
}

/// The pieces of the title bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TitlePart {
    Left,
    Fill,
    Title,
    Right,
}

/// One title bar piece: where it goes, how wide it is, and whether it is stretched to that width
/// (only the remainder filler is) or cut from the sprite.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tile {
    pub part: TitlePart,
    pub x: i32,
    pub w: i32,
    pub stretch: bool,
}

/// The title bar for a window `w` px wide (at least 150). The fillers are `(w - 150) / 25` pieces
/// of 25 px, split evenly on both sides of the title. When that count is odd, the left side gets
/// a 12 px slice and the right side a 13 px slice. The remaining `(w - 150) % 25` px become a
/// stretched filler just before the right corner. The pieces cover `0..w` without gaps.
pub fn title_layout(w: i32) -> Vec<Tile> {
    let span = (w - 2 * CORNER_W - TITLE_W).max(0);
    let count = span / CORNER_W;
    let remainder = span % CORNER_W;
    let half = count / 2;
    let odd = count % 2 == 1;

    let mut tiles = Vec::new();
    let mut x = 0;
    let mut push = |tiles: &mut Vec<Tile>, part, width, stretch| {
        tiles.push(Tile {
            part,
            x,
            w: width,
            stretch,
        });
        x += width;
    };
    push(&mut tiles, TitlePart::Left, CORNER_W, false);
    for _ in 0..half {
        push(&mut tiles, TitlePart::Fill, CORNER_W, false);
    }
    if odd {
        push(&mut tiles, TitlePart::Fill, 12, false);
    }
    push(&mut tiles, TitlePart::Title, TITLE_W, false);
    if odd {
        push(&mut tiles, TitlePart::Fill, 13, false);
    }
    for _ in 0..half {
        push(&mut tiles, TitlePart::Fill, CORNER_W, false);
    }
    if remainder > 0 {
        push(&mut tiles, TitlePart::Fill, remainder, true);
    }
    push(&mut tiles, TitlePart::Right, CORNER_W, false);
    tiles
}

/// One run of a tiled side or bottom: an offset along the run, a length, and whether the run is
/// stretched (the remainder) rather than a whole tile.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment {
    pub offset: i32,
    pub len: i32,
    pub stretch: bool,
}

/// Splits `len` px into whole `tile`-px tiles, then one stretched run for what is left.
pub fn tile_segments(len: i32, tile: i32) -> Vec<Segment> {
    let mut out = Vec::new();
    if len <= 0 || tile <= 0 {
        return out;
    }
    let full = len / tile;
    let rest = len % tile;
    for k in 0..full {
        out.push(Segment {
            offset: k * tile,
            len: tile,
            stretch: false,
        });
    }
    if rest > 0 {
        out.push(Segment {
            offset: full * tile,
            len: rest,
            stretch: true,
        });
    }
    out
}

fn part_name(part: TitlePart, focused: bool) -> &'static str {
    match (part, focused) {
        (TitlePart::Left, true) => "VIDEO_TITLE_LEFT",
        (TitlePart::Left, false) => "VIDEO_TITLE_LEFT_INACTIVE",
        (TitlePart::Fill, true) => "VIDEO_TITLE_FILL",
        (TitlePart::Fill, false) => "VIDEO_TITLE_FILL_INACTIVE",
        (TitlePart::Title, true) => "VIDEO_TITLE",
        (TitlePart::Title, false) => "VIDEO_TITLE_INACTIVE",
        (TitlePart::Right, true) => "VIDEO_TITLE_RIGHT",
        (TitlePart::Right, false) => "VIDEO_TITLE_RIGHT_INACTIVE",
    }
}

/// Draws the sprite into `rect`: cut from its top-left when it fits, or stretched to the rect.
fn draw_piece(canvas: &mut Canvas, sv: &SpriteView<'_>, rect: Rect, stretch: bool) {
    if rect.w <= 0 || rect.h <= 0 {
        return;
    }
    if stretch {
        canvas.blit_scaled(sv, rect.x, rect.y, rect.w as u32, rect.h as u32);
    } else {
        canvas.blit_region(sv, (0, 0, rect.w as u32, rect.h as u32), rect.x, rect.y);
    }
}

/// Draws the named VIDEO sprite into `rect` (see [`draw_piece`]), if the skin has it.
fn draw_named(skin: &Skin, canvas: &mut Canvas, name: &str, rect: Rect, stretch: bool) {
    if let Some(sv) = skin.sprite(Sheet::Video, name) {
        draw_piece(canvas, &sv, rect, stretch);
    }
}

/// Draws the whole frame and the info text into `canvas`, which must be `st.w` × `st.h`. The
/// content area is left as it was.
pub fn render_video_frame(skin: &Skin, st: &VideoWindowState, canvas: &mut Canvas) {
    if skin.sheet(Sheet::Video).is_some() {
        draw_sprite_frame(skin, st, canvas);
    } else {
        draw_plain_frame(skin, st, canvas);
    }
    draw_info(skin, st, canvas);
}

fn draw_sprite_frame(skin: &Skin, st: &VideoWindowState, canvas: &mut Canvas) {
    let (w, h) = (st.w as i32, st.h as i32);

    for tile in title_layout(w) {
        let name = part_name(tile.part, st.focused);
        draw_named(
            skin,
            canvas,
            name,
            Rect::new(tile.x, 0, tile.w, TITLE_H),
            tile.stretch,
        );
    }

    // Sides run from the title bar down to the bottom bar.
    for seg in tile_segments(h - TITLE_H - BOTTOM_H, SIDE_TILE_H) {
        let y = TITLE_H + seg.offset;
        draw_named(
            skin,
            canvas,
            "VIDEO_LEFT",
            Rect::new(0, y, LEFT_W, seg.len),
            seg.stretch,
        );
        draw_named(
            skin,
            canvas,
            "VIDEO_RIGHT",
            Rect::new(w - RIGHT_W, y, RIGHT_W, seg.len),
            seg.stretch,
        );
    }

    // Bottom bar: the left piece carries the normal buttons, the fill runs between the corners.
    let by = h - BOTTOM_H;
    draw_named(
        skin,
        canvas,
        "VIDEO_BOTTOM_LEFT",
        Rect::new(0, by, BOTTOM_CORNER_W, BOTTOM_H),
        false,
    );
    for seg in tile_segments(w - 2 * BOTTOM_CORNER_W, BOTTOM_FILL_W) {
        let rect = Rect::new(BOTTOM_CORNER_W + seg.offset, by, seg.len, BOTTOM_H);
        draw_named(skin, canvas, "VIDEO_BOTTOM_FILL", rect, seg.stretch);
    }
    draw_named(
        skin,
        canvas,
        "VIDEO_BOTTOM_RIGHT",
        Rect::new(w - BOTTOM_CORNER_W, by, BOTTOM_CORNER_W, BOTTOM_H),
        false,
    );

    // Pressed controls. The normal close button is part of the title bar art.
    if st.pressed == Some(VideoHit::Close) {
        draw_named(skin, canvas, "VIDEO_CLOSE_PRESSED", close_rect(w), false);
    }
    if let Some(VideoHit::Button(i)) = st.pressed {
        draw_named(skin, canvas, BUTTON_SPRITES[i], button_rect(h, i), false);
    }
}

/// The fallback frame for a skin without VIDEO.BMP: a flat bevel with the same layout, so the
/// controls stay where they are.
fn draw_plain_frame(skin: &Skin, st: &VideoWindowState, canvas: &mut Canvas) {
    let (w, h) = (st.w as i32, st.h as i32);
    canvas.fill(Rect::new(0, 0, w, h), BEVEL_FACE);
    let title = if st.focused {
        BEVEL_TITLE_ON
    } else {
        BEVEL_TITLE_OFF
    };
    canvas.fill(Rect::new(0, 0, w, TITLE_H), title);
    bevel(canvas, Rect::new(0, 0, w, h), BEVEL_LIGHT, BEVEL_DARK);
    // A sunken well around the content area.
    let c = content_rect(st.w, st.h);
    bevel(
        canvas,
        Rect::new(c.x - 1, c.y - 1, c.w + 2, c.h + 2),
        BEVEL_DARK,
        BEVEL_LIGHT,
    );
    canvas.fill(c, 0xFF00_0000);
    draw_text(
        canvas,
        skin,
        "VIDEO",
        INSET_LEFT,
        (TITLE_H - 6) / 2,
        Rect::new(0, 0, w, TITLE_H),
    );

    let close = close_rect(w);
    let pressed = st.pressed == Some(VideoHit::Close);
    plain_box(canvas, close, pressed);
    for i in 0..BUTTON_COUNT {
        let r = button_rect(h, i);
        plain_box(canvas, r, st.pressed == Some(VideoHit::Button(i)));
    }
}

/// A 1 px raised border: `light` on the top and left, `dark` on the bottom and right.
fn bevel(canvas: &mut Canvas, r: Rect, light: u32, dark: u32) {
    canvas.fill(Rect::new(r.x, r.y, r.w, 1), light);
    canvas.fill(Rect::new(r.x, r.y, 1, r.h), light);
    canvas.fill(Rect::new(r.x, r.y + r.h - 1, r.w, 1), dark);
    canvas.fill(Rect::new(r.x + r.w - 1, r.y, 1, r.h), dark);
}

/// A bevelled button for the fallback frame. Pressed, it is drawn inverted.
fn plain_box(canvas: &mut Canvas, r: Rect, pressed: bool) {
    let (light, dark) = if pressed {
        (BEVEL_DARK, BEVEL_LIGHT)
    } else {
        (BEVEL_LIGHT, BEVEL_DARK)
    };
    canvas.fill(r, BEVEL_FACE);
    bevel(canvas, r, light, dark);
}

fn draw_info(skin: &Skin, st: &VideoWindowState, canvas: &mut Canvas) {
    let rect = info_rect(st.w as i32, st.h as i32);
    canvas.fill(rect, skin.pledit.normal_bg);
    draw_track_text(
        canvas,
        &st.info_text,
        rect.x,
        rect.y + INFO_BASELINE,
        skin.pledit.normal,
        rect,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tubeamp_skin::testkit;

    fn px(c: &Canvas, x: i32, y: i32) -> u32 {
        c.px[(y as u32 * c.w + x as u32) as usize]
    }

    fn render(skin: &Skin, st: &VideoWindowState) -> Canvas {
        let mut c = Canvas::new(st.w, st.h);
        render_video_frame(skin, st, &mut c);
        c
    }

    fn state(w: u32, h: u32) -> VideoWindowState {
        VideoWindowState {
            w,
            h,
            focused: true,
            pressed: None,
            info_text: String::new(),
        }
    }

    /// The covered pieces must be contiguous from 0 to `w`, and the title must sit between the
    /// fillers as Webamp's layout does.
    #[test]
    fn title_layout_covers_the_width_for_several_widths() {
        for w in [150, 275, 276, 300, 301, 349, 350, 351, 400, 1000] {
            let tiles = title_layout(w);
            let mut x = 0;
            for t in &tiles {
                assert_eq!(t.x, x, "width {w}: gap before {t:?}");
                x += t.w;
            }
            assert_eq!(x, w, "width {w}: pieces end at {x}");
            assert_eq!(tiles.first().map(|t| t.part), Some(TitlePart::Left));
            assert_eq!(tiles.last().map(|t| t.part), Some(TitlePart::Right));
            assert_eq!(
                tiles.last().map(|t| t.x),
                Some(w - 25),
                "right corner at W-25"
            );
            let title = tiles
                .iter()
                .find(|t| t.part == TitlePart::Title)
                .expect("title");
            assert_eq!(title.w, 100);
            assert!(tiles.iter().all(|t| t.w > 0), "no empty pieces at {w}");
        }
    }

    #[test]
    fn title_layout_at_275_is_five_fillers_with_a_12_and_13_split() {
        let tiles = title_layout(275);
        let summary: Vec<(TitlePart, i32, i32, bool)> = tiles
            .iter()
            .map(|t| (t.part, t.x, t.w, t.stretch))
            .collect();
        assert_eq!(
            summary,
            vec![
                (TitlePart::Left, 0, 25, false),
                (TitlePart::Fill, 25, 25, false),
                (TitlePart::Fill, 50, 25, false),
                (TitlePart::Fill, 75, 12, false),
                (TitlePart::Title, 87, 100, false),
                (TitlePart::Fill, 187, 13, false),
                (TitlePart::Fill, 200, 25, false),
                (TitlePart::Fill, 225, 25, false),
                (TitlePart::Right, 250, 25, false),
            ]
        );
    }

    #[test]
    fn title_layout_stretches_the_remainder_before_the_right_corner() {
        // (349 - 150) = 199 = 7 * 25 + 24: the 24 px remainder is stretched.
        let tiles = title_layout(349);
        let stretched: Vec<&Tile> = tiles.iter().filter(|t| t.stretch).collect();
        assert_eq!(stretched.len(), 1);
        assert_eq!((stretched[0].x, stretched[0].w), (300, 24));
        assert_eq!(tiles[tiles.len() - 2].part, TitlePart::Fill);
        // Even counts have no 12/13 split.
        assert!(title_layout(300).iter().all(|t| t.w != 12 && t.w != 13));
    }

    #[test]
    fn side_and_bottom_runs_are_whole_tiles_plus_a_stretched_remainder() {
        assert_eq!(
            tile_segments(174, 29),
            (0..6)
                .map(|k| Segment {
                    offset: k * 29,
                    len: 29,
                    stretch: false
                })
                .collect::<Vec<_>>()
        );
        let mixed = tile_segments(180, 29);
        assert_eq!(mixed.len(), 7);
        assert_eq!(
            mixed[6],
            Segment {
                offset: 174,
                len: 6,
                stretch: true
            }
        );
        assert!(tile_segments(0, 29).is_empty());
        assert!(tile_segments(-5, 25).is_empty());
        let fill = tile_segments(25, 25);
        assert_eq!(fill.len(), 1);
        assert!(!fill[0].stretch);
    }

    #[test]
    fn content_area_follows_the_insets() {
        assert_eq!(content_rect(275, 232), Rect::new(11, 20, 256, 174));
        assert_eq!(content_rect(350, 260), Rect::new(11, 20, 331, 202));
    }

    #[test]
    fn window_size_for_a_content_area_adds_the_insets() {
        // 1× and 2× video: 320 × 180 and 640 × 360 of content.
        assert_eq!(window_for_content(320, 180), (339, 238));
        assert_eq!(window_for_content(640, 360), (659, 418));
        let c = content_rect(339, 238);
        assert_eq!((c.w, c.h), (320, 180));
    }

    #[test]
    fn hit_test_finds_each_control() {
        let (w, h) = (275, 232);
        let hit = |x, y| hit_test_video(x, y, w, h);
        assert_eq!(hit(100, 5), Some(VideoHit::TitleBar));
        assert_eq!(hit(264, 3), Some(VideoHit::Close), "close button, top-left");
        assert_eq!(
            hit(272, 11),
            Some(VideoHit::Close),
            "close button, bottom-right"
        );
        assert_eq!(
            hit(263, 3),
            Some(VideoHit::TitleBar),
            "just left of the close button"
        );
        assert_eq!(hit(9, 203), Some(VideoHit::Button(0)));
        assert_eq!(hit(24, 203), Some(VideoHit::Button(1)));
        assert_eq!(hit(39, 203), Some(VideoHit::Button(2)));
        assert_eq!(hit(54, 203), Some(VideoHit::Button(3)));
        assert_eq!(hit(69, 203), Some(VideoHit::Button(4)));
        assert_eq!(
            hit(84, 203),
            Some(VideoHit::Frame),
            "just past the last button"
        );
        assert_eq!(hit(270, 225), Some(VideoHit::Resize));
        assert_eq!(
            hit(250, 225),
            Some(VideoHit::Frame),
            "bottom bar, left of the grip"
        );
        assert_eq!(hit(50, 100), Some(VideoHit::Content));
        assert_eq!(hit(5, 100), Some(VideoHit::Frame), "left side frame");
        assert_eq!(hit(-1, 5), None);
        assert_eq!(hit(275, 5), None);
        assert_eq!(hit(5, 232), None);
    }

    #[test]
    fn hit_test_follows_the_window_size() {
        // The buttons and the grip sit at the bottom of whatever size the window is.
        assert_eq!(
            hit_test_video(9, 116 - 29, 275, 116),
            Some(VideoHit::Button(0))
        );
        assert_eq!(hit_test_video(340, 310, 350, 320), Some(VideoHit::Resize));
        assert_eq!(hit_test_video(320, 310, 350, 320), Some(VideoHit::Frame));
    }

    #[test]
    fn clamp_size_never_goes_below_the_minimum() {
        assert_eq!(clamp_size(10, 10), (MIN_W, MIN_H));
        assert_eq!(clamp_size(400, 300), (400, 300));
    }

    #[test]
    fn frame_uses_the_title_sprites_for_focus_state() {
        let skin = testkit::synthetic_skin();
        let focused = render(&skin, &state(275, 232));
        let left = skin
            .sprite(Sheet::Video, "VIDEO_TITLE_LEFT")
            .expect("sprite");
        assert_eq!(px(&focused, 0, 0), left.pixel(0, 0));
        let mut unfocused_state = state(275, 232);
        unfocused_state.focused = false;
        let unfocused = render(&skin, &unfocused_state);
        let inactive = skin
            .sprite(Sheet::Video, "VIDEO_TITLE_LEFT_INACTIVE")
            .expect("sprite");
        assert_eq!(px(&unfocused, 0, 0), inactive.pixel(0, 0));
    }

    #[test]
    fn right_corner_and_bottom_pieces_sit_at_their_doc_positions() {
        let skin = testkit::synthetic_skin();
        let (w, h) = (300, 240);
        let c = render(&skin, &state(w, h));
        let right = skin
            .sprite(Sheet::Video, "VIDEO_TITLE_RIGHT")
            .expect("sprite");
        assert_eq!(px(&c, w as i32 - 25, 0), right.pixel(0, 0));
        let bottom_right = skin
            .sprite(Sheet::Video, "VIDEO_BOTTOM_RIGHT")
            .expect("sprite");
        assert_eq!(
            px(&c, w as i32 - 125, h as i32 - 38),
            bottom_right.pixel(0, 0)
        );
        let side = skin.sprite(Sheet::Video, "VIDEO_RIGHT").expect("sprite");
        assert_eq!(px(&c, w as i32 - 8, 20), side.pixel(0, 0));
    }

    #[test]
    fn content_area_is_left_untouched() {
        let skin = testkit::synthetic_skin();
        let st = state(275, 232);
        let c = render(&skin, &st);
        let content = content_rect(st.w, st.h);
        for y in content.y..content.y + content.h {
            for x in content.x..content.x + content.w {
                assert_eq!(px(&c, x, y), 0xFF00_0000, "content pixel ({x},{y})");
            }
        }
    }

    #[test]
    fn pressed_close_and_buttons_draw_their_pressed_art() {
        let skin = testkit::synthetic_skin();
        let (w, h) = (275, 232);
        let mut st = state(w, h);
        st.pressed = Some(VideoHit::Close);
        let c = render(&skin, &st);
        let close = skin
            .sprite(Sheet::Video, "VIDEO_CLOSE_PRESSED")
            .expect("sprite");
        assert_eq!(px(&c, w as i32 - 11, 3), close.pixel(0, 0));

        st.pressed = Some(VideoHit::Button(2));
        let c = render(&skin, &st);
        let two_x = skin
            .sprite(Sheet::Video, "VIDEO_BUTTON_2X_PRESSED")
            .expect("sprite");
        assert_eq!(px(&c, 39, h as i32 - 29), two_x.pixel(0, 0));
    }

    #[test]
    fn info_text_is_drawn_only_inside_its_rectangle() {
        let skin = testkit::synthetic_skin();
        let (w, h) = (275, 232);
        let mut with_text = state(w, h);
        with_text.info_text = "Luis Fonsi - Despacito".to_string();
        let plain = render(&skin, &state(w, h));
        let texted = render(&skin, &with_text);
        let rect = info_rect(w as i32, h as i32);
        let mut changed = 0;
        for y in 0..h as i32 {
            for x in 0..w as i32 {
                if px(&plain, x, y) != px(&texted, x, y) {
                    changed += 1;
                    assert!(rect.contains(x, y), "text leaked to ({x},{y})");
                }
            }
        }
        assert!(changed > 0, "the text must draw something");
    }

    #[test]
    fn a_skin_without_video_bmp_gets_the_plain_frame() {
        let skin = testkit::synthetic_skin_without(&[Sheet::Video]);
        assert!(skin.sheet(Sheet::Video).is_none());
        let c = render(&skin, &state(275, 232));
        assert_eq!(px(&c, 100, 5), BEVEL_TITLE_ON, "title bar colour");
        assert_eq!(px(&c, 100, 100), 0xFF00_0000, "content stays black");
    }

    #[test]
    fn render_survives_every_size_and_state() {
        let skin = testkit::synthetic_skin();
        let bare = testkit::synthetic_skin_without(&[Sheet::Video]);
        for w in [MIN_W, 276, 301, 400] {
            for h in [MIN_H, 117, 232, 300] {
                for skin in [&skin, &bare] {
                    for pressed in [None, Some(VideoHit::Close), Some(VideoHit::Button(4))] {
                        let st = VideoWindowState {
                            w,
                            h,
                            focused: w % 2 == 0,
                            pressed,
                            info_text: "Some artist - a rather long title that overflows".into(),
                        };
                        let _ = render(skin, &st);
                    }
                }
            }
        }
    }
}
