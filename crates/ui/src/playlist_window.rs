//! Playlist editor window (Winamp 2.x `PLEDIT`): the tiled frame at any size, track rows,
//! scrollbar, the bottom buttons with their popups, running time and mini transport. Also
//! layout, rendering and hit testing.
//!
//! Element rectangles and the tiling rules come from Webamp's `css/playlist-window.css`, the
//! sprite names from `js/skinSelectors.ts` and `js/skinSprites.ts` (MIT, © Jordan Eldredge).
//!
//! Track text is drawn with Liberation Sans (SIL OFL, bundled in `assets/fonts`), because the
//! playlist uses a TrueType font, not TEXT.BMP. Labels on the bottom buttons and the running time
//! use TEXT.BMP like Winamp does.
//!
//! Where a skin draws nothing into a bottom button (the TubeAmp default skin's corner panels are
//! plain), the button gets a text label or a small pixel icon instead, so it can still be found and
//! clicked. Skins that do draw their own buttons keep them.

use std::sync::OnceLock;

use fontdue::{Font, FontSettings};
use tubeamp_playlist::Playlist;
use tubeamp_skin::{Sheet, Skin, SpriteView};

use crate::canvas::{Canvas, Rect};
use crate::labels::time_truncated;
use crate::text::draw_text;

/// Width of the playlist window at the base size (1×), in skin pixels.
pub const BASE_WIDTH: u32 = 275;
/// Height of the playlist window at the base size (1×), in skin pixels.
pub const BASE_HEIGHT: u32 = 116;
/// Width added per horizontal resize step.
pub const STEP_W: u32 = 25;
/// Height added per vertical resize step.
pub const STEP_H: u32 = 29;
/// Largest number of resize steps in either direction (a sanity bound, not a Winamp limit).
pub const MAX_STEPS: u32 = 40;
/// The size the window opens at: 275×232, the classic default.
pub const DEFAULT_STEPS: (u32, u32) = (0, 4);

/// Title bar height.
const TITLE_H: i32 = 20;
/// Bottom bar height.
const BOTTOM_H: i32 = 38;
/// Track area top, which is also the top of the first row (the middle's 3 px padding).
const LIST_TOP: i32 = 23;
/// Left and right frame widths around the track area.
const LEFT_W: i32 = 12;
const RIGHT_W: i32 = 20;
/// Track row height.
pub const ROW_H: i32 = 13;
/// Width of a title bar corner, and of the title text sprite.
const CORNER_W: i32 = 25;
const TITLE_TEXT_W: i32 = 100;
/// Bottom bar corner widths: the left corner, then the right corner (the centre tiles between).
const BOTTOM_LEFT_W: i32 = 125;
const BOTTOM_RIGHT_W: i32 = 150;
/// Height of a scrollbar handle.
const HANDLE_H: i32 = 18;
/// Height of a bottom menu button and of a popup item.
const MENU_H: i32 = 18;
/// Width of a bottom menu button and of a popup item.
const MENU_W: i32 = 22;
/// Pixel size of the track font (Winamp's playlist uses 9 px).
const FONT_PX: f32 = 9.0;
/// Coverage at or above this draws a pixel of the glyph: crisp, bitmap-like text.
const GLYPH_THRESHOLD: u8 = 100;
/// Liberation Sans Regular, SIL Open Font License (see `assets/fonts/`).
const FONT_BYTES: &[u8] = include_bytes!("../assets/fonts/LiberationSans-Regular.ttf");

/// The five bottom menu buttons, in left-to-right order (ADD, REM, SEL, MISC, LIST).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlMenu {
    Add,
    Remove,
    Select,
    Misc,
    List,
}

impl PlMenu {
    pub const ALL: [PlMenu; 5] = [
        PlMenu::Add,
        PlMenu::Remove,
        PlMenu::Select,
        PlMenu::Misc,
        PlMenu::List,
    ];

    /// Number of items in the popup.
    pub fn len(self) -> usize {
        match self {
            PlMenu::Add | PlMenu::Select | PlMenu::Misc | PlMenu::List => 3,
            PlMenu::Remove => 4,
        }
    }

    /// Whether the popup has no items. Never true; present to satisfy `len_without_is_empty`.
    pub fn is_empty(self) -> bool {
        self.len() == 0
    }

    /// Popup item sprites as `(normal, selected)` names, top to bottom.
    fn items(self) -> &'static [(&'static str, &'static str)] {
        match self {
            PlMenu::Add => &[
                ("PLAYLIST_ADD_URL", "PLAYLIST_ADD_URL_SELECTED"),
                ("PLAYLIST_ADD_DIR", "PLAYLIST_ADD_DIR_SELECTED"),
                ("PLAYLIST_ADD_FILE", "PLAYLIST_ADD_FILE_SELECTED"),
            ],
            PlMenu::Remove => &[
                ("PLAYLIST_REMOVE_ALL", "PLAYLIST_REMOVE_ALL_SELECTED"),
                ("PLAYLIST_CROP", "PLAYLIST_CROP_SELECTED"),
                (
                    "PLAYLIST_REMOVE_SELECTED",
                    "PLAYLIST_REMOVE_SELECTED_SELECTED",
                ),
                ("PLAYLIST_REMOVE_MISC", "PLAYLIST_REMOVE_MISC_SELECTED"),
            ],
            PlMenu::Select => &[
                (
                    "PLAYLIST_INVERT_SELECTION",
                    "PLAYLIST_INVERT_SELECTION_SELECTED",
                ),
                ("PLAYLIST_SELECT_ZERO", "PLAYLIST_SELECT_ZERO_SELECTED"),
                ("PLAYLIST_SELECT_ALL", "PLAYLIST_SELECT_ALL_SELECTED"),
            ],
            PlMenu::Misc => &[
                ("PLAYLIST_SORT_LIST", "PLAYLIST_SORT_LIST_SELECTED"),
                ("PLAYLIST_FILE_INFO", "PLAYLIST_FILE_INFO_SELECTED"),
                ("PLAYLIST_MISC_OPTIONS", "PLAYLIST_MISC_OPTIONS_SELECTED"),
            ],
            PlMenu::List => &[
                ("PLAYLIST_NEW_LIST", "PLAYLIST_NEW_LIST_SELECTED"),
                ("PLAYLIST_SAVE_LIST", "PLAYLIST_SAVE_LIST_SELECTED"),
                ("PLAYLIST_LOAD_LIST", "PLAYLIST_LOAD_LIST_SELECTED"),
            ],
        }
    }

    /// Name of the popup's bar sprite (the vertical strip at its left edge).
    fn bar(self) -> &'static str {
        match self {
            PlMenu::Add => "PLAYLIST_ADD_MENU_BAR",
            PlMenu::Remove => "PLAYLIST_REMOVE_MENU_BAR",
            PlMenu::Select => "PLAYLIST_SELECT_MENU_BAR",
            PlMenu::Misc => "PLAYLIST_MISC_MENU_BAR",
            PlMenu::List => "PLAYLIST_LIST_BAR",
        }
    }

    /// Label for the fallback face.
    fn label(self) -> &'static str {
        match self {
            PlMenu::Add => "ADD",
            PlMenu::Remove => "REM",
            PlMenu::Select => "SEL",
            PlMenu::Misc => "MISC",
            PlMenu::List => "LIST",
        }
    }
}

