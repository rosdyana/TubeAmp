//! The Media Library window: a generic frame (see `gen_window.rs`) around a client area with a nav
//! column (Search, Home, Library, Liked, then the user's playlists), a search field, a status line
//! and a list of rows with a scrollbar.
//!
//! Like Winamp's library, the window is skinned: the frame comes from GEN.BMP and the list, text
//! and background colours come from the skin's GENEX.BMP (see `GenExColors`). Text is Liberation
//! Sans, shared with the playlist (`font.rs`). Layout, selection, scrolling and hit testing are
//! pure functions of [`LibraryState`]; the app decides what the rows mean and what a click does.

use std::collections::BTreeSet;

use tubeamp_skin::Skin;

use crate::canvas::{Canvas, Rect};
use crate::font::{draw_track_text, fit_text, text_width};
use crate::gen_window::{self as gen, GenHit};
use crate::labels::time_truncated;

/// Title shown in the frame.
pub const TITLE: &str = "MEDIA LIBRARY";
/// Size the window opens at, in resize steps: 525 × 348.
pub const DEFAULT_STEPS: (u32, u32) = (10, 8);

/// Width of the nav column, in client pixels (its divider is the last pixel of it).
const NAV_W: i32 = 70;
/// Pitch of nav items and list rows.
pub const ROW_H: i32 = 13;
/// Top of the first nav item.
const NAV_TOP: i32 = 3;
/// The search field's box.
const SEARCH_Y: i32 = 3;
const SEARCH_H: i32 = 14;
/// The status line's top.
const STATUS_Y: i32 = 19;
/// Top of the list (the first row), below the search field and status line.
const LIST_TOP: i32 = 32;
/// Left edge of the content column (right of the nav column).
const CONTENT_X: i32 = NAV_W + 2;
/// Right margin of the search field, and bottom margin of the list.
const RIGHT_MARGIN: i32 = 4;
const BOTTOM_MARGIN: i32 = 2;
/// The list's scrollbar: 8 px wide, 2 px in from the right, and the thumb's minimum height.
const SCROLL_W: i32 = 8;
const SCROLL_INSET: i32 = 2;
const THUMB_MIN: i32 = 12;
/// Baseline of text in a row or box whose top is given (the playlist's convention).
const BASELINE: i32 = 10;

/// Kind of a list row. Only tracks, albums, artists and playlists can be selected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibKind {
    /// A section title (a shelf title or the "Top result" label). Not selectable.
    Header,
    /// The "« Back" row at the top of an opened album or playlist. Not selectable.
    Back,
    Track,
    Album,
    Artist,
    Playlist,
}

impl LibKind {
    /// Whether rows of this kind take part in selection.
    pub fn selectable(self) -> bool {
        matches!(
            self,
            LibKind::Track | LibKind::Album | LibKind::Artist | LibKind::Playlist
        )
    }
}

/// One list row, as the window draws it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibRow {
    pub kind: LibKind,
    pub text: String,
    pub duration_secs: Option<u32>,
    /// The row is the track that is playing now (drawn in the highlight colour).
    pub playing: bool,
}

impl LibRow {
    pub fn new(kind: LibKind, text: impl Into<String>) -> LibRow {
        LibRow {
            kind,
            text: text.into(),
            duration_secs: None,
            playing: false,
        }
    }
}

/// What a nav item opens.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKind {
    Search,
    Home,
    Library,
    Liked,
    /// One of the user's library playlists. The app keeps its id, by position in the nav.
    Playlist,
    /// The "Playlists" caption above the playlist items. Not clickable.
    Heading,
}

/// One item of the nav column.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavItem {
    pub kind: NavKind,
    pub label: String,
}

/// Everything the library window shows and the state its input works on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibraryState {
    /// Resize steps from the base size (see `gen_window`).
    pub size_steps: (u32, u32),
    pub nav: Vec<NavItem>,
    /// The nav item for the section on show.
    pub nav_selected: Option<usize>,
    /// Index of the first visible nav item.
    pub nav_scroll: usize,
    /// The search text, and the caret as a character index into it.
    pub query: String,
    pub caret: usize,
    /// The search field takes typed characters.
    pub search_active: bool,
    /// The status line: a loading note, a hint or an error. Empty for none.
    pub status: String,
    pub rows: Vec<LibRow>,
    /// Selected row indices (only selectable rows are ever in here).
    pub selected: BTreeSet<usize>,
    /// The row that shift-click ranges start from.
    pub anchor: Option<usize>,
    /// Index of the first visible row.
    pub scroll: usize,
    pub focused: bool,
}

impl Default for LibraryState {
    fn default() -> Self {
        LibraryState {
            size_steps: DEFAULT_STEPS,
            nav: default_nav(),
            nav_selected: None,
            nav_scroll: 0,
            query: String::new(),
            caret: 0,
            search_active: false,
            status: String::new(),
            rows: Vec::new(),
            selected: BTreeSet::new(),
            anchor: None,
            scroll: 0,
            focused: false,
        }
    }
}

