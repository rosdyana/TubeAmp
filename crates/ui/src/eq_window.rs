//! Equalizer window (275×116): rendering, hit testing and slider math.
//!
//! Element rectangles come from Webamp's `css/equalizer-window.css`. The band frame and thumb
//! rules come from `js/components/EqualizerWindow/Band.tsx` and `VerticalSlider.tsx`, the graph
//! from `EqGraph.tsx` and `spline.js`, and the drag snapping from
//! `js/actionCreators/equalizer.ts` (MIT, © Jordan Eldredge).
//!
//! Webamp's sliders run 0..=100 while our model runs 1..=64 (33 = 0 dB). Frames and thumb
//! positions are computed on Webamp's scale, converted at the boundary by [`webamp_level`] and
//! [`value_from_level`].

use crate::canvas::{Canvas, Rect};
use tubeamp_eq::model::{MAX_VALUE, MIN_VALUE};
use tubeamp_eq::EqSettings;
use tubeamp_skin::{Sheet, Skin};

/// Window width in pixels (1×).
pub const WIDTH: u32 = 275;
/// Window height in pixels (1×).
pub const HEIGHT: u32 = 116;
/// Height of the windowshade (1×).
pub const SHADE_HEIGHT: u32 = 14;

/// The window's height in skin pixels for its shade state.
pub fn window_height(shade: bool) -> u32 {
    if shade {
        SHADE_HEIGHT
    } else {
        HEIGHT
    }
}

const TITLE_BAR: Rect = Rect::new(0, 0, 275, 14);
const CLOSE_BTN: Rect = Rect::new(264, 3, 9, 9);
/// The windowshade button (`#equalizer-shade`), in both modes.
const SHADE_BTN: Rect = Rect::new(254, 3, 9, 9);
/// The shade's volume and balance tracks (`#equalizer-volume` and `#equalizer-balance`). The
/// thumbs are 3 px wide, so they travel 3 px less than the track.
const SHADE_VOLUME: Rect = Rect::new(61, 4, 97, 6);
const SHADE_BALANCE: Rect = Rect::new(164, 4, 43, 6);
const SHADE_THUMB_W: f64 = 3.0;
const ON_BTN: Rect = Rect::new(14, 18, 26, 12);
const AUTO_BTN: Rect = Rect::new(40, 18, 32, 12);
/// The PRESETS button (`#presets-context`).
pub const PRESETS_BTN: Rect = Rect::new(217, 18, 44, 12);
/// The 113×19 response graph (`#eqGraph`). It takes no clicks.
pub const GRAPH: Rect = Rect::new(86, 17, 113, 19);

/// Slider element width (Webamp `.band`).
const SLIDER_W: i32 = 14;
/// Slider track height (Webamp `VerticalSlider height={62}`; the `.band` element is 63 tall).
const SLIDER_H: i32 = 62;
/// Top of every slider.
const SLIDER_Y: i32 = 38;
const PREAMP_X: i32 = 21;
const BAND_X: i32 = 78;
const BAND_PITCH: i32 = 18;
/// Thumb size (`.slider-handle`).
const HANDLE: i32 = 11;
/// Webamp's `MAX_VALUE`: slider levels run 0..=100.
const LEVEL_MAX: f64 = 100.0;
/// Graph height in pixels (`GRAPH_HEIGHT`), and its largest row index.
const GRAPH_ROWS: f64 = 18.0;

/// The preamp or one of the ten bands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqSlider {
    Preamp,
    /// Band index `0..=9`, in `EqSettings::bands` order (60 Hz … 16 kHz).
    Band(usize),
}

/// The clickable parts of the equalizer window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqHit {
    TitleBar,
    Close,
    /// The windowshade button.
    Shade,
    On,
    Auto,
    Presets,
    Slider(EqSlider),
    /// The shade's mini volume slider.
    ShadeVolume,
    /// The shade's mini balance slider.
    ShadeBalance,
}

/// Everything the equalizer window shows.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EqWindowState {
    pub settings: EqSettings,
    pub focused: bool,
    /// The windowshade: a 275×14 strip with the title buttons and mini volume and balance.
    pub shade: bool,
    /// The volume and balance shown in shade mode (the main window's values).
    pub volume: u8,
    pub balance: i8,
    /// The control held down, drawn with its pressed sprite.
    pub pressed: Option<EqHit>,
    /// The slider being dragged. Its thumb is drawn selected.
    pub dragging: Option<EqSlider>,
}

/// `(x, y)` of a slider's track, and its size.
pub fn slider_rect(slider: EqSlider) -> Rect {
    match slider {
        EqSlider::Preamp => Rect::new(PREAMP_X, SLIDER_Y, SLIDER_W, SLIDER_H),
        EqSlider::Band(i) => Rect::new(BAND_X + BAND_PITCH * i as i32, SLIDER_Y, SLIDER_W, SLIDER_H),
    }
}

/// All sliders, preamp first.
pub fn all_sliders() -> impl Iterator<Item = EqSlider> {
    std::iter::once(EqSlider::Preamp).chain((0..10).map(EqSlider::Band))
}