/// The mini transport buttons under the running time, left to right.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlTransport {
    Previous,
    Play,
    Pause,
    Stop,
    Next,
    Eject,
}

impl PlTransport {
    pub const ALL: [PlTransport; 6] = [
        PlTransport::Previous,
        PlTransport::Play,
        PlTransport::Pause,
        PlTransport::Stop,
        PlTransport::Next,
        PlTransport::Eject,
    ];
}

/// What a point on the playlist window belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlHit {
    /// The title bar, away from its buttons: drags the window.
    TitleBar,
    Close,
    Shade,
    /// The bottom-right corner: resizes the window.
    Resize,
    Scrollbar,
    /// A track row, by playlist index (scroll already applied).
    Row(usize),
    /// The track area below the last row, or past the end of the list.
    List,
    Menu(PlMenu),
    /// An item of an open popup: `(menu, item index from the top)`.
    MenuItem(PlMenu, usize),
    Transport(PlTransport),
    /// The mini time: a click toggles elapsed and remaining time.
    MiniTime,
}

/// Everything the playlist window shows apart from the playlist itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistWindowState {
    /// Resize steps from the base size: width `275 + 25 × sx`, height `116 + 29 × sy`.
    pub size_steps: (u32, u32),
    /// Index of the first visible row.
    pub scroll: usize,
    pub focused: bool,
    /// The control held down.
    pub pressed: Option<PlHit>,
    /// The control under the pointer; a popup item shows its selected sprite.
    pub hover: Option<PlHit>,
    /// The popup that is open, if any.
    pub open_menu: Option<PlMenu>,
    /// Seconds for the mini time, or `None` to show nothing (stopped).
    pub mini_time_secs: Option<u32>,
    /// Shows the mini time as time remaining, with a minus sign.
    pub mini_remaining: bool,
}

impl Default for PlaylistWindowState {
    fn default() -> Self {
        PlaylistWindowState {
            size_steps: DEFAULT_STEPS,
            scroll: 0,
            focused: false,
            pressed: None,
            hover: None,
            open_menu: None,
            mini_time_secs: None,
            mini_remaining: false,
        }
    }
}

/// Window size in skin pixels for resize steps `(sx, sy)`.
pub fn window_size(steps: (u32, u32)) -> (u32, u32) {
    (
        BASE_WIDTH + STEP_W * steps.0,
        BASE_HEIGHT + STEP_H * steps.1,
    )
}

/// Resize steps for a window of `w` × `h` skin pixels, rounded to the nearest step and never
/// below the base size.
pub fn steps_for_size(w: i32, h: i32) -> (u32, u32) {
    let sx = ((w - BASE_WIDTH as i32) as f64 / f64::from(STEP_W)).round();
    let sy = ((h - BASE_HEIGHT as i32) as f64 / f64::from(STEP_H)).round();
    (
        sx.clamp(0.0, f64::from(MAX_STEPS)) as u32,
        sy.clamp(0.0, f64::from(MAX_STEPS)) as u32,
    )
}

/// Track rows that fit in a window with these steps.
pub fn visible_rows(steps: (u32, u32)) -> usize {
    Layout::new(steps).rows()
}

/// `scroll`, kept so that a full page of rows still fits.
pub fn clamp_scroll(scroll: usize, len: usize, rows: usize) -> usize {
    scroll.min(len.saturating_sub(rows))
}

/// The scroll offset that brings row `index` into view, moving as little as possible.
pub fn scroll_to_show(scroll: usize, index: usize, len: usize, rows: usize) -> usize {
    let scroll = clamp_scroll(scroll, len, rows);
    if rows == 0 {
        0
    } else if index < scroll {
        index
    } else if index >= scroll + rows {
        index + 1 - rows
    } else {
        scroll
    }
}

/// The playlist index of the row under window `y` (clamped to the visible rows and to the list),
/// for dragging rows. `None` only when the list is empty.
pub fn row_index_at(steps: (u32, u32), scroll: usize, len: usize, y: f64) -> Option<usize> {
    if len == 0 {
        return None;
    }
    let layout = Layout::new(steps);
    let rows = layout.rows().max(1);
    let row = ((y - f64::from(LIST_TOP)) / f64::from(ROW_H))
        .floor()
        .clamp(0.0, (rows - 1) as f64) as usize;
    Some((clamp_scroll(scroll, len, rows) + row).min(len - 1))
}

/// Where the popup button for `menu` sits, in skin pixels (its top-left and size).
pub fn menu_button_rect(steps: (u32, u32), menu: PlMenu) -> Rect {
    Layout::new(steps).menu(menu)
}

/// The scroll offset for a pointer at window `y` on the scrollbar, with the thumb centred on
/// the pointer.
pub fn scroll_at(steps: (u32, u32), len: usize, y: i32) -> usize {
    let layout = Layout::new(steps);
    let max = len.saturating_sub(layout.rows());
    if max == 0 {
        return 0;
    }
    let track = layout.scrollbar();
    let travel = f64::from((track.h - HANDLE_H).max(1));
    let rel = f64::from((y - track.y - HANDLE_H / 2).clamp(0, track.h - HANDLE_H));
    (rel / travel * max as f64).round() as usize
}

/// Geometry of the window at some resize steps, in skin pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Layout {
    w: i32,
    h: i32,
}

impl Layout {
    fn new(steps: (u32, u32)) -> Layout {
        let (w, h) = window_size(steps);
        Layout {
            w: w as i32,
            h: h as i32,
        }
    }

    /// Top of the bottom bar.
    fn bottom_y(self) -> i32 {
        self.h - BOTTOM_H
    }

    /// Bottom of the track area: the bottom bar less its 3 px of padding.
    fn list_bottom(self) -> i32 {
        self.h - BOTTOM_H - 3
    }

    fn list(self) -> Rect {
        Rect::new(
            LEFT_W,
            LIST_TOP,
            self.w - LEFT_W - RIGHT_W,
            self.list_bottom() - LIST_TOP,
        )
    }

    fn rows(self) -> usize {
        ((self.list_bottom() - LIST_TOP) / ROW_H).max(0) as usize
    }

    /// The scrollbar track: 8 px wide, 5 px in from the right edge of the frame.
    fn scrollbar(self) -> Rect {
        Rect::new(
            self.w - RIGHT_W + 5,
            LIST_TOP,
            8,
            self.list_bottom() - LIST_TOP,
        )
    }

    fn close(self) -> Rect {
        Rect::new(self.w - 11, 3, 9, 9)
    }

    fn shade(self) -> Rect {
        Rect::new(self.w - 21, 3, 9, 9)
    }

    fn resize(self) -> Rect {
        Rect::new(self.w - 20, self.h - 20, 20, 20)
    }

    /// The bottom-right corner panel's left edge.
    fn right_corner_x(self) -> i32 {
        self.w - BOTTOM_RIGHT_W
    }

    /// A bottom menu button. Its popup items stack upward from its bottom edge.
    fn menu(self, menu: PlMenu) -> Rect {
        let x = match menu {
            PlMenu::Add => 14,
            PlMenu::Remove => 43,
            PlMenu::Select => 72,
            PlMenu::Misc => 101,
            PlMenu::List => self.w - 44,
        };
        Rect::new(x, self.h - 30, MENU_W, MENU_H)
    }

