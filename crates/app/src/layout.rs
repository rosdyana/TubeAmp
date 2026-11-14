//! Window layout: snapping while dragging, connections between windows, and where they move when
//! a window changes size (double size, windowshade). A port of Webamp's `snapUtils.ts`,
//! `resizeUtils.ts` and the drag logic of `WindowManager.tsx` (MIT, © Jordan Eldredge).
//!
//! Everything here works in physical pixels, with the snap distance scaled by the monitor's scale
//! factor by the caller. Nothing here knows about winit, so it is tested on its own.

use std::collections::{BTreeSet, HashMap};

/// Webamp's `SNAP_DISTANCE`, in logical pixels. Callers multiply it by the scale factor.
pub const SNAP_DISTANCE: f64 = 15.0;

/// The windows of TubeAmp, as the layout sees them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Win {
    Main,
    Eq,
    Playlist,
    Library,
    Video,
    Milkdrop,
}

/// A window's rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bx {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

impl Bx {
    pub fn new(x: f64, y: f64, w: f64, h: f64) -> Bx {
        Bx { x, y, w, h }
    }

    pub fn top(&self) -> f64 {
        self.y
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.h
    }

    pub fn left(&self) -> f64 {
        self.x
    }

    pub fn right(&self) -> f64 {
        self.x + self.w
    }

    /// The same rectangle moved by `d`.
    pub fn offset(&self, d: (f64, f64)) -> Bx {
        Bx {
            x: self.x + d.0,
            y: self.y + d.1,
            ..*self
        }
    }
}

/// Whether two coordinates are strictly closer than `snap` (Webamp's `near`).
fn near(a: f64, b: f64, snap: f64) -> bool {
    (a - b).abs() < snap
}

/// Whether the boxes overlap in x, allowing a `snap` gap (Webamp's `overlapX`).
fn overlap_x(a: &Bx, b: &Bx, snap: f64) -> bool {
    a.left() <= b.right() + snap && b.left() <= a.right() + snap
}

/// Whether the boxes overlap in y, allowing a `snap` gap (Webamp's `overlapY`).
fn overlap_y(a: &Bx, b: &Bx, snap: f64) -> bool {
    a.top() <= b.bottom() + snap && b.top() <= a.bottom() + snap
}

/// The positions `a` would snap to next to `b`, if any: `(x, y)` where each is `None` when that
/// axis does not snap (Webamp's `snap`).
pub fn snap(a: &Bx, b: &Bx, snap: f64) -> (Option<f64>, Option<f64>) {
    let mut x = None;
    let mut y = None;
    if overlap_y(a, b, snap) {
        if near(a.left(), b.right(), snap) {
            x = Some(b.right());
        } else if near(a.right(), b.left(), snap) {
            x = Some(b.left() - a.w);
        } else if near(a.left(), b.left(), snap) {
            x = Some(b.left());
        } else if near(a.right(), b.right(), snap) {
            x = Some(b.right() - a.w);
        }
    }
    if overlap_x(a, b, snap) {
        if near(a.top(), b.bottom(), snap) {
            y = Some(b.bottom());
        } else if near(a.bottom(), b.top(), snap) {
            y = Some(b.top() - a.h);
        } else if near(a.top(), b.top(), snap) {
            y = Some(b.top());
        } else if near(a.bottom(), b.bottom(), snap) {
            y = Some(b.bottom() - a.h);
        }
    }
    (x, y)
}

/// Whether `a` and `b` would snap to each other, which is what makes them "connected".
pub fn abuts(a: &Bx, b: &Bx, snap_px: f64) -> bool {
    let (x, y) = snap(a, b, snap_px);
    x.is_some() || y.is_some()
}

/// The move that snaps `a` to `b` (Webamp's `snapDiff`). Zero on an axis that does not snap.
pub fn snap_diff(a: &Bx, b: &Bx, snap_px: f64) -> (f64, f64) {
    let (x, y) = snap(a, b, snap_px);
    (x.map_or(0.0, |x| x - a.x), y.map_or(0.0, |y| y - a.y))
}