/// Which hit target (if any) is at window point `(x, y)`.
pub fn hit_test_eq(x: i32, y: i32) -> Option<EqHit> {
    if CLOSE_BTN.contains(x, y) {
        return Some(EqHit::Close);
    }
    if SHADE_BTN.contains(x, y) {
        return Some(EqHit::Shade);
    }
    if ON_BTN.contains(x, y) {
        return Some(EqHit::On);
    }
    if AUTO_BTN.contains(x, y) {
        return Some(EqHit::Auto);
    }
    if PRESETS_BTN.contains(x, y) {
        return Some(EqHit::Presets);
    }
    if let Some(slider) = all_sliders().find(|&s| slider_rect(s).contains(x, y)) {
        return Some(EqHit::Slider(slider));
    }
    if GRAPH.contains(x, y) {
        return None;
    }
    TITLE_BAR.contains(x, y).then_some(EqHit::TitleBar)
}

/// Which hit target (if any) is at `(x, y)` in the windowshade (275 × 14).
pub fn hit_test_eq_shade(x: i32, y: i32) -> Option<EqHit> {
    if !(0..WIDTH as i32).contains(&x) || !(0..SHADE_HEIGHT as i32).contains(&y) {
        return None;
    }
    if CLOSE_BTN.contains(x, y) {
        return Some(EqHit::Close);
    }
    if SHADE_BTN.contains(x, y) {
        return Some(EqHit::Shade);
    }
    if SHADE_VOLUME.contains(x, y) {
        return Some(EqHit::ShadeVolume);
    }
    if SHADE_BALANCE.contains(x, y) {
        return Some(EqHit::ShadeBalance);
    }
    Some(EqHit::TitleBar)
}

/// The volume (`0..=100`) for a pointer at window `x` on the shade's volume track, when dragged.
pub fn shade_volume_from_x(x: f64) -> u8 {
    let travel = f64::from(SHADE_VOLUME.w) - SHADE_THUMB_W;
    let rel = (x - f64::from(SHADE_VOLUME.x) - SHADE_THUMB_W / 2.0) / travel;
    (rel.clamp(0.0, 1.0) * 100.0).round() as u8
}

/// The balance (`-100..=100`) for a pointer at window `x` on the shade's balance track.
pub fn shade_balance_from_x(x: f64) -> i8 {
    let travel = f64::from(SHADE_BALANCE.w) - SHADE_THUMB_W;
    let rel = (x - f64::from(SHADE_BALANCE.x) - SHADE_THUMB_W / 2.0) / travel;
    (rel.clamp(0.0, 1.0) * 200.0 - 100.0).round() as i8
}

/// JavaScript's `Math.round`, which rounds halves towards +Infinity.
fn js_round(x: f64) -> f64 {
    (x + 0.5).floor()
}

/// Webamp level (0..=100) for one of our slider values. Piecewise so that our
/// 0 dB centre (33) lands exactly on Webamp's centre (50): 32 steps below,
/// 31 above, mirroring `tubeamp_eq::value_to_db`.
pub fn webamp_level(v: u8) -> u32 {
    let v = f64::from(v.clamp(MIN_VALUE, MAX_VALUE));
    let half = LEVEL_MAX / 2.0;
    let level = if v <= 33.0 { (v - 1.0) / 32.0 * half } else { half + (v - 33.0) / 31.0 * half };
    js_round(level) as u32
}

/// Our slider value (1..=64) for a Webamp level; inverse of [`webamp_level`].
pub fn value_from_level(level: f64) -> u8 {
    let half = LEVEL_MAX / 2.0;
    let level = level.clamp(0.0, LEVEL_MAX);
    let v = if level <= half { 1.0 + js_round(level / half * 32.0) } else { 33.0 + js_round((level - half) / half * 31.0) };
    v.clamp(f64::from(MIN_VALUE), f64::from(MAX_VALUE)) as u8
}

/// Snaps a Webamp level strictly within 5 of 50 to 50 (`_snapBandValue`).
pub fn snap_level(level: f64) -> f64 {
    if level < 55.0 && level > 45.0 {
        50.0
    } else {
        level
    }
}

/// Webamp level of a slider's current value.
fn level_of(settings: &EqSettings, slider: EqSlider) -> u32 {
    match slider {
        EqSlider::Preamp => webamp_level(settings.preamp),
        EqSlider::Band(i) => webamp_level(settings.bands[i]),
    }
}

/// Thumb offset from the top of the track, for a Webamp level (`Math.floor((62 - 11) * value)`).
pub fn thumb_offset(level: u32) -> i32 {
    ((f64::from(SLIDER_H) - f64::from(HANDLE)) * (1.0 - f64::from(level) / LEVEL_MAX)).floor() as i32
}

/// Band background frame (0..=27) for a Webamp level (`spriteNumber`).
pub fn slider_frame(level: u32) -> u32 {
    js_round(f64::from(level) / LEVEL_MAX * 27.0) as u32
}

/// Grab offset for a press at `(x, y)`: the pointer's distance below the thumb's top when the
/// press lands on the thumb, otherwise the thumb's centre (Webamp `VerticalSlider`).
pub fn press_grab(slider: EqSlider, settings: &EqSettings, x: f64, y: f64) -> f64 {
    let rect = slider_rect(slider);
    let thumb_top = rect.y + thumb_offset(level_of(settings, slider));
    let thumb = Rect::new(rect.x + 1, thumb_top, HANDLE, HANDLE);
    if thumb.contains(x.floor() as i32, y.floor() as i32) {
        y - f64::from(thumb_top)
    } else {
        f64::from(HANDLE) / 2.0
    }
}