    /// Item `i` of a popup, from the top.
    fn menu_item(self, menu: PlMenu, i: usize) -> Rect {
        let button = self.menu(menu);
        let n = menu.len() as i32;
        Rect::new(
            button.x,
            self.h - 12 - MENU_H * n + MENU_H * i as i32,
            MENU_W,
            MENU_H,
        )
    }

    /// The popup's left bar, which hangs 3 px left of the button.
    fn menu_bar(self, menu: PlMenu) -> Rect {
        let button = self.menu(menu);
        let h = MENU_H * menu.len() as i32;
        Rect::new(button.x - 3, self.h - 12 - h, 3, h)
    }

    fn transport(self, t: PlTransport) -> Rect {
        let index = PlTransport::ALL.iter().position(|&x| x == t).unwrap_or(0) as i32;
        Rect::new(self.right_corner_x() + 3 + 10 * index, self.h - 16, 10, 10)
    }

    fn running_time(self) -> (i32, i32) {
        (self.right_corner_x() + 7, self.h - 28)
    }

    fn mini_time(self) -> Rect {
        Rect::new(self.right_corner_x() + 66, self.h - 15, 25, 6)
    }
}

/// Which element (if any) is at window point `(x, y)`. `scroll` and `len` are the list's, and
/// `open_menu` is the open popup, whose items are checked first.
pub fn hit_test_playlist(
    steps: (u32, u32),
    scroll: usize,
    len: usize,
    open_menu: Option<PlMenu>,
    x: i32,
    y: i32,
) -> Option<PlHit> {
    let l = Layout::new(steps);
    if x < 0 || y < 0 || x >= l.w || y >= l.h {
        return None;
    }
    let at = |r: Rect| r.contains(x, y);
    if let Some(menu) = open_menu {
        for i in 0..menu.len() {
            if at(l.menu_item(menu, i)) {
                return Some(PlHit::MenuItem(menu, i));
            }
        }
    }
    if at(l.close()) {
        return Some(PlHit::Close);
    }
    if at(l.shade()) {
        return Some(PlHit::Shade);
    }
    if at(l.resize()) {
        return Some(PlHit::Resize);
    }
    for menu in PlMenu::ALL {
        if at(l.menu(menu)) {
            return Some(PlHit::Menu(menu));
        }
    }
    for t in PlTransport::ALL {
        if at(l.transport(t)) {
            return Some(PlHit::Transport(t));
        }
    }
    if at(l.mini_time()) {
        return Some(PlHit::MiniTime);
    }
    if at(l.scrollbar()) {
        return Some(PlHit::Scrollbar);
    }
    if at(l.list()) {
        let row = ((y - LIST_TOP) / ROW_H) as usize;
        let index = clamp_scroll(scroll, len, l.rows()) + row;
        return Some(if index < len && row < l.rows() {
            PlHit::Row(index)
        } else {
            PlHit::List
        });
    }
    (y < TITLE_H).then_some(PlHit::TitleBar)
}

// Text.

struct Face {
    /// One rasterised glyph per Latin-1 code point (0..=255).
    glyphs: Vec<Glyph>,
}

#[derive(Clone)]
struct Glyph {
    advance: f32,
    xmin: i32,
    ymin: i32,
    w: usize,
    h: usize,
    coverage: Vec<u8>,
}

fn face() -> &'static Face {
    static FACE: OnceLock<Face> = OnceLock::new();
    FACE.get_or_init(|| {
        let font = Font::from_bytes(FONT_BYTES, FontSettings::default())
            .expect("the bundled Liberation Sans parses");
        let glyphs = (0u8..=255)
            .map(|byte| {
                let (m, coverage) = font.rasterize(char::from(byte), FONT_PX);
                Glyph {
                    advance: m.advance_width,
                    xmin: m.xmin,
                    ymin: m.ymin,
                    w: m.width,
                    h: m.height,
                    coverage,
                }
            })
            .collect();
        Face { glyphs }
    })
}

/// The glyph for `c`. Characters outside Latin-1 draw as `?`.
fn glyph_for(c: char) -> &'static Glyph {
    let code = if (c as u32) < 256 {
        c as usize
    } else {
        '?' as usize
    };
    &face().glyphs[code]
}

/// Width of `text` in pixels at the track font size.
fn text_width(text: &str) -> f32 {
    text.chars().map(|c| glyph_for(c).advance).sum()
}

/// `text`, cut down with an ellipsis so it fits in `max_w` pixels.
fn fit_text(text: &str, max_w: f32) -> String {
    if text_width(text) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (0..chars.len()).rev() {
        let candidate: String = chars[..keep].iter().collect::<String>() + "...";
        if text_width(&candidate) <= max_w {
            return candidate;
        }
    }
    String::new()
}

/// Draws `text` with its baseline at `baseline`, starting at `x`, clipped to `clip`.
fn draw_track_text(canvas: &mut Canvas, text: &str, x: i32, baseline: i32, color: u32, clip: Rect) {
    let mut pen = x as f32;
    for c in text.chars() {
        let g = glyph_for(c);
        let gx = pen.round() as i32 + g.xmin;
        let gy = baseline - g.ymin - g.h as i32;
        for row in 0..g.h {
            for col in 0..g.w {
                if g.coverage[row * g.w + col] < GLYPH_THRESHOLD {
                    continue;
                }
                let px = gx + col as i32;
                let py = gy + row as i32;
                if clip.contains(px, py)
                    && px >= 0
                    && py >= 0
                    && (px as u32) < canvas.w
                    && (py as u32) < canvas.h
                {
                    canvas.px[(py as u32 * canvas.w + px as u32) as usize] = color;
                }
            }
        }
        pen += g.advance;
    }
}

// Drawing.

/// Draws the playlist window at the size of `st.size_steps`. `canvas` must be
/// [`window_size`] in size.
pub fn render_playlist(skin: &Skin, pl: &Playlist, st: &PlaylistWindowState, canvas: &mut Canvas) {
    let l = Layout::new(st.size_steps);
    let rows = l.rows();
    let scroll = clamp_scroll(st.scroll, pl.len(), rows);

    draw_title_bar(skin, l, st, canvas);
    // Side frames, from the title bar down to the bottom bar.
    tile_y(skin, canvas, "PLAYLIST_LEFT_TILE", 0, TITLE_H, l.bottom_y());
    tile_y(
        skin,
        canvas,
        "PLAYLIST_RIGHT_TILE",
        l.w - RIGHT_W,
        TITLE_H,
        l.bottom_y(),
    );
    draw_bottom_bar(skin, l, st, pl, canvas);
    draw_title_buttons(skin, l, st, canvas);

    canvas.fill(l.list(), skin.pledit.normal_bg);
    draw_rows(skin, l, pl, scroll, canvas);
    draw_scrollbar(skin, l, st, pl.len(), scroll, canvas);

    // Popups last, so they cover the list and the bottom buttons.
    if let Some(menu) = st.open_menu {
        draw_popup(skin, l, menu, st.hover, canvas);
    }
}

/// Draws a sprite from PLEDIT at `(x, y)`, if the skin has it.
fn blit_pledit(skin: &Skin, canvas: &mut Canvas, name: &str, x: i32, y: i32) {
    if let Some(view) = skin.sprite(Sheet::Pledit, name) {
        canvas.blit(&view, x, y);
    }
}

