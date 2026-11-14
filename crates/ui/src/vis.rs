//! The main window's visualiser: spectrum bars, oscilloscope and the dotted background.
//!
//! Layout and colours follow Webamp's `js/components/Vis.tsx` and `js/components/VisPainter.ts`
//! (MIT, © Jordan Eldredge), using `VISCOLOR.TXT` entries exactly as Webamp indexes them:
//! 0 background, 1 dot grid, 2..=17 bar gradient, 18..=22 oscilloscope, 23 peak.

use crate::canvas::{Canvas, Rect};
use crate::main_window::{MainState, Status};
use tubeamp_audio::vis::{VisFrame, COLUMNS, ROWS};
use tubeamp_skin::Skin;

/// The vis area in main-window pixels (Webamp `#visualizer`: left 24, top 43, 76 × 16).
pub const VIS_RECT: Rect = Rect::new(24, 43, 76, 16);

const ROWS_I32: i32 = ROWS as i32;

/// What the vis area shows. Clicking it cycles through the modes in Webamp's order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VisMode {
    #[default]
    Spectrum,
    Oscilloscope,
    Off,
}

impl VisMode {
    /// The mode after a click: Spectrum, then Oscilloscope, then Off, then Spectrum.
    pub fn next(self) -> VisMode {
        match self {
            VisMode::Spectrum => VisMode::Oscilloscope,
            VisMode::Oscilloscope => VisMode::Off,
            VisMode::Off => VisMode::Spectrum,
        }
    }
}

/// Draws the vis area for the current frame. Draws nothing while not playing (Webamp clears the
/// canvas then, so the window background shows through) or when the mode is `Off`.
///
/// Bars and peaks are drawn in the un-doubled main window. Webamp's gradient shifts by two rows
/// (`pushDown`) unless the window is doubled.
pub fn render_vis(skin: &Skin, st: &MainState, vis: &VisFrame, canvas: &mut Canvas) {
    if st.status != Status::Play || st.vis_mode == VisMode::Off {
        return;
    }
    let colors = &skin.viscolor;
    paint_background(colors, canvas);
    let push_down = if st.doubled { 0 } else { 2 };
    match st.vis_mode {
        VisMode::Spectrum => paint_bars(colors, vis, push_down, canvas),
        VisMode::Oscilloscope => paint_scope(colors, vis, push_down, canvas),
        VisMode::Off => {}
    }
}

/// Writes one pixel at `(x, y)` relative to the vis area, clipped to the area and the canvas.
fn put(canvas: &mut Canvas, x: i32, y: i32, color: u32) {
    if x < 0 || y < 0 || x >= VIS_RECT.w || y >= VIS_RECT.h {
        return;
    }
    let (cx, cy) = (VIS_RECT.x + x, VIS_RECT.y + y);
    if cx < 0 || cy < 0 || cx as u32 >= canvas.w || cy as u32 >= canvas.h {
        return;
    }
    let i = (cy as u32 * canvas.w + cx as u32) as usize;
    canvas.px[i] = color;
}

/// Webamp `preRenderBg`: colour 0 everywhere, colour 1 on every second pixel of every second row.
fn paint_background(colors: &[u32; 24], canvas: &mut Canvas) {
    for y in 0..VIS_RECT.h {
        for x in 0..VIS_RECT.w {
            let color = if x % 2 == 0 && y % 2 == 1 {
                colors[1]
            } else {
                colors[0]
            };
            put(canvas, x, y, color);
        }
    }
}

/// Webamp's "wide" spectrum: 19 bars of 3 px, each followed by a 1 px gap except the last.
/// Columns 3, 7, ..., 71 are the gaps.
fn paint_bars(colors: &[u32; 24], vis: &VisFrame, push_down: i32, canvas: &mut Canvas) {
    for (col, bar) in vis.bars.iter().enumerate() {
        if col % 4 == 3 {
            continue;
        }
        let x = col as i32;
        let height = i32::from(bar.height) - push_down;
        if height > 0 {
            for row in (ROWS_I32 - height)..ROWS_I32 {
                // Webamp's bar canvas: row r has colour `2 - pushDown + r`.
                let color = colors[(2 - push_down + row) as usize];
                put(canvas, x, row, color);
            }
        }
        if let Some(peak) = bar.peak {
            let row = ROWS_I32 - (i32::from(peak) + 1 - push_down);
            put(canvas, x, row, colors[23]);
        }
    }
}