/// The slider's value for a pointer at window `y`, given the grab offset from [`press_grab`].
/// The value snaps to centre like Webamp's band setter.
pub fn slider_value_at(slider: EqSlider, y: f64, grab: f64) -> u8 {
    let rel = y - f64::from(slider_rect(slider).y);
    let span = f64::from(SLIDER_H - HANDLE);
    let t = ((rel - grab) / span).clamp(0.0, 1.0);
    value_from_level(snap_level((1.0 - t) * LEVEL_MAX))
}

/// One vertical run of the response line. `x` is 0..=108 from the graph's left padding (two
/// pixels). The run covers rows `y_top..y_top + height`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphRun {
    pub x: i32,
    pub y_top: i32,
    pub height: i32,
}

/// Graph row of the preamp line (`percentToRange(preamp / 100, 0, 18)`).
pub fn preamp_row(level: u32) -> i32 {
    js_round(f64::from(level) / LEVEL_MAX * GRAPH_ROWS) as i32
}

/// The response line through the ten band levels, as runs of pixels (`EqGraph.drawEqLine`).
pub fn graph_runs(levels: &[u32; 10]) -> Vec<GraphRun> {
    let ys: Vec<f64> = levels
        .iter()
        .map(|&v| js_round(((LEVEL_MAX - f64::from(v)) / LEVEL_MAX) * GRAPH_ROWS))
        .collect();
    let xs: Vec<f64> = (0..10).map(|i| f64::from(i * 12)).collect();
    let all_ys = spline(&xs, &ys);
    let max_x = xs[xs.len() - 1] as usize;

    let mut last_y = ys[0];
    let mut runs = Vec::with_capacity(max_x + 1);
    for (x, &raw) in all_ys.iter().enumerate().take(max_x + 1) {
        let y = js_round(raw).clamp(0.0, GRAPH_ROWS);
        let y_top = y.min(last_y);
        let height = 1.0 + (last_y - y).abs();
        runs.push(GraphRun {
            x: x as i32,
            y_top: y_top as i32,
            height: height as i32,
        });
        last_y = y;
    }
    runs
}

/// Natural cubic spline through `(xs, ys)`, evaluated at every integer `x` up to the last knot.
/// A port of Webamp's `spline.js` (adapted from morganherlocker/cubic-spline), kept in the same
/// order of operations so the rounded graph matches Webamp.
fn spline(xs: &[f64], ys: &[f64]) -> Vec<f64> {
    let ks = natural_ks(xs, ys);
    let max_x = xs[xs.len() - 1];
    let mut all_ys = Vec::new();
    let mut i = 1usize;
    let mut x = 0.0f64;
    while x <= max_x {
        while xs[i] < x {
            i += 1;
        }
        let t = (x - xs[i - 1]) / (xs[i] - xs[i - 1]);
        let a = ks[i - 1] * (xs[i] - xs[i - 1]) - (ys[i] - ys[i - 1]);
        let b = -ks[i] * (xs[i] - xs[i - 1]) + (ys[i] - ys[i - 1]);
        let q = (1.0 - t) * ys[i - 1] + t * ys[i] + t * (1.0 - t) * (a * (1.0 - t) + b * t);
        all_ys.push(q);
        x += 1.0;
    }
    all_ys
}

fn natural_ks(xs: &[f64], ys: &[f64]) -> Vec<f64> {
    let mut ks = vec![0.0; xs.len()];
    let n = xs.len() - 1;
    let mut mat = vec![vec![0.0f64; n + 2]; n + 1];

    for i in 1..n {
        mat[i][i - 1] = 1.0 / (xs[i] - xs[i - 1]);
        mat[i][i] = 2.0 * (1.0 / (xs[i] - xs[i - 1]) + 1.0 / (xs[i + 1] - xs[i]));
        mat[i][i + 1] = 1.0 / (xs[i + 1] - xs[i]);
        mat[i][n + 1] = 3.0
            * ((ys[i] - ys[i - 1]) / ((xs[i] - xs[i - 1]) * (xs[i] - xs[i - 1]))
                + (ys[i + 1] - ys[i]) / ((xs[i + 1] - xs[i]) * (xs[i + 1] - xs[i])));
    }

    mat[0][0] = 2.0 / (xs[1] - xs[0]);
    mat[0][1] = 1.0 / (xs[1] - xs[0]);
    mat[0][n + 1] = (3.0 * (ys[1] - ys[0])) / ((xs[1] - xs[0]) * (xs[1] - xs[0]));

    mat[n][n - 1] = 1.0 / (xs[n] - xs[n - 1]);
    mat[n][n] = 2.0 / (xs[n] - xs[n - 1]);
    mat[n][n + 1] = (3.0 * (ys[n] - ys[n - 1])) / ((xs[n] - xs[n - 1]) * (xs[n] - xs[n - 1]));

    solve(&mut mat, &mut ks);
    ks
}

/// Gaussian elimination with partial pivoting on the largest (signed) value, then
/// back-substitution, as in Webamp's `spline.js`. Writes the solution into `ks`.
///
/// The index loops mirror the JS row and column arithmetic on purpose, so the rounded graph
/// matches Webamp, hence the allow.
#[allow(clippy::needless_range_loop)]
fn solve(mat: &mut [Vec<f64>], ks: &mut [f64]) {
    let m = mat.len();
    for k in 0..m {
        let mut i_max = 0;
        let mut val_max = f64::NEG_INFINITY;
        for i in k..m {
            if mat[i][k] > val_max {
                i_max = i;
                val_max = mat[i][k];
            }
        }
        mat.swap(k, i_max);
        for i in (k + 1)..m {
            let factor = mat[i][k] / mat[k][k];
            for j in (k + 1)..(m + 1) {
                mat[i][j] -= mat[k][j] * factor;
            }
            mat[i][k] = 0.0;
        }
    }
    for i in (0..m).rev() {
        let v = mat[i][m] / mat[i][i];
        ks[i] = v;
        for j in (0..i).rev() {
            let term = mat[j][i] * v;
            mat[j][m] -= term;
            mat[j][i] = 0.0;
        }
    }
}