/// The first non-zero snap move of any moving box against any stationary one, per axis
/// (Webamp's `snapDiffManyToMany`).
pub fn snap_diff_many(moving: &[Bx], stationary: &[Bx], snap_px: f64) -> (f64, f64) {
    let mut x = 0.0;
    let mut y = 0.0;
    for a in moving {
        for b in stationary {
            let d = snap_diff(a, b, snap_px);
            if x == 0.0 {
                x = d.0;
            }
            if y == 0.0 {
                y = d.1;
            }
        }
    }
    (x, y)
}

/// The move that keeps `a` inside `screen`, when it is within `snap_px` of an edge (Webamp's
/// `snapWithinDiff`). A box larger than the screen on an axis cannot be kept inside it, so that
/// axis is left alone; otherwise a group taller than the screen would jump up as soon as it is
/// touched.
pub fn snap_within_diff(a: &Bx, screen: &Bx, snap_px: f64) -> (f64, f64) {
    let mut x = 0.0;
    let mut y = 0.0;
    if a.w <= screen.w {
        if a.x - snap_px < screen.x {
            x = screen.x - a.x;
        } else if a.right() + snap_px > screen.right() {
            x = (screen.right() - a.w) - a.x;
        }
    }
    if a.h <= screen.h {
        if a.y - snap_px < screen.y {
            y = screen.y - a.y;
        } else if a.bottom() + snap_px > screen.bottom() {
            y = (screen.bottom() - a.h) - a.y;
        }
    }
    (x, y)
}

/// Combines two moves the way Webamp's `applyMultipleDiffs` does: a zero on either axis adds,
/// two non-zero moves on the same axis keep the smaller one.
pub fn combine(m: (f64, f64), d: (f64, f64)) -> (f64, f64) {
    let axis = |m: f64, d: f64| {
        if m == 0.0 || d == 0.0 {
            m + d
        } else {
            m.min(d)
        }
    };
    (axis(m.0, d.0), axis(m.1, d.1))
}

/// The final move of a drag. `proposed` is where the pointer says the group should go, relative
/// to its start. Snapping to the stationary windows and to the screen edges changes it.
pub fn drag_diff(
    proposed: (f64, f64),
    moving: &[Bx],
    stationary: &[Bx],
    screen: &Bx,
    snap_px: f64,
) -> (f64, f64) {
    let proposed_boxes: Vec<Bx> = moving.iter().map(|b| b.offset(proposed)).collect();
    let by_windows = snap_diff_many(&proposed_boxes, stationary, snap_px);
    let bounds = bounding_box(&proposed_boxes);
    let by_screen = snap_within_diff(&bounds, screen, snap_px);
    let meta = combine(by_windows, by_screen);
    (proposed.0 + meta.0, proposed.1 + meta.1)
}

/// Snaps the right and bottom edges of a window being resized to the edges of the other windows
/// and of the screen, when they are close. The left and top edges stay where they are.
pub fn snap_resize(rect: &Bx, others: &[Bx], screen: &Bx, snap_px: f64) -> Bx {
    let mut right = rect.right();
    let mut bottom = rect.bottom();
    let mut right_targets: Vec<f64> = vec![screen.right()];
    let mut bottom_targets: Vec<f64> = vec![screen.bottom()];
    for o in others {
        if overlap_y(rect, o, snap_px) {
            right_targets.push(o.left());
            right_targets.push(o.right());
        }
        if overlap_x(rect, o, snap_px) {
            bottom_targets.push(o.top());
            bottom_targets.push(o.bottom());
        }
    }
    if let Some(t) = nearest_within(right, &right_targets, snap_px) {
        right = t;
    }
    if let Some(t) = nearest_within(bottom, &bottom_targets, snap_px) {
        bottom = t;
    }
    Bx {
        w: right - rect.x,
        h: bottom - rect.y,
        ..*rect
    }
}