/// The nav column before any playlists are known: the four sections and the caption.
pub fn default_nav() -> Vec<NavItem> {
    vec![
        NavItem {
            kind: NavKind::Search,
            label: "Search".to_string(),
        },
        NavItem {
            kind: NavKind::Home,
            label: "Home".to_string(),
        },
        NavItem {
            kind: NavKind::Library,
            label: "Library".to_string(),
        },
        NavItem {
            kind: NavKind::Liked,
            label: "Liked".to_string(),
        },
        NavItem {
            kind: NavKind::Heading,
            label: "Playlists".to_string(),
        },
    ]
}

impl LibraryState {
    /// Replaces the playlist items under the caption with `titles`, in order.
    pub fn set_nav_playlists(&mut self, titles: &[String]) {
        self.nav.truncate(default_nav().len());
        self.nav.extend(titles.iter().map(|t| NavItem {
            kind: NavKind::Playlist,
            label: t.clone(),
        }));
        self.nav_scroll = self.nav_scroll.min(self.nav.len().saturating_sub(1));
    }

    /// Whether row `index` exists and can be selected.
    pub fn is_selectable(&self, index: usize) -> bool {
        self.rows.get(index).is_some_and(|r| r.kind.selectable())
    }

    /// Rows that fit in the list.
    pub fn visible_rows(&self) -> usize {
        list_rect(self.size_steps).h as usize / ROW_H as usize
    }

    /// Nav items that fit in the nav column.
    pub fn visible_nav(&self) -> usize {
        nav_rows_visible(self.size_steps)
    }

    /// Keeps the scroll offsets within their lists.
    pub fn clamp_scroll(&mut self) {
        let rows = self.visible_rows();
        self.scroll = self.scroll.min(self.rows.len().saturating_sub(rows));
        let nav = self.visible_nav();
        self.nav_scroll = self.nav_scroll.min(self.nav.len().saturating_sub(nav));
    }

    /// Types `text` into the search field at the caret. Control characters are dropped.
    pub fn insert_text(&mut self, text: &str) {
        let mut chars: Vec<char> = self.query.chars().collect();
        let at = self.caret.min(chars.len());
        let typed: Vec<char> = text.chars().filter(|c| !c.is_control()).collect();
        let n = typed.len();
        chars.splice(at..at, typed);
        self.query = chars.into_iter().collect();
        self.caret = at + n;
    }

    /// Deletes the character before the caret.
    pub fn backspace(&mut self) {
        if self.caret == 0 {
            return;
        }
        let mut chars: Vec<char> = self.query.chars().collect();
        let at = self.caret.min(chars.len());
        if at > 0 {
            chars.remove(at - 1);
            self.query = chars.into_iter().collect();
            self.caret = at - 1;
        }
    }

    /// Deletes the character after the caret.
    pub fn delete_forward(&mut self) {
        let mut chars: Vec<char> = self.query.chars().collect();
        let at = self.caret.min(chars.len());
        if at < chars.len() {
            chars.remove(at);
            self.query = chars.into_iter().collect();
            self.caret = at;
        }
    }

    /// Moves the caret by `delta` characters, within the text.
    pub fn move_caret(&mut self, delta: isize) {
        let len = self.query.chars().count() as isize;
        self.caret = (self.caret as isize + delta).clamp(0, len) as usize;
    }

    /// Puts the caret at character `at` (clamped to the text).
    pub fn set_caret(&mut self, at: usize) {
        self.caret = at.min(self.query.chars().count());
    }

    /// Scrolls the list by `delta` rows.
    pub fn scroll_by(&mut self, delta: isize) {
        let max = self.rows.len().saturating_sub(self.visible_rows());
        self.scroll = (self.scroll as isize + delta).clamp(0, max as isize) as usize;
    }

    /// Scrolls the nav column by `delta` items.
    pub fn scroll_nav_by(&mut self, delta: isize) {
        let max = self.nav.len().saturating_sub(self.visible_nav());
        self.nav_scroll = (self.nav_scroll as isize + delta).clamp(0, max as isize) as usize;
    }

    /// Scrolls the least needed so that row `index` is visible.
    pub fn show_row(&mut self, index: usize) {
        let rows = self.visible_rows();
        if rows == 0 {
            return;
        }
        if index < self.scroll {
            self.scroll = index;
        } else if index >= self.scroll + rows {
            self.scroll = index + 1 - rows;
        }
        self.clamp_scroll();
    }

    /// A plain click: selects only row `index` and makes it the anchor.
    pub fn click(&mut self, index: usize) {
        if self.is_selectable(index) {
            self.selected = BTreeSet::from([index]);
            self.anchor = Some(index);
        }
    }

    /// Ctrl-click: toggles row `index` and makes it the anchor.
    pub fn ctrl_click(&mut self, index: usize) {
        if !self.is_selectable(index) {
            return;
        }
        if !self.selected.remove(&index) {
            self.selected.insert(index);
        }
        self.anchor = Some(index);
    }