/// Repeats a PLEDIT sprite horizontally across `[x0, x1)`. With `align_right` the tiles line up
/// on the right edge, as Webamp's `background-position: right` does.
fn tile_x(
    skin: &Skin,
    canvas: &mut Canvas,
    name: &str,
    x0: i32,
    x1: i32,
    y: i32,
    align_right: bool,
) {
    let Some(view) = skin.sprite(Sheet::Pledit, name) else {
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

/// Repeats a PLEDIT sprite vertically across `[y0, y1)` at column `x`, top-aligned.
fn tile_y(skin: &Skin, canvas: &mut Canvas, name: &str, x: i32, y0: i32, y1: i32) {
    let Some(view) = skin.sprite(Sheet::Pledit, name) else {
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

/// The title bar: corners, the tiled fills around the title, and the title text sprite. Layout
/// follows Webamp: the spacers exist only at even widths, so the title stays centred.
fn draw_title_bar(skin: &Skin, l: Layout, st: &PlaylistWindowState, canvas: &mut Canvas) {
    let selected = st.focused;
    let pick = |normal: &'static str, active: &'static str| if selected { active } else { normal };
    blit_pledit(
        skin,
        canvas,
        pick("PLAYLIST_TOP_LEFT_CORNER", "PLAYLIST_TOP_LEFT_SELECTED"),
        0,
        0,
    );
    blit_pledit(
        skin,
        canvas,
        pick(
            "PLAYLIST_TOP_RIGHT_CORNER",
            "PLAYLIST_TOP_RIGHT_CORNER_SELECTED",
        ),
        l.w - CORNER_W,
        0,
    );
    let tile = pick("PLAYLIST_TOP_TILE", "PLAYLIST_TOP_TILE_SELECTED");

    let even = st.size_steps.0.is_multiple_of(2);
    let (spacer_l, spacer_r) = if even { (12, 13) } else { (0, 0) };
    let remaining = l.w - 2 * CORNER_W - TITLE_TEXT_W - spacer_l - spacer_r;
    let fill_l = remaining / 2;

    let mut x = CORNER_W;
    tile_x(skin, canvas, tile, x, x + spacer_l, 0, false);
    x += spacer_l;
    tile_x(skin, canvas, tile, x, x + fill_l, 0, true);
    x += fill_l;
    blit_pledit(
        skin,
        canvas,
        pick("PLAYLIST_TITLE_BAR", "PLAYLIST_TITLE_BAR_SELECTED"),
        x,
        0,
    );
    x += TITLE_TEXT_W;
    tile_x(skin, canvas, tile, x, x + spacer_r, 0, false);
    x += spacer_r;
    tile_x(skin, canvas, tile, x, l.w - CORNER_W, 0, true);
}

/// Close and shade buttons. A focused window uses the skin's PLEDIT glyphs, an unfocused one the
/// main window's title bar buttons, as Winamp's skins do.
fn draw_title_buttons(skin: &Skin, l: Layout, st: &PlaylistWindowState, canvas: &mut Canvas) {
    let close = if st.pressed == Some(PlHit::Close) {
        "MAIN_CLOSE_BUTTON_DEPRESSED"
    } else if st.focused {
        "PLAYLIST_CLOSE_SELECTED"
    } else {
        "MAIN_CLOSE_BUTTON"
    };
    let close_rect = l.close();
    draw_named(skin, canvas, close, close_rect);

    let shade = if st.pressed == Some(PlHit::Shade) {
        "MAIN_SHADE_BUTTON_DEPRESSED"
    } else if st.focused {
        "PLAYLIST_COLLAPSE_SELECTED"
    } else {
        "MAIN_SHADE_BUTTON"
    };
    draw_named(skin, canvas, shade, l.shade());
}

/// Draws a named sprite at the top-left corner of `rect`. Titlebar sprites come from TITLEBAR,
/// PLEDIT sprites from PLEDIT: the name decides.
fn draw_named(skin: &Skin, canvas: &mut Canvas, name: &str, rect: Rect) {
    let sheet = if name.starts_with("MAIN_") {
        Sheet::Titlebar
    } else {
        Sheet::Pledit
    };
    if let Some(view) = skin.sprite(sheet, name) {
        canvas.blit(&view, rect.x, rect.y);
    }
}

/// The track rows: selection background, current track colour, number and title on the left,
/// the duration right-aligned.
fn draw_rows(skin: &Skin, l: Layout, pl: &Playlist, scroll: usize, canvas: &mut Canvas) {
    let colors = &skin.pledit;
    let list = l.list();
    let text_x = list.x + 1;
    let right = list.x + list.w - 3;
    for k in 0..l.rows() {
        let index = scroll + k;
        let Some(entry) = pl.get(index) else {
            break;
        };
        let y = list.y + k as i32 * ROW_H;
        if pl.is_selected(index) {
            canvas.fill(Rect::new(list.x, y, list.w, ROW_H), colors.selected_bg);
        }
        let color = if pl.current() == Some(index) {
            colors.current
        } else {
            colors.normal
        };
        let baseline = y + 10;

        let duration = entry.duration_secs.map(time_truncated).unwrap_or_default();
        let duration_x = right - text_width(&duration).round() as i32;
        let title_right = if duration.is_empty() {
            right
        } else {
            duration_x - 6
        };
        let label = pl.display_line(index).unwrap_or_default();
        let shown = fit_text(&label, (title_right - text_x).max(0) as f32);
        draw_track_text(canvas, &shown, text_x, baseline, color, list);
        if !duration.is_empty() {
            draw_track_text(canvas, &duration, duration_x, baseline, color, list);
        }
    }
}

/// The scrollbar thumb, placed by scroll position. It shows pressed while dragged.
fn draw_scrollbar(
    skin: &Skin,
    l: Layout,
    st: &PlaylistWindowState,
    len: usize,
    scroll: usize,
    canvas: &mut Canvas,
) {
    let track = l.scrollbar();
    let travel = track.h - HANDLE_H;
    let max = len.saturating_sub(l.rows());
    let offset = if max == 0 {
        0
    } else {
        (scroll as f64 / max as f64 * f64::from(travel)).round() as i32
    };
    let name = if st.pressed == Some(PlHit::Scrollbar) {
        "PLAYLIST_SCROLL_HANDLE_SELECTED"
    } else {
        "PLAYLIST_SCROLL_HANDLE"
    };
    blit_pledit(skin, canvas, name, track.x, track.y + offset);
}

/// The bottom bar: the corners and tiles, the visualiser panel when the window is wide enough,
/// the bottom buttons (or their fallback faces), the running time, the mini transport and mini
/// time.
fn draw_bottom_bar(
    skin: &Skin,
    l: Layout,
    st: &PlaylistWindowState,
    pl: &Playlist,
    canvas: &mut Canvas,
) {
    let by = l.bottom_y();
    let right_x = l.right_corner_x();
    blit_pledit(skin, canvas, "PLAYLIST_BOTTOM_LEFT_CORNER", 0, by);
    tile_x(
        skin,
        canvas,
        "PLAYLIST_BOTTOM_TILE",
        BOTTOM_LEFT_W,
        right_x,
        by,
        false,
    );
    blit_pledit(skin, canvas, "PLAYLIST_BOTTOM_RIGHT_CORNER", right_x, by);
    if st.size_steps.0 > 2 {
        blit_pledit(
            skin,
            canvas,
            "PLAYLIST_VISUALIZER_BACKGROUND",
            right_x - 75,
            by,
        );
    }

    for menu in PlMenu::ALL {
        if st.open_menu != Some(menu) {
            draw_menu_face(skin, l, menu, canvas);
        }
    }

    let (rx, ry) = l.running_time();
    let total = pl.total_secs();
    let selected = pl.selected_secs();
    let text = format!(
        "{}/{}",
        time_truncated(selected.min(u64::from(u32::MAX)) as u32),
        time_truncated(total.min(u64::from(u32::MAX)) as u32)
    );
    draw_text(
        canvas,
        skin,
        &text,
        rx,
        ry,
        Rect::new(rx, ry, BOTTOM_RIGHT_W - 14, 6),
    );

    for t in PlTransport::ALL {
        draw_transport_face(skin, l, t, canvas);
    }

    let mini = l.mini_time();
    if let Some(secs) = st.mini_time_secs {
        let sign = if st.mini_remaining { "-" } else { "" };
        let text = format!("{sign}{}", time_truncated(secs));
        draw_text(canvas, skin, &text, mini.x, mini.y, mini);
    }
}

/// Brightest luma (0..=255) that still counts as an empty panel. The TubeAmp default skin's
/// panels are about 45; the button faces and labels in classic skins are 150 and up.
const BLANK_MAX_LUMA: f32 = 96.0;

/// Whether the `rel` rectangle of a PLEDIT sprite has nothing light in it, so the skin draws no
/// button there and the fallback should. Out-of-sprite rectangles are not blank.
fn is_blank(view: &SpriteView<'_>, rel: Rect) -> bool {
    if rel.x < 0 || rel.y < 0 || rel.x + rel.w > view.w as i32 || rel.y + rel.h > view.h as i32 {
        return false;
    }
    (0..rel.h).all(|dy| {
        (0..rel.w).all(|dx| {
            let px = view.pixel((rel.x + dx) as u32, (rel.y + dy) as u32);
            luma(px) < BLANK_MAX_LUMA
        })
    })
}

/// Rec. 601 luma of a `0xAARRGGBB` pixel.
fn luma(px: u32) -> f32 {
    let channel = |shift: u32| ((px >> shift) & 0xFF) as f32;
    0.299 * channel(16) + 0.587 * channel(8) + 0.114 * channel(0)
}

/// A closed bottom button: the skin's own art if it has any, otherwise a bordered face with its
/// label in the TEXT font.
fn draw_menu_face(skin: &Skin, l: Layout, menu: PlMenu, canvas: &mut Canvas) {
    let button = l.menu(menu);
    let (corner, corner_x) = if menu == PlMenu::List {
        ("PLAYLIST_BOTTOM_RIGHT_CORNER", l.right_corner_x())
    } else {
        ("PLAYLIST_BOTTOM_LEFT_CORNER", 0)
    };
    let Some(view) = skin.sprite(Sheet::Pledit, corner) else {
        return;
    };
    // The button's offset inside the corner panel that the skin would draw it on.
    let rel = Rect::new(button.x - corner_x, button.y - l.bottom_y(), MENU_W, MENU_H);
    if !is_blank(&view, rel) {
        return;
    }
    let edge = dim(skin.pledit.normal, 0.5);
    draw_outline(canvas, button, edge);
    draw_text(
        canvas,
        skin,
        menu.label(),
        button.x + 1,
        button.y + 6,
        button,
    );
}

/// A mini transport button: the skin's art if it has any, otherwise a small pixel icon.
fn draw_transport_face(skin: &Skin, l: Layout, t: PlTransport, canvas: &mut Canvas) {
    let rect = l.transport(t);
    let Some(view) = skin.sprite(Sheet::Pledit, "PLAYLIST_BOTTOM_RIGHT_CORNER") else {
        return;
    };
    let index = PlTransport::ALL.iter().position(|&x| x == t).unwrap_or(0) as i32;
    let rel = Rect::new(3 + 10 * index, 22, 10, 10);
    if !is_blank(&view, rel) {
        return;
    }
    let color = skin.pledit.normal;
    draw_outline(canvas, rect, dim(color, 0.5));
    draw_icon(canvas, rect, t, color);
}

/// A popup: the bar on the left, then the items from the top (the hovered one in its selected
/// sprite).
fn draw_popup(skin: &Skin, l: Layout, menu: PlMenu, hover: Option<PlHit>, canvas: &mut Canvas) {
    let bar = l.menu_bar(menu);
    blit_pledit(skin, canvas, menu.bar(), bar.x, bar.y);
    for (i, (normal, selected)) in menu.items().iter().enumerate() {
        let rect = l.menu_item(menu, i);
        let name = if hover == Some(PlHit::MenuItem(menu, i)) {
            selected
        } else {
            normal
        };
        blit_pledit(skin, canvas, name, rect.x, rect.y);
    }
}

/// Outlines `rect` with one pixel of `color`.
fn draw_outline(canvas: &mut Canvas, rect: Rect, color: u32) {
    canvas.fill(Rect::new(rect.x, rect.y, rect.w, 1), color);
    canvas.fill(Rect::new(rect.x, rect.y + rect.h - 1, rect.w, 1), color);
    canvas.fill(Rect::new(rect.x, rect.y, 1, rect.h), color);
    canvas.fill(Rect::new(rect.x + rect.w - 1, rect.y, 1, rect.h), color);
}

/// `color` scaled towards black by `factor` (RGB only; the alpha is opaque).
fn dim(color: u32, factor: f32) -> u32 {
    let channel = |shift: u32| ((((color >> shift) & 0xFF) as f32) * factor) as u32;
    0xFF00_0000 | (channel(16) << 16) | (channel(8) << 8) | channel(0)
}

/// A 10×10 transport icon drawn in `color` inside `rect`.
fn draw_icon(canvas: &mut Canvas, rect: Rect, t: PlTransport, color: u32) {
    let fill = |canvas: &mut Canvas, x: i32, y: i32, w: i32, h: i32| {
        canvas.fill(Rect::new(rect.x + x, rect.y + y, w, h), color);
    };
    // Right-pointing triangle with its flat side at column `x0`: column c spans rows c..=7-c.
    let play = |canvas: &mut Canvas, x0: i32| {
        for c in 0..4 {
            fill(canvas, x0 + c, 1 + c, 1, 8 - 2 * c);
        }
    };
    // Left-pointing triangle with its flat side at column `x1`.
    let rewind = |canvas: &mut Canvas, x1: i32| {
        for c in 0..4 {
            fill(canvas, x1 - c, 1 + c, 1, 8 - 2 * c);
        }
    };
    match t {
        PlTransport::Play => play(canvas, 3),
        PlTransport::Pause => {
            fill(canvas, 2, 1, 2, 8);
            fill(canvas, 6, 1, 2, 8);
        }
        PlTransport::Stop => fill(canvas, 1, 1, 8, 8),
        PlTransport::Previous => {
            fill(canvas, 0, 1, 2, 8);
            rewind(canvas, 7);
        }
        PlTransport::Next => {
            play(canvas, 1);
            fill(canvas, 7, 1, 2, 8);
        }
        PlTransport::Eject => {
            for r in 0..4 {
                fill(canvas, 4 - r, 1 + r, 2 + 2 * r, 1);
            }
            fill(canvas, 1, 6, 8, 2);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tubeamp_playlist::Entry;
    use tubeamp_skin::testkit;

    fn playlist(n: usize) -> Playlist {
        Playlist::from_entries(
            (0..n)
                .map(|i| Entry {
                    video_id: format!("vid{i:08}"),
                    title: format!("Song number {i}"),
                    artist: format!("Artist {i}"),
                    duration_secs: Some(60 + i as u32 * 7),
                })
                .collect(),
        )
    }

    fn render(skin: &Skin, pl: &Playlist, st: &PlaylistWindowState) -> Canvas {
        let (w, h) = window_size(st.size_steps);
        let mut canvas = Canvas::new(w, h);
        render_playlist(skin, pl, st, &mut canvas);
        canvas
    }

    fn px(c: &Canvas, x: i32, y: i32) -> u32 {
        c.px[(y as u32 * c.w + x as u32) as usize]
    }

    /// Asserts canvas `(dx.., dy..)` shows the PLEDIT sprite `name` over its full size.
    fn assert_sprite_at(c: &Canvas, skin: &Skin, name: &str, dx: i32, dy: i32) {
        let view = skin.sprite(Sheet::Pledit, name).expect(name);
        assert_sprite_rows(c, skin, name, dx, dy, 0..view.h);
    }

    /// Like [`assert_sprite_at`], for the sprite rows in `rows` only (for parts that other
    /// elements draw over).
    fn assert_sprite_rows(
        c: &Canvas,
        skin: &Skin,
        name: &str,
        dx: i32,
        dy: i32,
        rows: std::ops::Range<u32>,
    ) {
        let view = skin.sprite(Sheet::Pledit, name).expect(name);
        for y in rows {
            for x in 0..view.w {
                assert_eq!(
                    px(c, dx + x as i32, dy + y as i32),
                    view.pixel(x, y),
                    "{name} at ({x},{y}) lands on canvas ({},{})",
                    dx + x as i32,
                    dy + y as i32
                );
            }
        }
    }

    #[test]
    fn sizes_and_rows_follow_the_steps() {
        assert_eq!(window_size((0, 4)), (275, 232));
        assert_eq!(window_size((2, 0)), (325, 116));
        assert_eq!(visible_rows((0, 4)), 12);
        assert_eq!(visible_rows((0, 0)), 4);
        assert_eq!(visible_rows((3, 6)), 17);
    }

    #[test]
    fn dragged_sizes_snap_to_steps_and_never_go_below_base() {
        assert_eq!(steps_for_size(275, 116), (0, 0));
        assert_eq!(steps_for_size(300, 145), (1, 1));
        assert_eq!(
            steps_for_size(311, 130),
            (1, 0),
            "rounds to the nearest step"
        );
        assert_eq!(steps_for_size(100, 50), (0, 0));
        assert_eq!(steps_for_size(100_000, 100_000), (MAX_STEPS, MAX_STEPS));
    }

    #[test]
    fn row_index_is_clamped_to_the_visible_rows_and_the_list() {
        let steps = (0, 4);
        assert_eq!(row_index_at(steps, 0, 0, 40.0), None);
        assert_eq!(row_index_at(steps, 0, 30, 23.0), Some(0));
        assert_eq!(row_index_at(steps, 0, 30, 23.0 + 13.0 * 5.5), Some(5));
        assert_eq!(
            row_index_at(steps, 4, 30, 500.0),
            Some(15),
            "below the list: the last visible row"
        );
        assert_eq!(
            row_index_at(steps, 0, 3, 150.0),
            Some(2),
            "past the end: the last entry"
        );
        assert_eq!(
            row_index_at(steps, 0, 30, -50.0),
            Some(0),
            "above the list: the first row"
        );
    }

    #[test]
    fn scroll_helpers_keep_a_full_page_and_show_the_selection() {
        assert_eq!(clamp_scroll(50, 20, 12), 8);
        assert_eq!(clamp_scroll(50, 5, 12), 0);
        assert_eq!(scroll_to_show(0, 15, 20, 12), 4);
        assert_eq!(scroll_to_show(8, 2, 20, 12), 2);
        assert_eq!(
            scroll_to_show(3, 5, 20, 12),
            3,
            "already visible: no change"
        );
        let steps = (0, 4);
        assert_eq!(scroll_at(steps, 12, 100), 0, "no overflow, no scrolling");
        let len = 30;
        let top = scroll_at(steps, len, 0);
        let bottom = scroll_at(steps, len, 1000);
        assert_eq!(top, 0);
        assert_eq!(bottom, len - visible_rows(steps));
    }

    #[test]
    fn title_bar_tiles_the_corners_and_centres_the_title() {
        let skin = testkit::synthetic_skin();
        let st = PlaylistWindowState {
            focused: true,
            ..PlaylistWindowState::default()
        };
        for steps in [(0, 4), (1, 4), (2, 4), (3, 0)] {
            let st = PlaylistWindowState {
                size_steps: steps,
                ..st.clone()
            };
            let c = render(&skin, &playlist(3), &st);
            let w = c.w as i32;
            assert_sprite_at(&c, &skin, "PLAYLIST_TOP_LEFT_SELECTED", 0, 0);
            // The close and shade buttons sit over the right corner from row 3 down.
            assert_sprite_rows(
                &c,
                &skin,
                "PLAYLIST_TOP_RIGHT_CORNER_SELECTED",
                w - 25,
                0,
                0..3,
            );
        }
        // Even width (275): corners 25, spacer 12, fill 50, title at x=87.
        let c = render(&skin, &playlist(3), &st);
        assert_sprite_at(&c, &skin, "PLAYLIST_TITLE_BAR_SELECTED", 87, 0);
        // Odd width (300): no spacers, the fills take 75 px each, so the title is at x=100.
        // Both cases centre the 100 px title: 87 + 50 = 137 of 275, and 100 + 50 = 150 of 300.
        let wide = PlaylistWindowState {
            size_steps: (1, 4),
            ..st.clone()
        };
        let c = render(&skin, &playlist(3), &wide);
        assert_sprite_at(&c, &skin, "PLAYLIST_TITLE_BAR_SELECTED", 100, 0);
    }

    #[test]
    fn unfocused_title_bar_uses_the_plain_sprites() {
        let skin = testkit::synthetic_skin();
        let st = PlaylistWindowState::default();
        let c = render(&skin, &playlist(1), &st);
        assert_sprite_at(&c, &skin, "PLAYLIST_TOP_LEFT_CORNER", 0, 0);
        assert_sprite_at(&c, &skin, "PLAYLIST_TITLE_BAR", 87, 0);
    }

    #[test]
    fn side_frames_and_bottom_corners_are_placed() {
        let skin = testkit::synthetic_skin();
        let st = PlaylistWindowState {
            size_steps: (0, 4),
            ..PlaylistWindowState::default()
        };
        let c = render(&skin, &playlist(3), &st);
        // Left tile starts under the title bar; its first frame is at y = 20.
        assert_sprite_at(&c, &skin, "PLAYLIST_LEFT_TILE", 0, TITLE_H);
        // The bottom buttons are drawn over the corners' lower rows (their faces start at row 8).
        assert_sprite_rows(&c, &skin, "PLAYLIST_BOTTOM_LEFT_CORNER", 0, 232 - 38, 0..8);
        // The running time and mini transport are drawn over the right corner's lower rows.
        assert_sprite_rows(
            &c,
            &skin,
            "PLAYLIST_BOTTOM_RIGHT_CORNER",
            275 - 150,
            232 - 38,
            0..8,
        );
    }

    #[test]
    fn rows_show_the_current_and_selected_colours() {
        let skin = testkit::synthetic_skin();
        let mut pl = playlist(4);
        pl.set_current(Some(1));
        pl.click(2);
        let st = PlaylistWindowState {
            size_steps: (0, 4),
            ..PlaylistWindowState::default()
        };
        let c = render(&skin, &pl, &st);
        let list = Layout::new((0, 4)).list();
        let row_bg = |index: i32| {
            let y = list.y + index * ROW_H;
            (y..y + ROW_H)
                .map(|yy| px(&c, list.x + list.w - 40, yy))
                .collect::<Vec<_>>()
        };
        // Row 2 is selected, so its empty background is the selected colour.
        assert!(row_bg(2).iter().all(|&p| p == skin.pledit.selected_bg));
        // Row 0 is neither: normal background.
        assert!(row_bg(0).iter().all(|&p| p == skin.pledit.normal_bg));
        // Row 1 is current, so its text uses the current colour somewhere.
        let text_row = |index: i32| {
            let y = list.y + index * ROW_H;
            (list.x..list.x + list.w)
                .flat_map(|x| (y..y + ROW_H).map(move |yy| (x, yy)))
                .map(|(x, yy)| px(&c, x, yy))
                .collect::<Vec<u32>>()
        };
        assert!(text_row(1).contains(&skin.pledit.current));
        assert!(!text_row(0).contains(&skin.pledit.current));
        assert!(text_row(0).contains(&skin.pledit.normal));
    }

    #[test]
    fn rows_stop_at_the_end_and_scroll_moves_the_first_row() {
        let skin = testkit::synthetic_skin();
        let pl = playlist(5);
        let st = PlaylistWindowState {
            size_steps: (0, 4),
            scroll: 2,
            ..PlaylistWindowState::default()
        };
        let c = render(&skin, &pl, &st);
        // Only rows 2..5 exist; the rows below them keep the normal background.
        let list = Layout::new((0, 4)).list();
        let y_past = list.y + 3 * ROW_H + 1;
        assert_eq!(px(&c, list.x + 2, y_past), skin.pledit.normal_bg);
    }

    #[test]
    fn scrollbar_thumb_follows_the_scroll_position() {
        let skin = testkit::synthetic_skin();
        let pl = playlist(30);
        let layout = Layout::new((0, 4));
        let track = layout.scrollbar();
        let top = render(&skin, &pl, &PlaylistWindowState::default());
        let bottom_state = PlaylistWindowState {
            scroll: 30,
            ..PlaylistWindowState::default()
        };
        let bottom = render(&skin, &pl, &bottom_state);
        let thumb = skin
            .sprite(Sheet::Pledit, "PLAYLIST_SCROLL_HANDLE")
            .expect("handle");
        assert_eq!(px(&top, track.x, track.y), thumb.pixel(0, 0));
        let travel = track.h - HANDLE_H;
        assert_eq!(
            px(&bottom, track.x, track.y + travel),
            thumb.pixel(0, 0),
            "thumb at the bottom when scrolled to the end"
        );
    }

    #[test]
    fn running_time_and_mini_time_are_drawn_in_the_bottom_bar() {
        let skin = testkit::synthetic_skin();
        let mut pl = playlist(3);
        pl.click(0);
        let st = PlaylistWindowState {
            size_steps: (0, 4),
            mini_time_secs: Some(83),
            ..PlaylistWindowState::default()
        };
        let quiet = PlaylistWindowState {
            size_steps: (0, 4),
            ..PlaylistWindowState::default()
        };
        let with = render(&skin, &pl, &st);
        let without = render(&skin, &pl, &quiet);
        let mini = Layout::new((0, 4)).mini_time();
        let mini_drawn = (mini.x..mini.x + mini.w)
            .any(|x| (mini.y..mini.y + mini.h).any(|y| px(&with, x, y) != px(&without, x, y)));
        assert!(mini_drawn, "the mini time draws text while playing");
        // The running time reads "sel/total": an empty list shows "0:00/0:00" instead.
        let empty = render(&skin, &Playlist::default(), &quiet);
        let (rx, ry) = Layout::new((0, 4)).running_time();
        let running =
            (rx..rx + 40).any(|x| (ry..ry + 6).any(|y| px(&without, x, y) != px(&empty, x, y)));
        assert!(running, "the running time draws the playlist's totals");
    }

    #[test]
    fn popup_items_and_bars_are_drawn_when_open() {
        let skin = testkit::synthetic_skin();
        let st = PlaylistWindowState {
            size_steps: (0, 4),
            open_menu: Some(PlMenu::Remove),
            hover: Some(PlHit::MenuItem(PlMenu::Remove, 1)),
            ..PlaylistWindowState::default()
        };
        let c = render(&skin, &playlist(3), &st);
        let l = Layout::new((0, 4));
        let bar = l.menu_bar(PlMenu::Remove);
        assert_eq!(bar.h, 72, "REMOVE's bar is four items tall");
        assert_sprite_at(&c, &skin, "PLAYLIST_REMOVE_MENU_BAR", bar.x, bar.y);
        let item = l.menu_item(PlMenu::Remove, 1);
        assert_sprite_at(&c, &skin, "PLAYLIST_CROP_SELECTED", item.x, item.y);
        let first = l.menu_item(PlMenu::Remove, 0);
        assert_sprite_at(&c, &skin, "PLAYLIST_REMOVE_ALL", first.x, first.y);
    }

    #[test]
    fn closed_bottom_buttons_show_skin_art_when_the_skin_has_it() {
        // A light sheet stands in for a classic skin's button faces.
        let (w, h) = testkit::sheet_size(Sheet::Pledit);
        let light = tubeamp_skin::Rgba {
            w,
            h,
            px: vec![0xFFE0_E0E0; (w * h) as usize],
        };
        let skin =
            Skin::load(&testkit::synthetic_zip_with(&[], &[(Sheet::Pledit, light)])).expect("skin");
        let st = PlaylistWindowState {
            size_steps: (0, 4),
            ..PlaylistWindowState::default()
        };
        let c = render(&skin, &playlist(3), &st);
        let add = Layout::new((0, 4)).menu(PlMenu::Add);
        // No fallback border: the skin's own light face shows through.
        assert_eq!(px(&c, add.x, add.y), 0xFFE0_E0E0);
        assert_eq!(px(&c, add.x + add.w - 1, add.y + add.h - 1), 0xFFE0_E0E0);
    }

    #[test]
    fn flat_bottom_buttons_get_a_label_and_a_border() {
        let (w, h) = testkit::sheet_size(Sheet::Pledit);
        let flat = tubeamp_skin::Rgba {
            w,
            h,
            px: vec![0xFF10_2030; (w * h) as usize],
        };
        let bytes = testkit::synthetic_zip_with(&[], &[(Sheet::Pledit, flat)]);
        let skin = Skin::load(&bytes).expect("skin");
        let st = PlaylistWindowState {
            size_steps: (0, 4),
            ..PlaylistWindowState::default()
        };
        let c = render(&skin, &playlist(3), &st);
        let add = Layout::new((0, 4)).menu(PlMenu::Add);
        let edge = dim(skin.pledit.normal, 0.5);
        assert_eq!(px(&c, add.x, add.y), edge, "top-left border pixel");
        assert_eq!(px(&c, add.x + add.w - 1, add.y + add.h - 1), edge);
        let flat_colour = 0xFF10_2030;
        let labelled = (add.x + 1..add.x + add.w - 1)
            .flat_map(|x| (add.y + 1..add.y + add.h - 1).map(move |y| (x, y)))
            .any(|(x, y)| px(&c, x, y) != flat_colour && px(&c, x, y) != edge);
        assert!(labelled, "the label is drawn inside the border");
    }

    #[test]
    fn flat_transport_buttons_get_an_icon() {
        let (w, h) = testkit::sheet_size(Sheet::Pledit);
        let flat = tubeamp_skin::Rgba {
            w,
            h,
            px: vec![0xFF10_2030; (w * h) as usize],
        };
        let skin =
            Skin::load(&testkit::synthetic_zip_with(&[], &[(Sheet::Pledit, flat)])).expect("skin");
        let st = PlaylistWindowState {
            size_steps: (0, 4),
            ..PlaylistWindowState::default()
        };
        let c = render(&skin, &playlist(3), &st);
        let play = Layout::new((0, 4)).transport(PlTransport::Play);
        let drawn = (play.x..play.x + play.w)
            .any(|x| (play.y..play.y + play.h).any(|y| px(&c, x, y) == skin.pledit.normal));
        assert!(drawn, "the play icon is drawn");
    }

    #[test]
    fn render_survives_every_size_and_popup() {
        let skin = testkit::synthetic_skin();
        let pl = playlist(40);
        for sx in 0..=6 {
            for sy in 0..=6 {
                for menu in PlMenu::ALL.iter().map(|m| Some(*m)).chain([None]) {
                    let st = PlaylistWindowState {
                        size_steps: (sx, sy),
                        scroll: 7,
                        focused: sx % 2 == 0,
                        open_menu: menu,
                        mini_time_secs: Some(3725),
                        mini_remaining: sy % 2 == 0,
                        ..PlaylistWindowState::default()
                    };
                    let _ = render(&skin, &pl, &st);
                }
            }
        }
        // Also an empty list and a skin without the PLEDIT sheet.
        let _ = render(&skin, &Playlist::default(), &PlaylistWindowState::default());
        let bare = testkit::synthetic_skin_without(&[Sheet::Pledit]);
        let _ = render(&bare, &pl, &PlaylistWindowState::default());
    }

    #[test]
    fn hit_test_finds_each_region() {
        let steps = (0, 4);
        let len = 30;
        let hit = |x, y| hit_test_playlist(steps, 0, len, None, x, y);
        assert_eq!(hit(150, 5), Some(PlHit::TitleBar));
        assert_eq!(hit(266, 5), Some(PlHit::Close));
        assert_eq!(hit(256, 5), Some(PlHit::Shade));
        assert_eq!(hit(270, 225), Some(PlHit::Resize));
        assert_eq!(hit(262, 60), Some(PlHit::Scrollbar));
        assert_eq!(hit(20, 23), Some(PlHit::Row(0)));
        assert_eq!(hit(20, 23 + 13 * 3 + 2), Some(PlHit::Row(3)));
        assert_eq!(hit(20, 23 + 13 * 3 + 2), hit(20, 23 + 13 * 3));
        assert_eq!(
            hit(20, 190),
            Some(PlHit::List),
            "the gap under the last row"
        );
        assert_eq!(hit(5, 40), None, "the left frame");
        assert_eq!(hit(15, 220), None, "the bottom-left corner panel");
        assert_eq!(hit(14 + 1, 232 - 30 + 1), Some(PlHit::Menu(PlMenu::Add)));
        assert_eq!(hit(43 + 1, 232 - 30 + 1), Some(PlHit::Menu(PlMenu::Remove)));
        assert_eq!(hit(72 + 1, 232 - 30 + 1), Some(PlHit::Menu(PlMenu::Select)));
        assert_eq!(hit(101 + 1, 232 - 30 + 1), Some(PlHit::Menu(PlMenu::Misc)));
        assert_eq!(
            hit(275 - 44 + 1, 232 - 30 + 1),
            Some(PlHit::Menu(PlMenu::List))
        );
        assert_eq!(
            hit(275 - 150 + 3, 232 - 16 + 1),
            Some(PlHit::Transport(PlTransport::Previous))
        );
        assert_eq!(
            hit(275 - 150 + 3 + 20, 232 - 16 + 1),
            Some(PlHit::Transport(PlTransport::Pause))
        );
        assert_eq!(
            hit(275 - 150 + 3 + 50, 232 - 16 + 1),
            Some(PlHit::Transport(PlTransport::Eject))
        );
        assert_eq!(hit(275 - 84 + 1, 232 - 15 + 1), Some(PlHit::MiniTime));
        assert_eq!(hit(-1, 5), None);
        assert_eq!(hit(275, 5), None);
        assert_eq!(hit(5, 232), None);
    }

    #[test]
    fn rows_past_the_end_of_the_list_are_not_rows() {
        let steps = (0, 4);
        assert_eq!(
            hit_test_playlist(steps, 0, 2, None, 20, 23 + 13 * 2 + 2),
            Some(PlHit::List)
        );
        assert_eq!(
            hit_test_playlist(steps, 5, 30, None, 20, 23 + 13 + 2),
            Some(PlHit::Row(6)),
            "rows are offset by the scroll"
        );
    }

    #[test]
    fn open_popup_items_win_and_clicks_outside_still_hit_the_frame() {
        let steps = (0, 4);
        let l = Layout::new(steps);
        let item = l.menu_item(PlMenu::Add, 2);
        let hit = hit_test_playlist(steps, 0, 0, Some(PlMenu::Add), item.x + 1, item.y + 1);
        assert_eq!(hit, Some(PlHit::MenuItem(PlMenu::Add, 2)));
        // The popup's top item is above the button, over the track area.
        let top = l.menu_item(PlMenu::Add, 0);
        assert_eq!(
            hit_test_playlist(steps, 0, 10, Some(PlMenu::Add), top.x + 1, top.y + 1),
            Some(PlHit::MenuItem(PlMenu::Add, 0))
        );
        // Without the popup the same point is a track row.
        assert_ne!(
            hit_test_playlist(steps, 0, 10, None, top.x + 1, top.y + 1),
            Some(PlHit::MenuItem(PlMenu::Add, 0))
        );
    }

    #[test]
    fn fit_text_adds_an_ellipsis_and_keeps_short_text() {
        assert_eq!(fit_text("abc", 1000.0), "abc");
        let cut = fit_text("A very long track title indeed", 60.0);
        assert!(cut.ends_with("..."), "{cut}");
        assert!(text_width(&cut) <= 60.0);
        assert_eq!(fit_text("abcdef", 0.0), "");
    }

    #[test]
    fn the_track_font_rasterises_text_and_clips() {
        let colour = 0xFF00_FF00;
        let mut canvas = Canvas::new(60, 14);
        draw_track_text(&mut canvas, "Hi", 2, 10, colour, Rect::new(0, 0, 60, 14));
        assert!(canvas.px.contains(&colour), "some pixels are drawn");
        // Clipped to the first column: nothing may be drawn to its right.
        let mut clipped = Canvas::new(60, 14);
        draw_track_text(&mut clipped, "Hi", 2, 10, colour, Rect::new(0, 0, 1, 14));
        assert!(clipped
            .px
            .iter()
            .enumerate()
            .all(|(i, &p)| p != colour || i % 60 < 1));
    }
}