/// The target closest to `value` among those strictly within `snap_px`, if any.
fn nearest_within(value: f64, targets: &[f64], snap_px: f64) -> Option<f64> {
    targets
        .iter()
        .copied()
        .filter(|&t| near(value, t, snap_px))
        .min_by(|a, b| (a - value).abs().total_cmp(&(b - value).abs()))
}

/// The smallest box that holds all of `boxes`. Panics on an empty slice, which callers never pass.
pub fn bounding_box(boxes: &[Bx]) -> Bx {
    let mut top = f64::INFINITY;
    let mut left = f64::INFINITY;
    let mut bottom = f64::NEG_INFINITY;
    let mut right = f64::NEG_INFINITY;
    for b in boxes {
        top = top.min(b.top());
        left = left.min(b.left());
        bottom = bottom.max(b.bottom());
        right = right.max(b.right());
    }
    Bx::new(left, top, right - left, bottom - top)
}

/// Every window transitively connected to `start` (Webamp's `traceConnection` with `abuts`). The
/// start window is included.
pub fn connected_group(all: &[(Win, Bx)], start: Win, snap_px: f64) -> BTreeSet<Win> {
    let mut group = BTreeSet::new();
    let Some(&(_, start_box)) = all.iter().find(|(w, _)| *w == start) else {
        return group;
    };
    group.insert(start);
    let mut stack = vec![start_box];
    while let Some(node) = stack.pop() {
        for (w, b) in all {
            if !group.contains(w) && abuts(b, &node, snap_px) {
                group.insert(*w);
                stack.push(*b);
            }
        }
    }
    group
}

/// Which window touches each window's top edge and left edge: Webamp's `generateGraph`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Edges {
    /// The window whose bottom edge meets this window's top edge.
    pub top: Option<Win>,
    /// The window whose right edge meets this window's left edge.
    pub left: Option<Win>,
}

/// Edge-to-edge neighbours, exactly touching and overlapping. Positions are integers in physical
/// pixels, so the touching test is exact.
pub fn adjacency(all: &[(Win, Bx)]) -> HashMap<Win, Edges> {
    let same = |a: f64, b: f64| (a - b).abs() < 0.5;
    let mut graph = HashMap::new();
    for (w, b) in all {
        let mut edges = Edges::default();
        for (o, ob) in all {
            if o != w && same(ob.bottom(), b.top()) {
                let is_left = ob.right() < b.left();
                let is_right = ob.left() > b.right();
                if !is_left && !is_right {
                    edges.top = Some(*o);
                    break;
                }
            }
        }
        for (o, ob) in all {
            if o != w && same(ob.right(), b.left()) {
                let is_above = ob.bottom() < b.top();
                let is_below = ob.top() > b.bottom();
                if !is_above && !is_below {
                    edges.left = Some(*o);
                    break;
                }
            }
        }
        graph.insert(*w, edges);
    }
    graph
}