/// Colour slot (0..=4, added to 18) for an oscilloscope row, as in Webamp's `colorIndex`.
fn scope_colour_index(y: i32) -> usize {
    match y {
        14.. => 4,
        12..=13 => 3,
        10..=11 => 2,
        8..=9 => 1,
        6..=7 => 0,
        4..=5 => 1,
        2..=3 => 2,
        _ => 3,
    }
}

/// Webamp's "lines" oscilloscope: one column per sample, each joined to the previous column.
fn paint_scope(colors: &[u32; 24], vis: &VisFrame, push_down: i32, canvas: &mut Canvas) {
    let mut last = 0;
    for (x, &value) in vis.scope.iter().enumerate().take(COLUMNS) {
        let y = i32::from(value);
        if x == 0 {
            last = y;
        }
        let (mut top, mut bottom) = (y, last);
        last = y;
        if bottom < top {
            top = bottom + 1;
            bottom = y;
        }
        let color = colors[18 + scope_colour_index(y)];
        for row in top..=bottom {
            put(canvas, x as i32, row + push_down, color);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::main_window::{render_main, HEIGHT, WIDTH};
    use tubeamp_audio::vis::{Bar, Vis};
    use tubeamp_skin::testkit;

    fn playing() -> MainState {
        MainState {
            status: Status::Play,
            title: "x".to_string(),
            ..MainState::default()
        }
    }

    fn px(c: &Canvas, x: i32, y: i32) -> u32 {
        c.px[(y as u32 * c.w + x as u32) as usize]
    }

    /// Vis-area pixel `(x, y)` relative to the area.
    fn vis_px(c: &Canvas, x: i32, y: i32) -> u32 {
        px(c, VIS_RECT.x + x, VIS_RECT.y + y)
    }

    fn render(st: &MainState, vis: &VisFrame, skin: &Skin) -> Canvas {
        let mut c = Canvas::new(WIDTH, HEIGHT);
        render_main(skin, st, &mut c);
        render_vis(skin, st, vis, &mut c);
        c
    }

    fn full_bars() -> VisFrame {
        let mut f = VisFrame::default();
        for b in f.bars.iter_mut() {
            *b = Bar {
                height: 15,
                peak: None,
            };
        }
        f
    }

    #[test]
    fn modes_cycle_in_webamp_order() {
        assert_eq!(VisMode::Spectrum.next(), VisMode::Oscilloscope);
        assert_eq!(VisMode::Oscilloscope.next(), VisMode::Off);
        assert_eq!(VisMode::Off.next(), VisMode::Spectrum);
    }

    #[test]
    fn nothing_is_drawn_while_stopped_paused_or_off() {
        let skin = testkit::synthetic_skin();
        let frame = full_bars();
        for st in [
            MainState {
                status: Status::Stop,
                ..playing()
            },
            MainState {
                status: Status::Pause,
                ..playing()
            },
            MainState {
                vis_mode: VisMode::Off,
                ..playing()
            },
        ] {
            let mut bg = Canvas::new(WIDTH, HEIGHT);
            render_main(&skin, &st, &mut bg);
            assert_eq!(render(&st, &frame, &skin), bg);
        }
    }

    #[test]
    fn background_has_the_dot_grid_in_colour_one() {
        let skin = testkit::synthetic_skin();
        let c = render(&playing(), &VisFrame::default(), &skin);
        assert_eq!(vis_px(&c, 0, 1), skin.viscolor[1]);
        assert_eq!(vis_px(&c, 2, 3), skin.viscolor[1]);
        assert_eq!(vis_px(&c, 1, 1), skin.viscolor[0]);
        assert_eq!(vis_px(&c, 0, 0), skin.viscolor[0]);
    }

    #[test]
    fn full_bars_use_the_gradient_and_leave_gaps() {
        let skin = testkit::synthetic_skin();
        let c = render(&playing(), &full_bars(), &skin);
        // Not doubled: row r has colour 2 - 2 + r = r. A height-15 bar drawn in pushDown 2 is
        // 13 rows, so it fills rows 3..=15.
        assert_eq!(vis_px(&c, 0, 2), skin.viscolor[0]);
        for row in 3..ROWS_I32 {
            assert_eq!(vis_px(&c, 0, row), skin.viscolor[row as usize], "row {row}");
        }
        // Column 3 is a gap: it keeps the background.
        assert_eq!(vis_px(&c, 3, 15), skin.viscolor[0]);
        assert_eq!(vis_px(&c, 3, 15), vis_px(&c, 3, 14));
        // Column 4 starts the next bar.
        assert_eq!(vis_px(&c, 4, 15), skin.viscolor[15]);
    }

    #[test]
    fn doubled_window_shifts_the_gradient_to_colours_two_to_seventeen() {
        let skin = testkit::synthetic_skin();
        let st = MainState {
            doubled: true,
            ..playing()
        };
        let c = render(&st, &full_bars(), &skin);
        // Height 15 with pushDown 0 fills rows 1..=15, colours 2 + r.
        assert_eq!(vis_px(&c, 0, 0), skin.viscolor[0]);
        assert_eq!(vis_px(&c, 0, 1), skin.viscolor[3]);
        assert_eq!(vis_px(&c, 0, 15), skin.viscolor[17]);
    }

    #[test]
    fn peak_is_drawn_in_colour_twenty_three() {
        let skin = testkit::synthetic_skin();
        let mut frame = VisFrame::default();
        frame.bars[0] = Bar {
            height: 2,
            peak: Some(10),
        };
        let c = render(&playing(), &frame, &skin);
        // Peak row = 16 - (10 + 1 - 2) = 7.
        assert_eq!(vis_px(&c, 0, 7), skin.viscolor[23]);
        // The bar itself is 2 - 2 = 0 rows high, so nothing below the peak.
        assert_eq!(
            vis_px(&c, 0, 8),
            vis_px(&render(&playing(), &VisFrame::default(), &skin), 0, 8)
        );
    }

    #[test]
    fn oscilloscope_draws_one_connected_line() {
        let skin = testkit::synthetic_skin();
        let st = MainState {
            vis_mode: VisMode::Oscilloscope,
            ..playing()
        };
        let mut frame = VisFrame::default();
        // Column 0 at row 4, column 1 at row 10: the lines join them, rows 5..=10 at column 1.
        frame.scope[0] = 4;
        frame.scope[1] = 10;
        let c = render(&st, &frame, &skin);
        // Row y is drawn at y + 2 (pushDown when not doubled).
        assert_eq!(vis_px(&c, 0, 6), skin.viscolor[18 + scope_colour_index(4)]);
        for y in 5..=10 {
            assert_eq!(
                vis_px(&c, 1, y + 2),
                skin.viscolor[18 + scope_colour_index(10)],
                "row {y}"
            );
        }
    }

    #[test]
    fn scope_colour_indices_match_webamp() {
        assert_eq!(scope_colour_index(15), 4);
        assert_eq!(scope_colour_index(12), 3);
        assert_eq!(scope_colour_index(10), 2);
        assert_eq!(scope_colour_index(8), 1);
        assert_eq!(scope_colour_index(6), 0);
        assert_eq!(scope_colour_index(4), 1);
        assert_eq!(scope_colour_index(2), 2);
        assert_eq!(scope_colour_index(0), 3);
    }

    #[test]
    fn live_analyser_output_renders_without_panicking() {
        let skin = testkit::synthetic_skin();
        let mut vis = Vis::new();
        let samples: Vec<f32> = (0..512).map(|i| (i as f32 * 0.3).sin()).collect();
        let frame = vis.update(&samples).clone();
        let st = MainState {
            doubled: true,
            ..playing()
        };
        let c = render(&st, &frame, &skin);
        assert_eq!(c.w, WIDTH);
    }
}