/// Draws the whole equalizer window in Webamp's layout, from the background up.
pub fn render_eq(skin: &Skin, st: &EqWindowState, canvas: &mut Canvas) {
    if st.shade {
        render_eq_shade(skin, st, canvas);
        return;
    }
    draw(skin, canvas, "EQ_WINDOW_BACKGROUND", 0, 0);
    let bar = if st.focused {
        "EQ_TITLE_BAR_SELECTED"
    } else {
        "EQ_TITLE_BAR"
    };
    draw(skin, canvas, bar, 0, 0);

    let close = if is_pressed(st, EqHit::Close) {
        "EQ_CLOSE_BUTTON_ACTIVE"
    } else {
        "EQ_CLOSE_BUTTON"
    };
    draw(skin, canvas, close, CLOSE_BTN.x, CLOSE_BTN.y);
    draw_shade_button(skin, canvas, st, false);

    let on = 2 * usize::from(st.settings.on) + usize::from(is_pressed(st, EqHit::On));
    draw(skin, canvas, ON_NAMES[on], ON_BTN.x, ON_BTN.y);
    let auto = 2 * usize::from(st.settings.auto) + usize::from(is_pressed(st, EqHit::Auto));
    draw(skin, canvas, AUTO_NAMES[auto], AUTO_BTN.x, AUTO_BTN.y);

    draw_graph(skin, canvas, &st.settings);

    let presets = if is_pressed(st, EqHit::Presets) {
        "EQ_PRESETS_BUTTON_SELECTED"
    } else {
        "EQ_PRESETS_BUTTON"
    };
    draw(skin, canvas, presets, PRESETS_BTN.x, PRESETS_BTN.y);

    for slider in all_sliders() {
        draw_slider(skin, canvas, slider, level_of(&st.settings, slider), st.dragging == Some(slider));
    }
}

/// `[off, off + pressed, on, on + pressed]`
const ON_NAMES: [&str; 4] = [
    "EQ_ON_BUTTON",
    "EQ_ON_BUTTON_DEPRESSED",
    "EQ_ON_BUTTON_SELECTED",
    "EQ_ON_BUTTON_SELECTED_DEPRESSED",
];
/// `[off, off + pressed, on, on + pressed]`
const AUTO_NAMES: [&str; 4] = [
    "EQ_AUTO_BUTTON",
    "EQ_AUTO_BUTTON_DEPRESSED",
    "EQ_AUTO_BUTTON_SELECTED",
    "EQ_AUTO_BUTTON_SELECTED_DEPRESSED",
];

fn is_pressed(st: &EqWindowState, hit: EqHit) -> bool {
    st.pressed == Some(hit)
}

fn draw(skin: &Skin, canvas: &mut Canvas, name: &str, x: i32, y: i32) {
    if let Some(view) = skin.sprite(Sheet::Eqmain, name) {
        canvas.blit(&view, x, y);
    }
}

/// Like [`draw`], from EQ_EX.BMP (the shade and the window buttons that EQMAIN lacks).
fn draw_ex(skin: &Skin, canvas: &mut Canvas, name: &str, x: i32, y: i32) {
    if let Some(view) = skin.sprite(Sheet::EqEx, name) {
        canvas.blit(&view, x, y);
    }
}

/// The windowshade button. Its normal sprite is EQMAIN's (Webamp's fallback), and the pressed one
/// is EQ_EX's: the restore glyph in the full window, the minimise glyph in shade mode.
fn draw_shade_button(skin: &Skin, canvas: &mut Canvas, st: &EqWindowState, shade: bool) {
    let x = SHADE_BTN.x;
    let y = SHADE_BTN.y;
    if is_pressed(st, EqHit::Shade) {
        let name = if shade {
            "EQ_MINIMIZE_BUTTON_ACTIVE"
        } else {
            "EQ_MAXIMIZE_BUTTON_ACTIVE"
        };
        draw_ex(skin, canvas, name, x, y);
    } else {
        draw(skin, canvas, "EQ_MAXIMIZE_BUTTON_ACTIVE_FALLBACK", x, y);
    }
}