/// How far each window moves when the windows change size: Webamp's `getPositionDiff`. A window
/// below a resized window moves by its height change, and a window to its right by its width
/// change, all the way down or along the chain.
pub fn position_diff(
    graph: &HashMap<Win, Edges>,
    size_diff: &HashMap<Win, (f64, f64)>,
) -> HashMap<Win, (f64, f64)> {
    // `below[w]` lists the windows under `w`; `right[w]` lists the windows to its right.
    let mut below: HashMap<Win, Vec<Win>> = HashMap::new();
    let mut right: HashMap<Win, Vec<Win>> = HashMap::new();
    let mut diff: HashMap<Win, (f64, f64)> = graph.keys().map(|w| (*w, (0.0, 0.0))).collect();
    for (w, edges) in graph {
        if let Some(t) = edges.top {
            below.entry(t).or_default().push(*w);
        }
        if let Some(l) = edges.left {
            right.entry(l).or_default().push(*w);
        }
    }

    fn walk_down(
        key: Win,
        below: &HashMap<Win, Vec<Win>>,
        size_diff: &HashMap<Win, (f64, f64)>,
        diff: &mut HashMap<Win, (f64, f64)>,
    ) {
        let Some(children) = below.get(&key) else {
            return;
        };
        let own_h = size_diff.get(&key).map_or(0.0, |s| s.1);
        for child in children {
            let before = diff[&key].1;
            diff.get_mut(child).expect("every window has an entry").1 += own_h + before;
            walk_down(*child, below, size_diff, diff);
        }
    }

    fn walk_right(
        key: Win,
        right: &HashMap<Win, Vec<Win>>,
        size_diff: &HashMap<Win, (f64, f64)>,
        diff: &mut HashMap<Win, (f64, f64)>,
    ) {
        let Some(children) = right.get(&key) else {
            return;
        };
        let own_w = size_diff.get(&key).map_or(0.0, |s| s.0);
        for child in children {
            let before = diff[&key].0;
            diff.get_mut(child).expect("every window has an entry").0 += own_w + before;
            walk_right(*child, right, size_diff, diff);
        }
    }

    for (w, edges) in graph {
        if edges.top.is_none() {
            walk_down(*w, &below, size_diff, &mut diff);
        }
        if edges.left.is_none() {
            walk_right(*w, &right, size_diff, &mut diff);
        }
    }
    diff
}

#[cfg(test)]
mod tests {
    use super::*;

    const S: f64 = SNAP_DISTANCE;

    fn b(x: f64, y: f64, w: f64, h: f64) -> Bx {
        Bx::new(x, y, w, h)
    }

    #[test]
    fn near_is_strict_at_the_snap_distance() {
        assert!(near(0.0, 14.9, S));
        assert!(!near(0.0, 15.0, S));
        assert!(!near(0.0, -15.0, S));
    }

    #[test]
    fn a_box_snaps_its_left_edge_to_another_right_edge() {
        let main = b(0.0, 0.0, 275.0, 116.0);
        // Its top is 5 px from main's top, so the y axis snaps too.
        let a = b(279.0, 5.0, 275.0, 116.0);
        assert_eq!(snap(&a, &main, S), (Some(275.0), Some(0.0)));
        assert_eq!(snap_diff(&a, &main, S), (-4.0, -5.0));
        // Well away from main's top: only the x axis snaps.
        let lower = b(279.0, 60.0, 275.0, 116.0);
        assert_eq!(snap(&lower, &main, S), (Some(275.0), None));
    }

    #[test]
    fn a_box_snaps_its_top_edge_to_another_bottom_edge() {
        let main = b(0.0, 0.0, 275.0, 116.0);
        let eq = b(3.0, 120.0, 275.0, 116.0);
        assert_eq!(snap(&eq, &main, S), (Some(0.0), Some(116.0)));
    }

    #[test]
    fn no_snap_across_a_gap_or_without_overlap() {
        let main = b(0.0, 0.0, 275.0, 116.0);
        // 20 px gap: outside the snap distance on x, and so not overlapping on x either.
        let far = b(295.0, 0.0, 275.0, 116.0);
        assert_eq!(snap(&far, &main, S), (None, None));
        assert!(!abuts(&far, &main, S));
        // Far below, and not overlapping in x: no y snap either.
        let below = b(900.0, 300.0, 100.0, 100.0);
        assert_eq!(snap(&below, &main, S), (None, None));
        assert!(!abuts(&below, &main, S));
    }

    #[test]
    fn snap_is_scaled_with_the_distance() {
        let main = b(0.0, 0.0, 275.0, 116.0);
        let a = b(560.0, 0.0, 275.0, 116.0);
        // 560 is 285 from the main's right edge at 275: no snap at 15, snap at 300.
        assert_eq!(snap(&a, &main, S).0, None);
        assert_eq!(snap(&a, &main, 300.0).0, Some(275.0));
    }