    /// Shift-click: selects the selectable rows from the anchor to `index`, replacing the
    /// selection. Without an anchor it selects only `index`.
    pub fn shift_click(&mut self, index: usize) {
        if !self.is_selectable(index) {
            return;
        }
        let Some(anchor) = self.anchor else {
            self.click(index);
            return;
        };
        let (lo, hi) = (anchor.min(index), anchor.max(index));
        self.selected = (lo..=hi).filter(|&i| self.is_selectable(i)).collect();
    }

    /// Clears the selection (the anchor stays).
    pub fn select_none(&mut self) {
        self.selected.clear();
    }

    /// The lowest selected row, if any.
    pub fn first_selected(&self) -> Option<usize> {
        self.selected.iter().next().copied()
    }

    /// Moves the selection cursor by `delta` rows from the anchor, skipping rows that cannot be
    /// selected. With `extend` (shift held) the selection grows from the anchor to the new row.
    pub fn move_cursor(&mut self, delta: isize, extend: bool) {
        let len = self.rows.len() as isize;
        if len == 0 || delta == 0 {
            return;
        }
        let base = match self.anchor {
            Some(a) => a as isize,
            None if delta > 0 => -1,
            None => len,
        };
        let mut target = (base + delta).clamp(0, len - 1);
        // Step past headers in the direction of travel. Give up when the list has none left.
        while !self.is_selectable(target as usize) {
            let next = target + delta.signum();
            if next < 0 || next >= len {
                return;
            }
            target = next;
        }
        let target = target as usize;
        if extend && self.anchor.is_some() {
            self.shift_click(target);
        } else {
            self.click(target);
        }
        self.show_row(target);
    }
}

/// The client area's rectangle inside the window, from the frame's client rectangle.
fn client(steps: (u32, u32)) -> Rect {
    gen::client_rect(steps)
}

/// The nav column, in client coordinates.
fn nav_column(steps: (u32, u32)) -> Rect {
    let c = client(steps);
    Rect::new(0, 0, NAV_W, c.h)
}

/// Nav items that fit under the nav column's top.
fn nav_rows_visible(steps: (u32, u32)) -> usize {
    let c = client(steps);
    ((c.h - NAV_TOP - BOTTOM_MARGIN) / ROW_H).max(0) as usize
}

/// One nav item's rectangle in client coordinates, for the `k`th visible item.
fn nav_item_rect(k: usize) -> Rect {
    Rect::new(3, NAV_TOP + k as i32 * ROW_H, NAV_W - 6, ROW_H)
}

/// The search field in client coordinates.
fn search_rect(steps: (u32, u32)) -> Rect {
    let c = client(steps);
    Rect::new(
        CONTENT_X,
        SEARCH_Y,
        c.w - RIGHT_MARGIN - CONTENT_X,
        SEARCH_H,
    )
}

/// The list in client coordinates.
pub fn list_rect(steps: (u32, u32)) -> Rect {
    let c = client(steps);
    Rect::new(
        CONTENT_X,
        LIST_TOP,
        c.w - CONTENT_X - SCROLL_W - SCROLL_INSET - 2,
        c.h - LIST_TOP - BOTTOM_MARGIN,
    )
}

/// The list's scrollbar in client coordinates.
fn scrollbar_rect(steps: (u32, u32)) -> Rect {
    let c = client(steps);
    let list = list_rect(steps);
    Rect::new(c.w - SCROLL_W - SCROLL_INSET, LIST_TOP, SCROLL_W, list.h)
}

/// What a point on the library window belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibHit {
    /// The title bar: drags the window.
    TitleBar,
    Close,
    /// The bottom-right grip: resizes the window.
    Resize,
    /// A nav item, by index into `LibraryState::nav` (the caption is one too).
    Nav(usize),
    /// The search field.
    Search,
    /// A list row, by index into `LibraryState::rows`.
    Row(usize),
    /// The list's scrollbar.
    Scrollbar,
    /// The list, below its last row.
    List,
    /// Other client area (the status line, the gaps, the empty nav column).
    Client,
    /// The side or bottom frame.
    Frame,
}