/// The shade: EQ_EX's strip, the title buttons, and the mini volume and balance thumbs. The tracks
/// are part of the strip's background, so only the thumbs are drawn on top.
fn render_eq_shade(skin: &Skin, st: &EqWindowState, canvas: &mut Canvas) {
    let bg = if st.focused {
        "EQ_SHADE_BACKGROUND_SELECTED"
    } else {
        "EQ_SHADE_BACKGROUND"
    };
    draw_ex(skin, canvas, bg, 0, 0);
    draw_shade_button(skin, canvas, st, true);

    let close = if is_pressed(st, EqHit::Close) {
        "EQ_SHADE_CLOSE_BUTTON_ACTIVE"
    } else {
        "EQ_SHADE_CLOSE_BUTTON"
    };
    draw_ex(skin, canvas, close, CLOSE_BTN.x, CLOSE_BTN.y);

    // Thumbs take one of three sprites by the value's third (Webamp's `segment`).
    let third = |ratio: f64| ["LEFT", "CENTER", "RIGHT"][(ratio * 2.0).round() as usize];
    let vol_ratio = f64::from(st.volume.min(100)) / 100.0;
    let vol_x = SHADE_VOLUME.x + (vol_ratio * (f64::from(SHADE_VOLUME.w) - SHADE_THUMB_W)).round() as i32;
    let vol_name = format!("EQ_SHADE_VOLUME_SLIDER_{}", third(vol_ratio));
    draw_ex(skin, canvas, &vol_name, vol_x, SHADE_VOLUME.y);

    let bal = i32::from(st.balance.clamp(-100, 100));
    let bal_ratio = f64::from(bal + 100) / 200.0;
    let bal_x = SHADE_BALANCE.x + (bal_ratio * (f64::from(SHADE_BALANCE.w) - SHADE_THUMB_W)).round() as i32;
    let bal_name = format!("EQ_SHADE_BALANCE_SLIDER_{}", third(bal_ratio));
    draw_ex(skin, canvas, &bal_name, bal_x, SHADE_BALANCE.y);
}

/// One slider: the band background frame for `level`, then the thumb at its offset.
fn draw_slider(skin: &Skin, canvas: &mut Canvas, slider: EqSlider, level: u32, selected: bool) {
    let rect = slider_rect(slider);
    if let Some(bg) = skin.sprite(Sheet::Eqmain, "EQ_SLIDER_BACKGROUND") {
        // Frames are 15 px apart across and 65 px down in the 209×129 background sprite.
        let frame = slider_frame(level);
        let src = ((frame % 14) * 15, (frame / 14) * 65, SLIDER_W as u32, (SLIDER_H + 1) as u32);
        canvas.blit_region(&bg, src, rect.x, rect.y);
    }
    let thumb = if selected {
        "EQ_SLIDER_THUMB_SELECTED"
    } else {
        "EQ_SLIDER_THUMB"
    };
    draw(skin, canvas, thumb, rect.x + 1, rect.y + thumb_offset(level));
}