    #[test]
    fn stationary_snap_moves_the_dragged_group() {
        let main = b(0.0, 0.0, 275.0, 116.0);
        // 5 px from main's right edge, and 15 px below its bottom edge: still overlapping within
        // the snap gap, but not near enough to snap on y. Only x snaps.
        let moving = [b(280.0, 131.0, 275.0, 116.0)];
        let d = snap_diff_many(&moving, &[main], S);
        assert_eq!(d, (-5.0, 0.0));
        // 14 px below: y snaps as well.
        let close = [b(280.0, 130.0, 275.0, 116.0)];
        assert_eq!(snap_diff_many(&close, &[main], S), (-5.0, -14.0));
    }

    #[test]
    fn snap_within_keeps_a_box_on_screen() {
        let screen = b(0.0, 0.0, 1000.0, 800.0);
        assert_eq!(
            snap_within_diff(&b(5.0, 5.0, 100.0, 100.0), &screen, S),
            (-5.0, -5.0)
        );
        assert_eq!(
            snap_within_diff(&b(900.0, 700.0, 100.0, 100.0), &screen, S),
            (0.0, 0.0)
        );
        // Right edge 10 px from the screen edge: pushed to the edge.
        assert_eq!(
            snap_within_diff(&b(890.0, 300.0, 100.0, 100.0), &screen, S),
            (10.0, 0.0)
        );
        // Far from any edge: nothing.
        assert_eq!(
            snap_within_diff(&b(300.0, 300.0, 100.0, 100.0), &screen, S),
            (0.0, 0.0)
        );
    }

    #[test]
    fn a_box_taller_than_the_screen_is_not_pushed_on_that_axis() {
        let screen = b(0.0, 0.0, 1000.0, 800.0);
        // 928 tall on an 800 screen: no vertical move, but the horizontal snap still applies.
        let tall = b(890.0, 300.0, 100.0, 928.0);
        assert_eq!(snap_within_diff(&tall, &screen, S), (10.0, 0.0));
        // Wider than the screen: no horizontal move either.
        let wide = b(-5.0, 300.0, 1100.0, 100.0);
        assert_eq!(snap_within_diff(&wide, &screen, S), (0.0, 0.0));
    }

    #[test]
    fn combine_adds_on_an_axis_with_a_zero_and_keeps_the_smaller_otherwise() {
        assert_eq!(combine((3.0, 0.0), (0.0, -2.0)), (3.0, -2.0));
        assert_eq!(combine((3.0, -4.0), (-1.0, 2.0)), (-1.0, -4.0));
        assert_eq!(combine((0.0, 0.0), (0.0, 0.0)), (0.0, 0.0));
    }

    #[test]
    fn drag_diff_snaps_to_a_neighbour_and_to_the_screen() {
        let screen = b(0.0, 0.0, 1000.0, 800.0);
        let main = b(0.0, 0.0, 275.0, 116.0);
        // Pointer says (+283, +127): the window is 8 px from main's right edge and
        // 11 px from main's bottom edge, so it snaps to both.
        let moving = [b(0.0, 0.0, 275.0, 116.0)];
        let d = drag_diff((283.0, 127.0), &moving, &[main], &screen, S);
        assert_eq!(d, (275.0, 116.0));
    }

    #[test]
    fn group_bounding_box_covers_every_member() {
        let bb = bounding_box(&[b(10.0, 20.0, 100.0, 50.0), b(0.0, 60.0, 30.0, 10.0)]);
        assert_eq!(bb, b(0.0, 20.0, 110.0, 50.0));
    }

    fn windows() -> Vec<(Win, Bx)> {
        vec![
            (Win::Main, b(100.0, 100.0, 275.0, 116.0)),
            (Win::Eq, b(100.0, 216.0, 275.0, 116.0)),
            (Win::Playlist, b(100.0, 332.0, 275.0, 232.0)),
            // 8 px right of main: a gap, but within the snap distance.
            (Win::Library, b(383.0, 100.0, 300.0, 200.0)),
            // Far away: not connected.
            (Win::Video, b(1500.0, 800.0, 275.0, 232.0)),
        ]
    }