/// Which element of the library window is at window point `(x, y)`, or `None` outside it.
pub fn hit_test_library(st: &LibraryState, x: i32, y: i32) -> Option<LibHit> {
    let steps = st.size_steps;
    match gen::hit_test_gen(steps, x, y)? {
        GenHit::TitleBar => return Some(LibHit::TitleBar),
        GenHit::Close => return Some(LibHit::Close),
        GenHit::Resize => return Some(LibHit::Resize),
        GenHit::Frame => return Some(LibHit::Frame),
        GenHit::Client => {}
    }
    let origin = client(steps);
    let (cx, cy) = (x - origin.x, y - origin.y);

    if nav_column(steps).contains(cx, cy) {
        let k = ((cy - NAV_TOP) / ROW_H).max(0) as usize;
        let inside = cy >= NAV_TOP && k < nav_rows_visible(steps);
        let index = st.nav_scroll + k;
        return Some(if inside && index < st.nav.len() {
            LibHit::Nav(index)
        } else {
            LibHit::Client
        });
    }
    if search_rect(steps).contains(cx, cy) {
        return Some(LibHit::Search);
    }
    if scrollbar_rect(steps).contains(cx, cy) {
        return Some(LibHit::Scrollbar);
    }
    let list = list_rect(steps);
    if list.contains(cx, cy) {
        let row = ((cy - LIST_TOP) / ROW_H) as usize;
        let index = st.scroll + row;
        return Some(if index < st.rows.len() {
            LibHit::Row(index)
        } else {
            LibHit::List
        });
    }
    Some(LibHit::Client)
}

// Drawing.

/// Draws the whole window at the size of `st.size_steps`. `canvas` must be the window's size.
pub fn render_library(skin: &Skin, st: &LibraryState, canvas: &mut Canvas) {
    gen::render_frame(skin, st.size_steps, TITLE, st.focused, canvas);
    let origin = client(st.size_steps);
    let colors = skin.genex;
    canvas.fill(origin, colors.window_background);

    let at = |r: Rect| Rect::new(origin.x + r.x, origin.y + r.y, r.w, r.h);

    draw_nav(skin, st, origin, canvas);
    // A divider between the nav column and the content.
    canvas.fill(at(Rect::new(NAV_W - 1, 0, 1, origin.h)), colors.divider);

    draw_search(skin, st, at(search_rect(st.size_steps)), canvas);
    if !st.status.is_empty() {
        let max_w = (origin.w - CONTENT_X - RIGHT_MARGIN) as f32;
        let text = fit_text(&st.status, max_w);
        draw_track_text(
            canvas,
            &text,
            origin.x + CONTENT_X,
            origin.y + STATUS_Y + 9,
            colors.window_text,
            at(Rect::new(CONTENT_X, STATUS_Y, origin.w - CONTENT_X, 10)),
        );
    }

    let list = at(list_rect(st.size_steps));
    canvas.fill(list, colors.item_background);
    draw_rows(skin, st, list, canvas);
    draw_scrollbar(skin, st, at(scrollbar_rect(st.size_steps)), canvas);
}

fn draw_nav(skin: &Skin, st: &LibraryState, origin: Rect, canvas: &mut Canvas) {
    let colors = skin.genex;
    let column = Rect::new(origin.x, origin.y, NAV_W - 1, origin.h);
    let clip = column;
    for k in 0..st.visible_nav() {
        let index = st.nav_scroll + k;
        let Some(item) = st.nav.get(index) else {
            break;
        };
        let r = nav_item_rect(k);
        let rect = Rect::new(origin.x + r.x, origin.y + r.y, r.w, r.h);
        let baseline = rect.y + BASELINE;
        match item.kind {
            NavKind::Heading => {
                draw_track_text(
                    canvas,
                    &fit_text(&item.label, (rect.w - 2) as f32),
                    rect.x + 1,
                    baseline,
                    colors.window_text,
                    clip,
                );
            }
            _ => {
                let selected = st.nav_selected == Some(index);
                let (bg, fg) = if selected {
                    (colors.playlist_selection, colors.list_text_highlighted)
                } else {
                    (colors.window_background, colors.item_foreground)
                };
                canvas.fill(rect, bg);
                draw_track_text(
                    canvas,
                    &fit_text(&item.label, (rect.w - 4) as f32),
                    rect.x + 2,
                    baseline,
                    fg,
                    clip,
                );
            }
        }
    }
}

/// The search field: a sunken box with its text (or a hint) and a caret while it is active.
fn draw_search(skin: &Skin, st: &LibraryState, rect: Rect, canvas: &mut Canvas) {
    let colors = skin.genex;
    canvas.fill(rect, colors.item_background);
    // A 1 px border in the divider colour.
    canvas.fill(Rect::new(rect.x, rect.y, rect.w, 1), colors.divider);
    canvas.fill(
        Rect::new(rect.x, rect.y + rect.h - 1, rect.w, 1),
        colors.divider,
    );
    canvas.fill(Rect::new(rect.x, rect.y, 1, rect.h), colors.divider);
    canvas.fill(
        Rect::new(rect.x + rect.w - 1, rect.y, 1, rect.h),
        colors.divider,
    );

    let inner = Rect::new(rect.x + 3, rect.y, rect.w - 6, rect.h);
    let baseline = rect.y + BASELINE;
    if st.query.is_empty() && !st.search_active {
        let hint = fit_text("Search YouTube Music", inner.w as f32);
        draw_track_text(canvas, &hint, inner.x, baseline, colors.button_text, inner);
        return;
    }
    // The text before the caret, cut from the left so the caret stays in view, then the rest.
    let chars: Vec<char> = st.query.chars().collect();
    let caret = st.caret.min(chars.len());
    let before: String = chars[..caret].iter().collect();
    let after: String = chars[caret..].iter().collect();
    let before = tail_fit(&before, inner.w as f32);
    let before_w = text_width(&before).round() as i32;
    draw_track_text(
        canvas,
        &before,
        inner.x,
        baseline,
        colors.item_foreground,
        inner,
    );
    let room = (inner.w - before_w).max(0) as f32;
    let after = fit_text(&after, room);
    draw_track_text(
        canvas,
        &after,
        inner.x + before_w,
        baseline,
        colors.item_foreground,
        inner,
    );
    if st.search_active {
        let caret_x = inner.x + before_w;
        canvas.fill(
            Rect::new(caret_x, rect.y + 2, 1, rect.h - 4),
            colors.item_foreground,
        );
    }
}