/// The graph: background, preamp line, then the response line one pixel per column and row,
/// coloured from the one-pixel-wide colour strip per row, as `EqGraph` fills it.
fn draw_graph(skin: &Skin, canvas: &mut Canvas, settings: &EqSettings) {
    draw(skin, canvas, "EQ_GRAPH_BACKGROUND", GRAPH.x, GRAPH.y);
    let preamp_y = preamp_row(webamp_level(settings.preamp));
    draw(skin, canvas, "EQ_PREAMP_LINE", GRAPH.x, GRAPH.y + preamp_y);

    let Some(colors) = skin.sprite(Sheet::Eqmain, "EQ_GRAPH_LINE_COLORS") else {
        return;
    };
    let levels = settings.bands.map(webamp_level);
    for run in graph_runs(&levels) {
        for row in run.y_top..run.y_top + run.height {
            // The graph's left padding is two pixels.
            canvas.blit_region(&colors, (0, row as u32, 1, 1), GRAPH.x + 2 + run.x, GRAPH.y + row);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tubeamp_skin::testkit::{self, decode_coord, encode};
    use tubeamp_skin::Rgba;

    fn synthetic() -> Skin {
        testkit::synthetic_skin()
    }

    fn render(skin: &Skin, st: &EqWindowState) -> Canvas {
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        render_eq(skin, st, &mut canvas);
        canvas
    }

    fn px(c: &Canvas, x: i32, y: i32) -> u32 {
        c.px[(y as u32 * c.w + x as u32) as usize]
    }

    fn sheet_px(skin: &Skin, x: u32, y: u32) -> u32 {
        let img = skin.sheet(Sheet::Eqmain).expect("eqmain");
        img.px[(y * img.w + x) as usize]
    }

    /// Asserts that canvas `(dx.., dy..)` shows Eqmain `(sx.., sy..)` over `size`.
    fn assert_copied(c: &Canvas, skin: &Skin, src: (u32, u32), dst: (i32, i32), size: (u32, u32)) {
        for j in 0..size.1 {
            for i in 0..size.0 {
                let want = sheet_px(skin, src.0 + i, src.1 + j);
                let got = px(c, dst.0 + i as i32, dst.1 + j as i32);
                assert_eq!(
                    got,
                    want,
                    "canvas ({},{}) should show Eqmain ({},{})",
                    dst.0 + i as i32,
                    dst.1 + j as i32,
                    src.0 + i,
                    src.1 + j
                );
            }
        }
    }

    fn flat_state() -> EqWindowState {
        EqWindowState {
            focused: true,
            ..EqWindowState::default()
        }
    }

    #[test]
    fn hit_test_finds_each_control() {
        assert_eq!(hit_test_eq(270, 5), Some(EqHit::Close));
        assert_eq!(hit_test_eq(20, 20), Some(EqHit::On));
        assert_eq!(hit_test_eq(45, 20), Some(EqHit::Auto));
        assert_eq!(hit_test_eq(230, 25), Some(EqHit::Presets));
        assert_eq!(hit_test_eq(150, 5), Some(EqHit::TitleBar));
        assert_eq!(hit_test_eq(25, 50), Some(EqHit::Slider(EqSlider::Preamp)));
        assert_eq!(hit_test_eq(80, 60), Some(EqHit::Slider(EqSlider::Band(0))));
        assert_eq!(hit_test_eq(242, 99), Some(EqHit::Slider(EqSlider::Band(9))));
    }

    #[test]
    fn hit_test_graph_and_gaps_are_empty() {
        assert_eq!(hit_test_eq(100, 25), None, "graph takes no clicks");
        assert_eq!(hit_test_eq(93, 60), None, "gap between band 0 and band 1");
        assert_eq!(hit_test_eq(5, 100), None);
        assert_eq!(hit_test_eq(275, 5), None);
        assert_eq!(hit_test_eq(-1, 5), None);
    }

    #[test]
    fn webamp_level_maps_our_scale_to_0_100() {
        assert_eq!(webamp_level(1), 0);
        assert_eq!(webamp_level(33), 50);
        assert_eq!(webamp_level(64), 100);
        assert_eq!(webamp_level(0), 0, "values below the range clamp");
    }

    #[test]
    fn value_from_level_maps_back_and_centre_is_33() {
        assert_eq!(value_from_level(0.0), 1);
        assert_eq!(value_from_level(100.0), 64);
        assert_eq!(value_from_level(50.0), 33);
        for v in 1..=64u8 {
            assert_eq!(value_from_level(f64::from(webamp_level(v))), v, "value {v}");
        }
    }

    #[test]
    fn snap_only_inside_the_dead_zone() {
        assert_eq!(snap_level(44.9), 44.9);
        assert_eq!(snap_level(45.0), 45.0);
        assert_eq!(snap_level(54.9), 50.0);
        assert_eq!(snap_level(55.0), 55.0);
    }

    #[test]
    fn slider_value_follows_the_pointer_and_clamps() {
        let track = f64::from(slider_rect(EqSlider::Preamp).y);
        let half = f64::from(HANDLE) / 2.0;
        // Top of the track is +12 dB, bottom is -12 dB, both with the grab at the thumb centre.
        assert_eq!(slider_value_at(EqSlider::Preamp, track, half), 64);
        assert_eq!(slider_value_at(EqSlider::Preamp, track + 200.0, half), 1);
        // The centre of the track snaps to 0 dB.
        assert_eq!(slider_value_at(EqSlider::Preamp, track + 31.0, half), 33);
        // 40% from the top is level 60, which is not in the dead zone: 1 + round(0.6 * 63) = 39.
        assert_eq!(slider_value_at(EqSlider::Band(0), track + half + 0.4 * 51.0, half), 39);
    }

    #[test]
    fn thumb_offset_and_frame_follow_the_level() {
        assert_eq!(thumb_offset(100), 0);
        assert_eq!(thumb_offset(50), 25);
        assert_eq!(thumb_offset(0), 51);
        assert_eq!(slider_frame(0), 0);
        assert_eq!(slider_frame(50), 14);
        assert_eq!(slider_frame(100), 27);
    }

    #[test]
    fn press_on_thumb_keeps_the_grab_point_else_centres_it() {
        let settings = EqSettings::default();
        // Flat: level 50, thumb offset 25, thumb at y = 38 + 25 = 63, x = 22..33.
        let on_thumb = press_grab(EqSlider::Preamp, &settings, 25.0, 65.0);
        assert_eq!(on_thumb, 2.0);
        let on_track = press_grab(EqSlider::Preamp, &settings, 25.0, 45.0);
        assert_eq!(on_track, 5.5);
    }

    #[test]
    fn flat_graph_is_a_straight_centre_line() {
        let runs = graph_runs(&[51; 10]);
        assert_eq!(runs.len(), 109);
        for (x, run) in runs.iter().enumerate() {
            assert_eq!(*run, GraphRun { x: x as i32, y_top: 9, height: 1 });
        }
        assert_eq!(preamp_row(51), 9);
    }

    #[test]
    fn graph_passes_through_each_band_value() {
        let levels = [70, 60, 30, 20, 40, 50, 50, 50, 50, 50];
        let runs = graph_runs(&levels);
        for (i, &level) in levels.iter().enumerate() {
            let want = js_round((100.0 - f64::from(level)) / 100.0 * 18.0) as i32;
            let run = runs[i * 12];
            assert!(
                run.y_top <= want && want < run.y_top + run.height,
                "band {i}: row {want} not covered by {run:?}"
            );
        }
    }

    #[test]
    fn background_and_titlebar_follow_focus() {
        let skin = synthetic();
        let focused = render(&skin, &flat_state());
        assert_copied(&focused, &skin, (2, 30), (2, 30), (1, 1));
        assert_eq!(px(&focused, 100, 5), sheet_px(&skin, 100, 139));

        let unfocused = render(&skin, &EqWindowState { focused: false, ..flat_state() });
        assert_eq!(px(&unfocused, 100, 5), sheet_px(&skin, 100, 154));
    }

    #[test]
    fn close_button_shows_pressed_sprite_while_held() {
        let skin = synthetic();
        let c = render(&skin, &flat_state());
        assert_copied(&c, &skin, (0, 116), (CLOSE_BTN.x, CLOSE_BTN.y), (9, 9));
        let pressed = render(&skin, &EqWindowState { pressed: Some(EqHit::Close), ..flat_state() });
        assert_copied(&pressed, &skin, (0, 125), (CLOSE_BTN.x, CLOSE_BTN.y), (9, 9));
    }

    #[test]
    fn on_and_auto_pick_their_lit_and_pressed_sprites() {
        let skin = synthetic();
        let lit = render(&skin, &flat_state());
        assert_copied(&lit, &skin, (69, 119), (ON_BTN.x, ON_BTN.y), (26, 12));
        assert_copied(&lit, &skin, (36, 119), (AUTO_BTN.x, AUTO_BTN.y), (32, 12));

        let off = EqWindowState {
            settings: EqSettings { on: false, auto: true, ..EqSettings::default() },
            pressed: Some(EqHit::On),
            ..flat_state()
        };
        let c = render(&skin, &off);
        // Off and pressed is EQ_ON_BUTTON_DEPRESSED at (128, 119).
        assert_copied(&c, &skin, (128, 119), (ON_BTN.x, ON_BTN.y), (26, 12));
        // Auto on, not pressed is EQ_AUTO_BUTTON_SELECTED at (95, 119).
        assert_copied(&c, &skin, (95, 119), (AUTO_BTN.x, AUTO_BTN.y), (32, 12));
    }

    #[test]
    fn presets_button_normal_and_pressed() {
        let skin = synthetic();
        let c = render(&skin, &flat_state());
        assert_copied(&c, &skin, (224, 164), (PRESETS_BTN.x, PRESETS_BTN.y), (44, 12));
        let pressed = render(&skin, &EqWindowState { pressed: Some(EqHit::Presets), ..flat_state() });
        assert_copied(&pressed, &skin, (224, 176), (PRESETS_BTN.x, PRESETS_BTN.y), (44, 12));
    }

    #[test]
    fn flat_sliders_show_the_centre_frame_and_thumb() {
        let skin = synthetic();
        let c = render(&skin, &flat_state());
        // Frame 14 (x 0, row 1) is at sprite (13, 229). Rows 38 and 100 stay clear of the thumb,
        // which is at y 63..74 for the preamp (level 50, offset 25).
        assert_copied(&c, &skin, (13, 229), (21, 38), (14, 1));
        assert_copied(&c, &skin, (13, 229 + 62), (21, 100), (14, 1));
        // Preamp thumb at (22, 63).
        assert_copied(&c, &skin, (0, 164), (22, 63), (11, 11));
        // Band 4 (1 kHz) at x = 78 + 4 * 18 = 150.
        assert_copied(&c, &skin, (0, 164), (151, 63), (11, 11));
    }

    #[test]
    fn dragged_band_thumb_is_selected_and_at_the_bottom() {
        let skin = synthetic();
        let st = EqWindowState {
            settings: EqSettings { bands: [1; 10], ..EqSettings::default() },
            dragging: Some(EqSlider::Band(0)),
            ..flat_state()
        };
        let c = render(&skin, &st);
        // Level 0: thumb offset 51, so the thumb sits at (79, 89).
        assert_copied(&c, &skin, (0, 176), (79, 38 + 51), (11, 11));
    }

    #[test]
    fn flat_graph_draws_the_colour_strip_on_row_nine() {
        let skin = synthetic();
        let c = render(&skin, &flat_state());
        // The strip is at (115, 294); row 9 of the graph is window y 17 + 9 = 26.
        for x in 0..=108 {
            assert_eq!(
                decode_coord(px(&c, GRAPH.x + 2 + x, GRAPH.y + 9)),
                (115, 294 + 9),
                "column {x}"
            );
        }
        // Left padding (x 0..2) shows the preamp line, not the response line.
        assert_eq!(decode_coord(px(&c, GRAPH.x, GRAPH.y + 9)), (0, 314));
        // Above the line is the graph background.
        assert_eq!(decode_coord(px(&c, GRAPH.x + 10, GRAPH.y + 2)), (10, 296));
    }

    #[test]
    fn graph_colour_follows_the_row() {
        let skin = synthetic();
        let st = EqWindowState {
            settings: EqSettings { bands: [64; 10], ..EqSettings::default() },
            ..flat_state()
        };
        let c = render(&skin, &st);
        // Level 100 everywhere: the line sits on row 0, so column 50 uses strip row 0.
        assert_eq!(decode_coord(px(&c, GRAPH.x + 2 + 50, GRAPH.y)), (115, 294));
    }

    #[test]
    fn render_is_deterministic_and_matches_the_canvas_size() {
        let skin = synthetic();
        let st = flat_state();
        let a = render(&skin, &st);
        assert_eq!((a.w, a.h), (WIDTH, HEIGHT));
        assert_eq!(a, render(&skin, &st));
    }

    #[test]
    fn undersized_skin_renders_without_panicking() {
        let tiny = Rgba {
            w: 10,
            h: 10,
            px: vec![encode(1, 1); 100],
        };
        let bytes = testkit::zip_bytes(&[("MAIN.BMP", testkit::bmp_bytes(&tiny))]);
        let skin = Skin::load(&bytes).expect("only MAIN is required");
        let _ = render(&skin, &flat_state());
    }
}

#[cfg(test)]
mod shade_tests {
    use super::*;
    use tubeamp_skin::testkit;

    fn shade_state() -> EqWindowState {
        EqWindowState {
            shade: true,
            focused: true,
            volume: 0,
            balance: 0,
            ..EqWindowState::default()
        }
    }

    fn render(skin: &Skin, st: &EqWindowState) -> Canvas {
        let mut canvas = Canvas::new(WIDTH, SHADE_HEIGHT);
        render_eq(skin, st, &mut canvas);
        canvas
    }

    fn px(c: &Canvas, x: i32, y: i32) -> u32 {
        c.px[(y as u32 * c.w + x as u32) as usize]
    }

    fn ex_px(skin: &Skin, x: u32, y: u32) -> u32 {
        let img = skin.sheet(Sheet::EqEx).expect("eq_ex");
        img.px[(y * img.w + x) as usize]
    }

    /// Asserts that canvas `dst` shows EQ_EX `src` over `size`.
    fn assert_ex(c: &Canvas, skin: &Skin, src: (u32, u32), dst: (i32, i32), size: (u32, u32)) {
        for j in 0..size.1 {
            for i in 0..size.0 {
                assert_eq!(
                    px(c, dst.0 + i as i32, dst.1 + j as i32),
                    ex_px(skin, src.0 + i, src.1 + j),
                    "canvas ({},{}) should show EQ_EX ({},{})",
                    dst.0 + i as i32,
                    dst.1 + j as i32,
                    src.0 + i,
                    src.1 + j
                );
            }
        }
    }

    #[test]
    fn shade_strip_follows_focus() {
        let skin = testkit::synthetic_skin();
        // The thumbs (rows 4..11) and the buttons (x 254..) cover parts of the strip, so the
        // bands above and below them are compared.
        let c = render(&skin, &shade_state());
        assert_ex(&c, &skin, (0, 0), (0, 0), (254, 3));
        assert_ex(&c, &skin, (0, 11), (0, 11), (254, 3));
        let unfocused = render(
            &skin,
            &EqWindowState {
                focused: false,
                ..shade_state()
            },
        );
        assert_ex(&unfocused, &skin, (0, 15 + 11), (0, 11), (254, 3));
    }

    #[test]
    fn shade_close_and_restore_buttons_use_the_shade_sprites() {
        let skin = testkit::synthetic_skin();
        let c = render(&skin, &shade_state());
        assert_ex(&c, &skin, (11, 38), (264, 3), (9, 9));
        let pressed = render(
            &skin,
            &EqWindowState {
                pressed: Some(EqHit::Close),
                ..shade_state()
            },
        );
        assert_ex(&pressed, &skin, (11, 47), (264, 3), (9, 9));
        // The shade button's normal sprite comes from EQMAIN, the fallback.
        let shade_btn = skin
            .sprite(Sheet::Eqmain, "EQ_MAXIMIZE_BUTTON_ACTIVE_FALLBACK")
            .expect("fallback sprite");
        assert_eq!(px(&c, 254, 3), shade_btn.pixel(0, 0));
    }

    #[test]
    fn shade_thumbs_follow_the_volume_and_balance() {
        let skin = testkit::synthetic_skin();
        // Volume 100 is the right thumb at the end of the track; balance 0 is the middle one.
        let st = EqWindowState {
            volume: 100,
            balance: 0,
            ..shade_state()
        };
        let c = render(&skin, &st);
        assert_ex(&c, &skin, (7, 30), (61 + 94, 4), (3, 7));
        assert_ex(&c, &skin, (14, 30), (164 + 20, 4), (3, 7));
        // Volume 0 and balance -100 are the left thumbs.
        let left = render(
            &skin,
            &EqWindowState {
                volume: 0,
                balance: -100,
                ..shade_state()
            },
        );
        assert_ex(&left, &skin, (1, 30), (61, 4), (3, 7));
        assert_ex(&left, &skin, (11, 30), (164, 4), (3, 7));
    }

    #[test]
    fn shade_hit_test_finds_the_controls_and_the_strip() {
        assert_eq!(hit_test_eq_shade(266, 5), Some(EqHit::Close));
        assert_eq!(hit_test_eq_shade(256, 5), Some(EqHit::Shade));
        assert_eq!(hit_test_eq_shade(100, 6), Some(EqHit::ShadeVolume));
        assert_eq!(hit_test_eq_shade(180, 6), Some(EqHit::ShadeBalance));
        assert_eq!(hit_test_eq_shade(20, 7), Some(EqHit::TitleBar));
        assert_eq!(hit_test_eq_shade(20, 14), None);
        assert_eq!(hit_test_eq_shade(275, 7), None);
    }

    #[test]
    fn full_window_has_a_shade_button_at_the_same_place() {
        assert_eq!(hit_test_eq(256, 5), Some(EqHit::Shade));
        assert_eq!(hit_test_eq(266, 5), Some(EqHit::Close));
    }

    #[test]
    fn shade_volume_and_balance_map_pointer_x_to_their_values() {
        assert_eq!(shade_volume_from_x(61.0 + 1.5), 0);
        assert_eq!(shade_volume_from_x(61.0 + 1.5 + 94.0), 100);
        assert_eq!(shade_volume_from_x(61.0 + 1.5 + 47.0), 50);
        assert_eq!(shade_volume_from_x(-10.0), 0);
        assert_eq!(shade_balance_from_x(164.0 + 1.5), -100);
        assert_eq!(shade_balance_from_x(164.0 + 1.5 + 20.0), 0);
        assert_eq!(shade_balance_from_x(164.0 + 1.5 + 40.0), 100);
    }

    #[test]
    fn window_height_is_the_shade_height_or_the_full_height() {
        assert_eq!(window_height(true), 14);
        assert_eq!(window_height(false), 116);
    }
}