    #[test]
    fn connected_group_follows_touching_windows() {
        let group = connected_group(&windows(), Win::Main, S);
        assert_eq!(
            group.into_iter().collect::<Vec<_>>(),
            vec![Win::Main, Win::Eq, Win::Playlist, Win::Library]
        );
    }

    #[test]
    fn a_detached_window_forms_its_own_group() {
        let group = connected_group(&windows(), Win::Video, S);
        assert_eq!(group.into_iter().collect::<Vec<_>>(), vec![Win::Video]);
    }

    #[test]
    fn adjacency_finds_the_window_above_and_to_the_left() {
        let g = adjacency(&windows());
        assert_eq!(g[&Win::Main].top, None);
        assert_eq!(g[&Win::Eq].top, Some(Win::Main));
        assert_eq!(g[&Win::Playlist].top, Some(Win::Eq));
        // The library has a gap, so it is not an exact neighbour.
        assert_eq!(g[&Win::Library].left, None);
    }

    #[test]
    fn shading_main_moves_the_windows_under_it_up() {
        let before = windows();
        let g = adjacency(&before);
        let mut size_diff = HashMap::new();
        // 116 → 14 px tall, all of it through the window's own size.
        size_diff.insert(Win::Main, (0.0, -102.0));
        let d = position_diff(&g, &size_diff);
        assert_eq!(d[&Win::Main], (0.0, 0.0));
        assert_eq!(d[&Win::Eq], (0.0, -102.0));
        assert_eq!(d[&Win::Playlist], (0.0, -102.0));
        assert_eq!(d[&Win::Video], (0.0, 0.0));
    }

    #[test]
    fn doubling_moves_a_window_to_the_right_by_the_width_change() {
        let before = [
            (Win::Main, b(0.0, 0.0, 275.0, 116.0)),
            (Win::Library, b(275.0, 0.0, 300.0, 200.0)),
        ];
        let g = adjacency(&before);
        let mut size_diff = HashMap::new();
        size_diff.insert(Win::Main, (275.0, 116.0));
        let d = position_diff(&g, &size_diff);
        assert_eq!(d[&Win::Library], (275.0, 0.0));
    }

    #[test]
    fn position_diff_accumulates_along_a_chain() {
        // Main, EQ and playlist stacked. Main grows by 10 and EQ by 4: the playlist moves by both.
        let before = windows();
        let g = adjacency(&before);
        let mut size_diff = HashMap::new();
        size_diff.insert(Win::Main, (0.0, 10.0));
        size_diff.insert(Win::Eq, (0.0, 4.0));
        let d = position_diff(&g, &size_diff);
        assert_eq!(d[&Win::Eq], (0.0, 10.0));
        assert_eq!(d[&Win::Playlist], (0.0, 14.0));
    }

    #[test]
    fn snap_resize_moves_the_right_edge_onto_a_neighbour() {
        let screen = b(0.0, 0.0, 1000.0, 800.0);
        let rect = b(100.0, 100.0, 300.0, 232.0);
        let other = b(412.0, 0.0, 100.0, 90.0);
        // The right edge is at 400, 12 px from the other window's left edge at 412.
        let snapped = snap_resize(&rect, &[other], &screen, S);
        assert_eq!(snapped.right(), 412.0);
        assert_eq!(snapped.bottom(), 332.0);
    }

    #[test]
    fn snap_resize_leaves_far_edges_alone() {
        let screen = b(0.0, 0.0, 1000.0, 800.0);
        let rect = b(100.0, 100.0, 300.0, 232.0);
        let snapped = snap_resize(&rect, &[], &screen, S);
        assert_eq!(snapped, rect);
    }
}