/// The end of `text` that fits in `max_w` pixels (the start is cut, with no ellipsis).
fn tail_fit(text: &str, max_w: f32) -> String {
    if text_width(text) <= max_w {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    for keep in (0..chars.len()).rev() {
        let candidate: String = chars[chars.len() - keep..].iter().collect();
        if text_width(&candidate) <= max_w {
            return candidate;
        }
    }
    String::new()
}

fn draw_rows(skin: &Skin, st: &LibraryState, list: Rect, canvas: &mut Canvas) {
    let colors = skin.genex;
    let text_x = list.x + 2;
    let right = list.x + list.w - 3;
    for k in 0..st.visible_rows() {
        let index = st.scroll + k;
        let Some(row) = st.rows.get(index) else {
            break;
        };
        let y = list.y + k as i32 * ROW_H;
        let band = Rect::new(list.x, y, list.w, ROW_H);
        let selected = st.selected.contains(&index);
        if selected {
            canvas.fill(band, colors.playlist_selection);
        }
        let color = match row.kind {
            LibKind::Header => colors.window_text,
            LibKind::Back => colors.list_text_highlighted,
            _ if row.playing => colors.list_text_highlighted,
            _ => colors.item_foreground,
        };
        let baseline = y + BASELINE;
        let duration = row.duration_secs.map(time_truncated).unwrap_or_default();
        let duration_x = right - text_width(&duration).round() as i32;
        let title_right = if duration.is_empty() {
            right
        } else {
            duration_x - 6
        };
        let shown = fit_text(&row.text, (title_right - text_x).max(0) as f32);
        draw_track_text(canvas, &shown, text_x, baseline, color, list);
        if !duration.is_empty() {
            draw_track_text(canvas, &duration, duration_x, baseline, color, list);
        }
    }
}

/// The thumb's height and how far it can travel, for `total` rows of which `visible` show in a
/// track `track_h` tall. `None` when nothing scrolls.
fn thumb_geometry(total: usize, visible: usize, track_h: i32) -> Option<(i32, i32)> {
    if total <= visible || visible == 0 {
        return None;
    }
    let thumb_h = ((track_h as f64 * visible as f64 / total as f64).round() as i32)
        .clamp(THUMB_MIN.min(track_h), track_h);
    Some((thumb_h, track_h - thumb_h))
}

/// The scroll offset for a pointer at window height `y` on the scrollbar, with the thumb centred
/// on the pointer (what a drag of the thumb does).
pub fn scroll_at(st: &LibraryState, y: f64) -> usize {
    let steps = st.size_steps;
    let track = scrollbar_rect(steps);
    let total = st.rows.len();
    let visible = st.visible_rows();
    let Some((thumb_h, travel)) = thumb_geometry(total, visible, track.h) else {
        return 0;
    };
    let top = f64::from(client(steps).y + track.y);
    let rel = (y - top - f64::from(thumb_h) / 2.0).clamp(0.0, f64::from(travel));
    let max = total - visible;
    (rel / f64::from(travel.max(1)) * max as f64).round() as usize
}

fn draw_scrollbar(skin: &Skin, st: &LibraryState, track: Rect, canvas: &mut Canvas) {
    let colors = skin.genex;
    canvas.fill(track, colors.scrollbar_dead_area);
    let Some((thumb_h, travel)) = thumb_geometry(st.rows.len(), st.visible_rows(), track.h) else {
        return;
    };
    let max = st.rows.len() - st.visible_rows();
    let offset = (st.scroll as f64 / max as f64 * f64::from(travel)).round() as i32;
    let thumb = Rect::new(track.x, track.y + offset, track.w, thumb_h);
    canvas.fill(thumb, colors.scrollbar_one);
    // A 1 px frame in the second colour, on the top and left.
    canvas.fill(
        Rect::new(thumb.x, thumb.y, thumb.w, 1),
        colors.scrollbar_two,
    );
    canvas.fill(
        Rect::new(thumb.x, thumb.y, 1, thumb.h),
        colors.scrollbar_two,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use tubeamp_skin::testkit;

    fn row(kind: LibKind, text: &str) -> LibRow {
        LibRow::new(kind, text)
    }

    /// A list with a header, then tracks and albums, and a back row at the top.
    fn sample() -> LibraryState {
        LibraryState {
            rows: vec![
                row(LibKind::Back, "« Back"),
                row(LibKind::Header, "Songs"),
                row(LibKind::Track, "1. Artist - One"),
                row(LibKind::Track, "2. Artist - Two"),
                row(LibKind::Header, "Albums"),
                row(LibKind::Album, "[ALBUM] Discovery - Daft Punk"),
            ],
            ..LibraryState::default()
        }
    }

    fn px(c: &Canvas, x: i32, y: i32) -> u32 {
        c.px[(y as u32 * c.w + x as u32) as usize]
    }

    fn render(skin: &Skin, st: &LibraryState) -> Canvas {
        let (w, h) = gen::window_size(st.size_steps);
        let mut c = Canvas::new(w, h);
        render_library(skin, st, &mut c);
        c
    }

    /// Window point of client-local `(x, y)`.
    fn win(steps: (u32, u32), x: i32, y: i32) -> (i32, i32) {
        let c = client(steps);
        (c.x + x, c.y + y)
    }

    #[test]
    fn list_and_nav_geometry_at_the_base_size() {
        let steps = (0, 0);
        assert_eq!(
            list_rect(steps),
            Rect::new(72, 32, 256 - 72 - 10 - 2, 82 - 32 - 2)
        );
        assert_eq!(nav_rows_visible(steps), (82 - 3 - 2) / 13);
        let big = DEFAULT_STEPS;
        assert!(
            list_rect(big).h / ROW_H > 20,
            "a tall window shows many rows"
        );
    }

    #[test]
    fn selection_skips_headers_and_back_rows() {
        let mut st = sample();
        st.click(0);
        assert!(st.selected.is_empty(), "the back row is not selectable");
        st.click(1);
        assert!(st.selected.is_empty(), "a header is not selectable");
        st.click(2);
        assert_eq!(st.selected, BTreeSet::from([2]));
        st.ctrl_click(5);
        assert_eq!(st.selected, BTreeSet::from([2, 5]));
        st.ctrl_click(2);
        assert_eq!(st.selected, BTreeSet::from([5]), "ctrl-click toggles off");
    }

    #[test]
    fn shift_click_selects_only_selectable_rows_in_the_range() {
        let mut st = sample();
        st.click(2);
        st.shift_click(5);
        assert_eq!(st.selected, BTreeSet::from([2, 3, 5]));
        assert_eq!(st.anchor, Some(2), "the anchor does not move");
        st.shift_click(0);
        assert_eq!(
            st.selected,
            BTreeSet::from([2, 3, 5]),
            "a shift-click on the back row changes nothing"
        );
    }

    #[test]
    fn keyboard_cursor_moves_past_headers() {
        let mut st = sample();
        st.move_cursor(1, false);
        assert_eq!(st.selected, BTreeSet::from([2]), "the first selectable row");
        st.move_cursor(1, false);
        assert_eq!(st.selected, BTreeSet::from([3]));
        st.move_cursor(1, false);
        assert_eq!(st.selected, BTreeSet::from([5]), "skips the header at 4");
        st.move_cursor(1, false);
        assert_eq!(st.selected, BTreeSet::from([5]), "stays at the last row");
        st.move_cursor(-1, true);
        assert_eq!(
            st.selected,
            BTreeSet::from([3, 5]),
            "shift-up extends, skipping the header"
        );
    }

    #[test]
    fn keyboard_on_a_list_with_no_selectable_rows_does_nothing() {
        let mut st = LibraryState {
            rows: vec![row(LibKind::Header, "Only a title")],
            ..LibraryState::default()
        };
        st.move_cursor(1, false);
        assert!(st.selected.is_empty());
    }

    #[test]
    fn scrolling_clamps_to_the_list_and_show_row_brings_a_row_in() {
        let mut st = LibraryState {
            rows: (0..100)
                .map(|i| row(LibKind::Track, &format!("{i}")))
                .collect(),
            ..LibraryState::default()
        };
        let rows = st.visible_rows();
        st.scroll_by(1000);
        assert_eq!(st.scroll, 100 - rows);
        st.scroll_by(-1000);
        assert_eq!(st.scroll, 0);
        st.show_row(50);
        assert!((st.scroll..st.scroll + rows).contains(&50));
        st.show_row(0);
        assert_eq!(st.scroll, 0);
    }

    #[test]
    fn nav_playlists_go_under_the_caption() {
        let mut st = LibraryState::default();
        st.set_nav_playlists(&["Mix".to_string(), "Road".to_string()]);
        assert_eq!(st.nav.len(), 7);
        assert_eq!(st.nav[4].kind, NavKind::Heading);
        assert_eq!(st.nav[5].label, "Mix");
        assert_eq!(st.nav[6].kind, NavKind::Playlist);
        st.set_nav_playlists(&[]);
        assert_eq!(st.nav.len(), 5, "replacing with none drops them");
    }

    #[test]
    fn hit_test_finds_each_part_of_the_window() {
        let st = sample();
        let steps = st.size_steps;
        let hit = |x, y| hit_test_library(&st, x, y);
        assert_eq!(hit(100, 5), Some(LibHit::TitleBar));
        let (w, _) = gen::window_size(steps);
        assert_eq!(hit(w as i32 - 11 + 1, 4), Some(LibHit::Close));
        let (w, h) = gen::window_size(steps);
        assert_eq!(hit(w as i32 - 5, h as i32 - 5), Some(LibHit::Resize));
        assert_eq!(hit(3, 50), Some(LibHit::Frame));

        let (x, y) = win(steps, 10, NAV_TOP + 2 * ROW_H + 2);
        assert_eq!(hit(x, y), Some(LibHit::Nav(2)), "the Library nav item");
        let (x, y) = win(steps, 100, SEARCH_Y + 4);
        assert_eq!(hit(x, y), Some(LibHit::Search));
        let (x, y) = win(steps, 100, LIST_TOP + 2 * ROW_H + 2);
        assert_eq!(hit(x, y), Some(LibHit::Row(2)));
        let (x, y) = win(steps, 100, LIST_TOP + 5 * ROW_H + 2);
        assert_eq!(hit(x, y), Some(LibHit::Row(5)));
        let (x, y) = win(steps, 100, LIST_TOP + 7 * ROW_H + 2);
        assert_eq!(hit(x, y), Some(LibHit::List), "below the last row");
        let sb = scrollbar_rect(steps);
        let (x, y) = win(steps, sb.x + 2, sb.y + 2);
        assert_eq!(hit(x, y), Some(LibHit::Scrollbar));
        assert_eq!(hit(-1, 5), None);
    }

    #[test]
    fn nav_past_the_last_item_is_client_area() {
        let st = sample();
        let (x, y) = win(st.size_steps, 10, NAV_TOP + 4 * ROW_H + 2);
        // Four items fill the first rows of the base-size column; below them there is nothing.
        assert_eq!(hit_test_library(&st, x, y), Some(LibHit::Nav(4)));
        let (x, y) = win(st.size_steps, 10, NAV_TOP + 10 * ROW_H + 2);
        assert_eq!(hit_test_library(&st, x, y), Some(LibHit::Client));
    }

    #[test]
    fn render_draws_the_frame_nav_search_and_rows() {
        let skin = testkit::synthetic_skin();
        let mut st = sample();
        st.query = "daft punk".to_string();
        st.nav_selected = Some(0);
        st.selected = BTreeSet::from([2]);
        let c = render(&skin, &st);
        let colors = skin.genex;

        // The frame's top-left corner is the GEN piece for a focused or unfocused window.
        assert_eq!(
            px(&c, 0, 0),
            skin.sprite(tubeamp_skin::Sheet::Gen, "GEN_TOP_LEFT")
                .unwrap()
                .pixel(0, 0)
        );
        // The selected row has the selection colour behind it.
        let list = list_rect(st.size_steps);
        let row_y = |k: i32| list.y + k * ROW_H;
        let client_origin = client(st.size_steps);
        let lx = client_origin.x + list.x + list.w - 40;
        let sel_y = client_origin.y + row_y(2) + 1;
        assert_eq!(px(&c, lx, sel_y), colors.playlist_selection);
        // An unselected row has the item background.
        let plain_y = client_origin.y + row_y(3) + 1;
        assert_eq!(px(&c, lx, plain_y), colors.item_background);
        // The search field's box is drawn in the item background, with the query's text on it.
        let search = search_rect(st.size_steps);
        let sx = client_origin.x + search.x + 1;
        let sy = client_origin.y + search.y + 2;
        let inside = (sx..sx + 80).any(|x| {
            (sy..sy + 10)
                .any(|y| px(&c, x, y) != colors.item_background && px(&c, x, y) != colors.divider)
        });
        assert!(inside, "the query text is drawn in the search field");
        // The nav column shows the selected item in the selection colour.
        let nav_sel = client_origin.y + NAV_TOP + 1;
        assert_eq!(
            px(&c, client_origin.x + 4, nav_sel),
            colors.playlist_selection
        );
    }

    #[test]
    fn render_draws_a_status_line_and_a_scrollbar_thumb() {
        let skin = testkit::synthetic_skin();
        let mut st = LibraryState {
            rows: (0..60)
                .map(|i| row(LibKind::Track, &format!("{i}. Song")))
                .collect(),
            status: "Sign in to see your library (press L)".to_string(),
            ..LibraryState::default()
        };
        let quiet = render(
            &skin,
            &LibraryState {
                status: String::new(),
                ..st.clone()
            },
        );
        let c = render(&skin, &st);
        let origin = client(st.size_steps);
        let status_drawn = (origin.x + CONTENT_X..origin.x + CONTENT_X + 150).any(|x| {
            (origin.y + STATUS_Y..origin.y + STATUS_Y + 10)
                .any(|y| px(&c, x, y) != px(&quiet, x, y))
        });
        assert!(status_drawn, "the status line draws text");

        // With 60 rows and 21 visible, the thumb is at the top when unscrolled, and it has moved
        // down (out of the top pixels) when the list is scrolled to the middle.
        let track = scrollbar_rect(st.size_steps);
        let probe = |canvas: &Canvas| px(canvas, origin.x + track.x + 3, origin.y + track.y + 2);
        let at_top = render(&skin, &st);
        assert_eq!(
            probe(&at_top),
            skin.genex.scrollbar_one,
            "the thumb starts at the top"
        );
        st.scroll = 30;
        let scrolled = render(&skin, &st);
        assert_eq!(
            probe(&scrolled),
            skin.genex.scrollbar_dead_area,
            "a scrolled list moves the thumb down"
        );
    }

    #[test]
    fn render_survives_every_size_and_state() {
        let skin = testkit::synthetic_skin();
        let rows: Vec<LibRow> = (0..40)
            .map(|i| match i % 5 {
                0 => row(LibKind::Header, "Shelf"),
                1 => row(LibKind::Album, "[ALBUM] Something - Someone"),
                _ => row(
                    LibKind::Track,
                    &format!("{i}. A very long artist - a very long title indeed"),
                ),
            })
            .collect();
        for sx in 0..=5 {
            for sy in 0..=5 {
                let mut st = LibraryState {
                    size_steps: (sx, sy),
                    rows: rows.clone(),
                    query: "daft punk".into(),
                    caret: 3,
                    search_active: sx % 2 == 0,
                    focused: sy % 2 == 0,
                    scroll: 7,
                    ..LibraryState::default()
                };
                st.set_nav_playlists(&(0..20).map(|i| format!("Playlist {i}")).collect::<Vec<_>>());
                st.nav_scroll = 4;
                st.selected = BTreeSet::from([2, 3]);
                let _ = render(&skin, &st);
            }
        }
        let bare = testkit::synthetic_skin_without(&[tubeamp_skin::Sheet::Gen]);
        let _ = render(&bare, &sample());
        let _ = render(&skin, &LibraryState::default());
    }

    #[test]
    fn typing_inserts_at_the_caret_and_drops_control_characters() {
        let mut st = LibraryState::default();
        st.insert_text("dt");
        assert_eq!((st.query.as_str(), st.caret), ("dt", 2));
        st.move_caret(-1);
        st.insert_text("a\n");
        assert_eq!(
            (st.query.as_str(), st.caret),
            ("dat", 2),
            "inserted before the caret"
        );
        st.move_caret(100);
        assert_eq!(st.caret, 3, "the caret stops at the end");
        st.insert_text(" punk");
        assert_eq!(st.query, "dat punk");
    }

    #[test]
    fn backspace_and_delete_work_around_the_caret() {
        let mut st = LibraryState {
            query: "daft punk".into(),
            caret: 4,
            ..LibraryState::default()
        };
        st.backspace();
        assert_eq!(
            (st.query.as_str(), st.caret),
            ("daf punk", 3),
            "removes the t before the caret"
        );
        st.delete_forward();
        assert_eq!((st.query.as_str(), st.caret), ("dafpunk", 3));
        st.set_caret(0);
        st.backspace();
        assert_eq!(
            (st.query.as_str(), st.caret),
            ("dafpunk", 0),
            "nothing before the start"
        );
        st.set_caret(99);
        st.delete_forward();
        assert_eq!(st.query, "dafpunk", "nothing after the end");
    }

    #[test]
    fn dragging_the_thumb_maps_the_pointer_to_the_scroll_offset() {
        let mut st = LibraryState {
            rows: (0..100)
                .map(|i| row(LibKind::Track, &format!("{i}")))
                .collect(),
            ..LibraryState::default()
        };
        st.size_steps = (0, 0);
        let track = scrollbar_rect(st.size_steps);
        let origin = client(st.size_steps);
        let top = f64::from(origin.y + track.y);
        let max = 100 - st.visible_rows();
        assert_eq!(
            scroll_at(&st, top),
            0,
            "the top of the track is the top of the list"
        );
        let bottom = top + f64::from(track.h) + 50.0;
        assert_eq!(scroll_at(&st, bottom), max, "past the bottom is the end");
        let few = LibraryState {
            rows: vec![row(LibKind::Track, "one")],
            ..LibraryState::default()
        };
        assert_eq!(scroll_at(&few, top + 10.0), 0, "nothing scrolls");
    }

    #[test]
    fn tail_fit_keeps_the_end_of_the_query() {
        assert_eq!(tail_fit("abc", 1000.0), "abc");
        let cut = tail_fit("daft punk and friends", 40.0);
        assert!(text_width(&cut) <= 40.0);
        assert!("daft punk and friends".ends_with(&cut));
        assert_eq!(tail_fit("abc", 0.0), "");
    }
}
