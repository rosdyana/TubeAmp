//! Main window (275×116): rendering and hit testing.
//!
//! Element rectangles come from Webamp's `css/main-window.css`; the state →
//! sprite rules come from Webamp's `js/skinSelectors.ts` and
//! `js/components/MainWindow/*.tsx` (MIT, © Jordan Eldredge).

use crate::canvas::{Canvas, Rect};
use crate::sliders::{BALANCE_TRAVEL, POSITION_TRAVEL, VOLUME_TRAVEL};
use crate::text::{draw_text, marquee_frame};
use crate::vis::{VisMode, VIS_RECT};
use tubeamp_skin::{Sheet, Skin};

/// Main window width in pixels (1×).
pub const WIDTH: u32 = 275;
/// Main window height in pixels (1×).
pub const HEIGHT: u32 = 116;

/// Playback status, which selects the play/pause/stop indicator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Status {
    Play,
    Pause,
    #[default]
    Stop,
}

/// Everything the main window shows.
#[derive(Debug, Clone, PartialEq)]
pub struct MainState {
    pub title: String,
    pub time_secs: u32,
    pub duration_secs: u32,
    /// Show the time left (`duration - time`) instead of the elapsed time.
    pub remaining_mode: bool,
    pub status: Status,
    pub kbps: Option<u32>,
    pub khz: Option<u32>,
    pub stereo: bool,
    /// 0..=100.
    pub volume: u8,
    /// -100..=100.
    pub balance: i8,
    /// 0.0..=1.0.
    pub position: f32,
    pub shuffle: bool,
    pub repeat: bool,
    pub eq_open: bool,
    pub pl_open: bool,
    pub focused: bool,
    /// Marquee scroll position in pixels.
    pub marquee_offset: u32,
    /// The control currently held down, drawn with its pressed sprite.
    pub pressed: Option<MainButton>,
    /// Whether the window is in double-size mode (lights the D button).
    pub doubled: bool,
    /// What the vis area shows.
    pub vis_mode: VisMode,
}

impl Default for MainState {
    fn default() -> Self {
        MainState {
            title: String::new(),
            time_secs: 0,
            duration_secs: 0,
            remaining_mode: false,
            status: Status::Stop,
            kbps: None,
            khz: None,
            stereo: true,
            volume: 0,
            balance: 0,
            position: 0.0,
            shuffle: false,
            repeat: false,
            eq_open: false,
            pl_open: false,
            focused: false,
            marquee_offset: 0,
            pressed: None,
            doubled: false,
            vis_mode: VisMode::default(),
        }
    }
}

/// Controls that can be held down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainButton {
    Option,
    Minimize,
    Shade,
    Close,
    ClutterO,
    ClutterA,
    ClutterI,
    ClutterD,
    ClutterV,
    Previous,
    Play,
    Pause,
    Stop,
    Next,
    Eject,
    Shuffle,
    Repeat,
    EqToggle,
    PlToggle,
    Position,
    Volume,
    Balance,
}

/// What a point on the main window belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MainHit {
    /// Draggable title bar (no button under the point).
    TitleBar,
    Button(MainButton),
    Time,
    Marquee,
    Kbps,
    Khz,
    MonoStereo,
    /// The visualiser area (clicking it cycles the mode).
    Vis,
}

// Element rectangles (window pixels), from css/main-window.css.
const TITLE_BAR: Rect = Rect::new(0, 0, 275, 14);
const OPTION_BTN: Rect = Rect::new(6, 3, 9, 9);
const MINIMIZE_BTN: Rect = Rect::new(244, 3, 9, 9);
const SHADE_BTN: Rect = Rect::new(254, 3, 9, 9);
const CLOSE_BTN: Rect = Rect::new(264, 3, 9, 9);
const CLUTTER_BAR: Rect = Rect::new(10, 22, 8, 43);
/// The clutterbar's O button (options). Its menu opens from this rectangle.
pub const CLUTTER_O: Rect = Rect::new(10, 25, 8, 8);
const CLUTTER_A: Rect = Rect::new(10, 33, 8, 7);
const CLUTTER_I: Rect = Rect::new(10, 40, 8, 7);
const CLUTTER_D: Rect = Rect::new(10, 47, 8, 8);
const CLUTTER_V: Rect = Rect::new(10, 55, 8, 7);
const TIME: Rect = Rect::new(39, 26, 59, 13);
/// Marquee clip area: the element's padding box, which is what `overflow: hidden` clips to.
const MARQUEE: Rect = Rect::new(111, 24, 154, 12);
const KBPS: Rect = Rect::new(111, 43, 15, 6);
const KHZ: Rect = Rect::new(156, 43, 10, 6);
const MONO_STEREO: Rect = Rect::new(212, 41, 57, 12);
pub const VOLUME: Rect = Rect::new(107, 57, 68, 13);
pub const BALANCE: Rect = Rect::new(177, 57, 38, 13);
const EQ_TOGGLE: Rect = Rect::new(219, 58, 23, 12);
const PL_TOGGLE: Rect = Rect::new(242, 58, 23, 12);
pub const POSITION: Rect = Rect::new(16, 72, 248, 10);
const PREVIOUS: Rect = Rect::new(16, 88, 23, 18);
const PLAY: Rect = Rect::new(39, 88, 23, 18);
const PAUSE: Rect = Rect::new(62, 88, 23, 18);
const STOP: Rect = Rect::new(85, 88, 23, 18);
const NEXT: Rect = Rect::new(108, 88, 22, 18);
const EJECT: Rect = Rect::new(136, 89, 22, 16);
const SHUFFLE: Rect = Rect::new(164, 89, 47, 15);
const REPEAT: Rect = Rect::new(210, 89, 28, 15);

/// Hit-test order: earlier entries win. The title bar is checked last.
const HIT_TABLE: &[(Rect, MainHit)] = &[
    (OPTION_BTN, MainHit::Button(MainButton::Option)),
    (MINIMIZE_BTN, MainHit::Button(MainButton::Minimize)),
    (SHADE_BTN, MainHit::Button(MainButton::Shade)),
    (CLOSE_BTN, MainHit::Button(MainButton::Close)),
    (CLUTTER_O, MainHit::Button(MainButton::ClutterO)),
    (CLUTTER_A, MainHit::Button(MainButton::ClutterA)),
    (CLUTTER_I, MainHit::Button(MainButton::ClutterI)),
    (CLUTTER_D, MainHit::Button(MainButton::ClutterD)),
    (CLUTTER_V, MainHit::Button(MainButton::ClutterV)),
    (TIME, MainHit::Time),
    (MARQUEE, MainHit::Marquee),
    (VIS_RECT, MainHit::Vis),
    (KBPS, MainHit::Kbps),
    (KHZ, MainHit::Khz),
    (MONO_STEREO, MainHit::MonoStereo),
    (VOLUME, MainHit::Button(MainButton::Volume)),
    (BALANCE, MainHit::Button(MainButton::Balance)),
    (EQ_TOGGLE, MainHit::Button(MainButton::EqToggle)),
    (PL_TOGGLE, MainHit::Button(MainButton::PlToggle)),
    (POSITION, MainHit::Button(MainButton::Position)),
    (PREVIOUS, MainHit::Button(MainButton::Previous)),
    (PLAY, MainHit::Button(MainButton::Play)),
    (PAUSE, MainHit::Button(MainButton::Pause)),
    (STOP, MainHit::Button(MainButton::Stop)),
    (NEXT, MainHit::Button(MainButton::Next)),
    (EJECT, MainHit::Button(MainButton::Eject)),
    (SHUFFLE, MainHit::Button(MainButton::Shuffle)),
    (REPEAT, MainHit::Button(MainButton::Repeat)),
];

/// Absolute left edges of the four digits (minutes tens/units, seconds tens/units).
const DIGIT_X: [i32; 4] = [48, 60, 78, 90];
const DIGIT_Y: i32 = 26;
/// Baseline of the marquee/bitrate text (the element's content box top).
const MARQUEE_TEXT_Y: i32 = 27;

const DIGIT_NAMES: [&str; 10] = [
    "DIGIT_0_EX",
    "DIGIT_1_EX",
    "DIGIT_2_EX",
    "DIGIT_3_EX",
    "DIGIT_4_EX",
    "DIGIT_5_EX",
    "DIGIT_6_EX",
    "DIGIT_7_EX",
    "DIGIT_8_EX",
    "DIGIT_9_EX",
];

/// `[normal, selected, depressed, depressed+selected]`
const SHUFFLE_NAMES: [&str; 4] = [
    "MAIN_SHUFFLE_BUTTON",
    "MAIN_SHUFFLE_BUTTON_SELECTED",
    "MAIN_SHUFFLE_BUTTON_DEPRESSED",
    "MAIN_SHUFFLE_BUTTON_SELECTED_DEPRESSED",
];
const REPEAT_NAMES: [&str; 4] = [
    "MAIN_REPEAT_BUTTON",
    "MAIN_REPEAT_BUTTON_SELECTED",
    "MAIN_REPEAT_BUTTON_DEPRESSED",
    "MAIN_REPEAT_BUTTON_SELECTED_DEPRESSED",
];
const EQ_NAMES: [&str; 4] = [
    "MAIN_EQ_BUTTON",
    "MAIN_EQ_BUTTON_SELECTED",
    "MAIN_EQ_BUTTON_DEPRESSED",
    "MAIN_EQ_BUTTON_DEPRESSED_SELECTED",
];
const PL_NAMES: [&str; 4] = [
    "MAIN_PLAYLIST_BUTTON",
    "MAIN_PLAYLIST_BUTTON_SELECTED",
    "MAIN_PLAYLIST_BUTTON_DEPRESSED",
    "MAIN_PLAYLIST_BUTTON_DEPRESSED_SELECTED",
];

/// Minutes and seconds as four digits `[m tens, m units, s tens, s units]`.
/// Minutes wrap at 100 so the display stays four digits.
pub fn time_digits(secs: u32) -> [u32; 4] {
    let m = (secs / 60) % 100;
    let s = secs % 60;
    [m / 10, m % 10, s / 10, s % 10]
}

/// The number shown on the time display: elapsed, or time left in remaining mode.
pub fn displayed_secs(st: &MainState) -> u32 {
    if st.remaining_mode {
        st.duration_secs.saturating_sub(st.time_secs)
    } else {
        st.time_secs
    }
}

/// Which hit target (if any) is at window point `(x, y)`.
pub fn hit_test_main(x: i32, y: i32) -> Option<MainHit> {
    HIT_TABLE
        .iter()
        .find(|(rect, _)| rect.contains(x, y))
        .map(|(_, hit)| *hit)
        .or_else(|| TITLE_BAR.contains(x, y).then_some(MainHit::TitleBar))
}

fn is_down(st: &MainState, button: MainButton) -> bool {
    st.pressed == Some(button)
}

/// Index into `[normal, selected, depressed, depressed+selected]`.
fn state_index(selected: bool, pressed: bool) -> usize {
    usize::from(selected) + 2 * usize::from(pressed)
}

fn up_down(down: bool, up: &'static str, pressed: &'static str) -> &'static str {
    if down {
        pressed
    } else {
        up
    }
}

fn draw(skin: &Skin, canvas: &mut Canvas, sheet: Sheet, name: &str, x: i32, y: i32) {
    if let Some(view) = skin.sprite(sheet, name) {
        canvas.blit(&view, x, y);
    }
}

fn draw_part(
    skin: &Skin,
    canvas: &mut Canvas,
    sheet: Sheet,
    name: &str,
    src: (u32, u32, u32, u32),
    x: i32,
    y: i32,
) {
    if let Some(view) = skin.sprite(sheet, name) {
        canvas.blit_region(&view, src, x, y);
    }
}

/// Draws the whole main window in Webamp's layout, from the background up.
///
/// The visualiser area (24, 43) is left as background here. [`crate::render_vis`] draws it on top.
pub fn render_main(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    draw(skin, canvas, Sheet::Main, "MAIN_WINDOW_BACKGROUND", 0, 0);
    draw_titlebar(skin, st, canvas);
    draw_clutterbar(skin, st, canvas);
    draw_status(skin, st, canvas);
    draw_time(skin, st, canvas);
    draw_marquee(skin, st, canvas);
    draw_bitrate(skin, st, canvas);
    draw_mono_stereo(skin, st, canvas);
    draw_volume(skin, st, canvas);
    draw_balance(skin, st, canvas);
    draw_toggles(skin, st, canvas);
    draw_position(skin, st, canvas);
    draw_transport(skin, st, canvas);
    draw_shuffle_repeat(skin, st, canvas);
}

fn draw_titlebar(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    let bar = if st.focused {
        "MAIN_TITLE_BAR_SELECTED"
    } else {
        "MAIN_TITLE_BAR"
    };
    draw(skin, canvas, Sheet::Titlebar, bar, 0, 0);

    let (x, y) = (OPTION_BTN.x, OPTION_BTN.y);
    let name = up_down(
        is_down(st, MainButton::Option),
        "MAIN_OPTIONS_BUTTON",
        "MAIN_OPTIONS_BUTTON_DEPRESSED",
    );
    draw(skin, canvas, Sheet::Titlebar, name, x, y);

    let name = up_down(
        is_down(st, MainButton::Minimize),
        "MAIN_MINIMIZE_BUTTON",
        "MAIN_MINIMIZE_BUTTON_DEPRESSED",
    );
    draw(
        skin,
        canvas,
        Sheet::Titlebar,
        name,
        MINIMIZE_BTN.x,
        MINIMIZE_BTN.y,
    );

    let name = up_down(
        is_down(st, MainButton::Shade),
        "MAIN_SHADE_BUTTON",
        "MAIN_SHADE_BUTTON_DEPRESSED",
    );
    draw(
        skin,
        canvas,
        Sheet::Titlebar,
        name,
        SHADE_BTN.x,
        SHADE_BTN.y,
    );

    let name = up_down(
        is_down(st, MainButton::Close),
        "MAIN_CLOSE_BUTTON",
        "MAIN_CLOSE_BUTTON_DEPRESSED",
    );
    draw(
        skin,
        canvas,
        Sheet::Titlebar,
        name,
        CLOSE_BTN.x,
        CLOSE_BTN.y,
    );
}

fn draw_clutterbar(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    draw(
        skin,
        canvas,
        Sheet::Titlebar,
        "MAIN_CLUTTER_BAR_BACKGROUND",
        CLUTTER_BAR.x,
        CLUTTER_BAR.y,
    );
    let letters = [
        (
            MainButton::ClutterO,
            CLUTTER_O,
            "MAIN_CLUTTER_BAR_BUTTON_O_SELECTED",
        ),
        (
            MainButton::ClutterA,
            CLUTTER_A,
            "MAIN_CLUTTER_BAR_BUTTON_A_SELECTED",
        ),
        (
            MainButton::ClutterI,
            CLUTTER_I,
            "MAIN_CLUTTER_BAR_BUTTON_I_SELECTED",
        ),
        (
            MainButton::ClutterD,
            CLUTTER_D,
            "MAIN_CLUTTER_BAR_BUTTON_D_SELECTED",
        ),
        (
            MainButton::ClutterV,
            CLUTTER_V,
            "MAIN_CLUTTER_BAR_BUTTON_V_SELECTED",
        ),
    ];
    for (button, rect, name) in letters {
        if is_down(st, button) {
            draw(skin, canvas, Sheet::Titlebar, name, rect.x, rect.y);
        }
    }
    // D stays lit while the window is doubled.
    if st.doubled {
        draw(
            skin,
            canvas,
            Sheet::Titlebar,
            "MAIN_CLUTTER_BAR_BUTTON_D_SELECTED",
            CLUTTER_D.x,
            CLUTTER_D.y,
        );
    }
}

fn draw_status(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    let name = match st.status {
        Status::Play => "MAIN_PLAYING_INDICATOR",
        Status::Pause => "MAIN_PAUSED_INDICATOR",
        Status::Stop => "MAIN_STOPPED_INDICATOR",
    };
    draw(skin, canvas, Sheet::Playpaus, name, 26, 28);
    // The "working" slot is only visible while playing; no working state yet.
    if st.status == Status::Play {
        draw_part(
            skin,
            canvas,
            Sheet::Playpaus,
            "MAIN_NOT_WORKING_INDICATOR",
            (0, 0, 3, 9),
            24,
            28,
        );
    }
}

fn draw_time(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    // Webamp hides the time display entirely while stopped.
    if st.status == Status::Stop {
        return;
    }
    let digits = time_digits(displayed_secs(st));
    for (i, &d) in digits.iter().enumerate() {
        draw(
            skin,
            canvas,
            Sheet::NumsEx,
            DIGIT_NAMES[d as usize],
            DIGIT_X[i],
            DIGIT_Y,
        );
    }
    if st.remaining_mode {
        // NUMS_EX.BMP has a 9×13 minus sign; NUMBERS.BMP has a 5×1 one.
        if skin.has_nums_ex() {
            draw(skin, canvas, Sheet::NumsEx, "MINUS_SIGN_EX", 38, DIGIT_Y);
        } else {
            draw(skin, canvas, Sheet::Numbers, "MINUS_SIGN", 38, 32);
        }
    }
}

fn draw_marquee(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    let frame = marquee_frame(&st.title, MARQUEE.w as u32, st.marquee_offset);
    let x = MARQUEE.x - (st.marquee_offset % 5) as i32;
    draw_text(canvas, skin, &frame, x, MARQUEE_TEXT_Y, MARQUEE);
}

fn draw_bitrate(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    if st.status == Status::Stop {
        return;
    }
    if let Some(kbps) = st.kbps {
        draw_text(canvas, skin, &kbps.to_string(), KBPS.x, KBPS.y, KBPS);
    }
    if let Some(khz) = st.khz {
        draw_text(canvas, skin, &khz.to_string(), KHZ.x, KHZ.y, KHZ);
    }
}

fn draw_mono_stereo(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    // While stopped Webamp draws both labels unselected.
    let playing = st.status != Status::Stop;
    let mono = if playing && !st.stereo {
        "MAIN_MONO_SELECTED"
    } else {
        "MAIN_MONO"
    };
    let stereo = if playing && st.stereo {
        "MAIN_STEREO_SELECTED"
    } else {
        "MAIN_STEREO"
    };
    draw(
        skin,
        canvas,
        Sheet::Monoster,
        mono,
        MONO_STEREO.x,
        MONO_STEREO.y,
    );
    draw(
        skin,
        canvas,
        Sheet::Monoster,
        stereo,
        MONO_STEREO.x + 27,
        MONO_STEREO.y,
    );
}

fn draw_volume(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    let v = u32::from(st.volume.min(100));
    // Webamp: sprite = round(volume/100 * 28), frames are 15 px apart, frame 0 sits at y = 0.
    let frame = (v * 28 + 50) / 100;
    let y = if frame == 0 { 0 } else { (frame - 1) * 15 };
    draw_part(
        skin,
        canvas,
        Sheet::Volume,
        "MAIN_VOLUME_BACKGROUND",
        (0, y, 68, 13),
        VOLUME.x,
        VOLUME.y,
    );

    let thumb_x = VOLUME.x + ((v * VOLUME_TRAVEL + 50) / 100) as i32;
    let name = up_down(
        is_down(st, MainButton::Volume),
        "MAIN_VOLUME_THUMB",
        "MAIN_VOLUME_THUMB_SELECTED",
    );
    draw(skin, canvas, Sheet::Volume, name, thumb_x, VOLUME.y + 1);
}

fn draw_balance(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    let b = i32::from(st.balance.clamp(-100, 100));
    // Webamp: frame = floor(|balance|/100 * 27), frames are 15 px apart.
    let frame = (b.unsigned_abs() * 27) / 100;
    draw_part(
        skin,
        canvas,
        Sheet::Balance,
        "MAIN_BALANCE_BACKGROUND",
        (0, frame * 15, 38, 13),
        BALANCE.x,
        BALANCE.y,
    );

    let travel = BALANCE_TRAVEL as i32;
    let thumb_x = BALANCE.x + (((b + 100) * travel + 100) / 200);
    let name = up_down(
        is_down(st, MainButton::Balance),
        "MAIN_BALANCE_THUMB",
        "MAIN_BALANCE_THUMB_ACTIVE",
    );
    draw(skin, canvas, Sheet::Balance, name, thumb_x, BALANCE.y + 1);
}

fn draw_toggles(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    let eq = state_index(st.eq_open, is_down(st, MainButton::EqToggle));
    draw(
        skin,
        canvas,
        Sheet::Shufrep,
        EQ_NAMES[eq],
        EQ_TOGGLE.x,
        EQ_TOGGLE.y,
    );
    let pl = state_index(st.pl_open, is_down(st, MainButton::PlToggle));
    draw(
        skin,
        canvas,
        Sheet::Shufrep,
        PL_NAMES[pl],
        PL_TOGGLE.x,
        PL_TOGGLE.y,
    );
}

fn draw_position(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    draw(
        skin,
        canvas,
        Sheet::Posbar,
        "MAIN_POSITION_SLIDER_BACKGROUND",
        POSITION.x,
        POSITION.y,
    );
    // The thumb is hidden while stopped and when there is no duration.
    if st.status != Status::Stop && st.duration_secs > 0 {
        let p = st.position.clamp(0.0, 1.0);
        let offset = (p * POSITION_TRAVEL as f32).round() as i32;
        let name = up_down(
            is_down(st, MainButton::Position),
            "MAIN_POSITION_SLIDER_THUMB",
            "MAIN_POSITION_SLIDER_THUMB_SELECTED",
        );
        draw(
            skin,
            canvas,
            Sheet::Posbar,
            name,
            POSITION.x + offset,
            POSITION.y,
        );
    }
}

fn draw_transport(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    let previous = up_down(
        is_down(st, MainButton::Previous),
        "MAIN_PREVIOUS_BUTTON",
        "MAIN_PREVIOUS_BUTTON_ACTIVE",
    );
    draw(
        skin,
        canvas,
        Sheet::Cbuttons,
        previous,
        PREVIOUS.x,
        PREVIOUS.y,
    );
    let play = up_down(
        is_down(st, MainButton::Play),
        "MAIN_PLAY_BUTTON",
        "MAIN_PLAY_BUTTON_ACTIVE",
    );
    draw(skin, canvas, Sheet::Cbuttons, play, PLAY.x, PLAY.y);
    let pause = up_down(
        is_down(st, MainButton::Pause),
        "MAIN_PAUSE_BUTTON",
        "MAIN_PAUSE_BUTTON_ACTIVE",
    );
    draw(skin, canvas, Sheet::Cbuttons, pause, PAUSE.x, PAUSE.y);
    let stop = up_down(
        is_down(st, MainButton::Stop),
        "MAIN_STOP_BUTTON",
        "MAIN_STOP_BUTTON_ACTIVE",
    );
    draw(skin, canvas, Sheet::Cbuttons, stop, STOP.x, STOP.y);
    // The NEXT element is 22 px wide even though its sprite is 23 px.
    let next = up_down(
        is_down(st, MainButton::Next),
        "MAIN_NEXT_BUTTON",
        "MAIN_NEXT_BUTTON_ACTIVE",
    );
    draw_part(
        skin,
        canvas,
        Sheet::Cbuttons,
        next,
        (0, 0, 22, 18),
        NEXT.x,
        NEXT.y,
    );
    let eject = up_down(
        is_down(st, MainButton::Eject),
        "MAIN_EJECT_BUTTON",
        "MAIN_EJECT_BUTTON_ACTIVE",
    );
    draw(skin, canvas, Sheet::Cbuttons, eject, EJECT.x, EJECT.y);
}

fn draw_shuffle_repeat(skin: &Skin, st: &MainState, canvas: &mut Canvas) {
    let shuffle = state_index(st.shuffle, is_down(st, MainButton::Shuffle));
    draw(
        skin,
        canvas,
        Sheet::Shufrep,
        SHUFFLE_NAMES[shuffle],
        SHUFFLE.x,
        SHUFFLE.y,
    );
    let repeat = state_index(st.repeat, is_down(st, MainButton::Repeat));
    draw(
        skin,
        canvas,
        Sheet::Shufrep,
        REPEAT_NAMES[repeat],
        REPEAT.x,
        REPEAT.y,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::text::marquee_period_px;
    use tubeamp_skin::testkit::{self, decode_coord};

    fn base_state() -> MainState {
        MainState {
            title: "Song".to_string(),
            time_secs: 83,
            duration_secs: 245,
            status: Status::Play,
            kbps: Some(128),
            khz: Some(44),
            stereo: true,
            volume: 75,
            balance: 0,
            position: 83.0 / 245.0,
            focused: true,
            ..MainState::default()
        }
    }

    /// Source pixel of a sheet, as the synthetic skin encodes it.
    fn sheet_coord(skin: &Skin, sheet: Sheet, x: u32, y: u32) -> u32 {
        let img = skin.sheet(sheet).expect("sheet");
        img.px[(y * img.w + x) as usize]
    }

    fn render(skin: &Skin, st: &MainState) -> Canvas {
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        render_main(skin, st, &mut canvas);
        canvas
    }

    fn px(c: &Canvas, x: i32, y: i32) -> u32 {
        c.px[(y as u32 * c.w + x as u32) as usize]
    }

    /// Asserts that canvas `(dx.., dy..)` equals sheet `(sx.., sy..)` over `w` × `h`.
    fn assert_copied(
        c: &Canvas,
        skin: &Skin,
        sheet: Sheet,
        src: (u32, u32),
        dst: (i32, i32),
        size: (u32, u32),
    ) {
        for j in 0..size.1 {
            for i in 0..size.0 {
                let want = sheet_coord(skin, sheet, src.0 + i, src.1 + j);
                let got = px(c, dst.0 + i as i32, dst.1 + j as i32);
                assert_eq!(
                    got,
                    want,
                    "canvas ({},{}) should show sheet {:?} ({},{})",
                    dst.0 + i as i32,
                    dst.1 + j as i32,
                    sheet,
                    src.0 + i,
                    src.1 + j
                );
            }
        }
    }

    #[test]
    fn play_button_sprite_lands_at_its_css_position() {
        let skin = testkit::synthetic_skin();
        let c = render(&skin, &base_state());
        assert_copied(&c, &skin, Sheet::Cbuttons, (23, 0), (39, 88), (23, 18));
    }

    #[test]
    fn pressed_play_button_shows_the_active_sprite() {
        let skin = testkit::synthetic_skin();
        let st = MainState {
            pressed: Some(MainButton::Play),
            ..base_state()
        };
        let c = render(&skin, &st);
        assert_copied(&c, &skin, Sheet::Cbuttons, (23, 18), (39, 88), (23, 18));
    }

    /// A skin whose NUMS_EX sheet is a flat colour, so NUMS_EX use is detectable.
    fn skin_with_flat_nums_ex(color: u32) -> Skin {
        let (w, h) = testkit::sheet_size(Sheet::NumsEx);
        let flat = tubeamp_skin::Rgba {
            w,
            h,
            px: vec![color; (w * h) as usize],
        };
        Skin::load(&testkit::synthetic_zip_with(&[], &[(Sheet::NumsEx, flat)])).expect("skin")
    }

    #[test]
    fn time_digits_render_at_their_positions_from_numbers_without_nums_ex() {
        let skin = testkit::synthetic_skin_without(&[Sheet::NumsEx]);
        let c = render(&skin, &base_state());
        // 83 s is 01:23: NUMBERS digits 0, 1, 2, 3.
        for (i, d) in [0u32, 1, 2, 3].into_iter().enumerate() {
            assert_copied(
                &c,
                &skin,
                Sheet::Numbers,
                (9 * d, 0),
                (DIGIT_X[i], DIGIT_Y),
                (9, 13),
            );
        }
    }

    #[test]
    fn time_digits_use_nums_ex_when_the_skin_has_it() {
        let color = 0xFF_12_34_56;
        let skin = skin_with_flat_nums_ex(color);
        let c = render(&skin, &base_state());
        for x in DIGIT_X {
            assert_eq!(px(&c, x, DIGIT_Y), color);
            assert_eq!(px(&c, x + 8, DIGIT_Y + 12), color);
        }
    }

    #[test]
    fn remaining_mode_shows_minus_sign_and_time_left() {
        let st = MainState {
            remaining_mode: true,
            ..base_state()
        };
        // 245 - 83 = 162 s = 02:42.
        let no_ex = testkit::synthetic_skin_without(&[Sheet::NumsEx]);
        let c = render(&no_ex, &st);
        assert_copied(
            &c,
            &no_ex,
            Sheet::Numbers,
            (18, 0),
            (DIGIT_X[1], DIGIT_Y),
            (9, 13),
        );
        assert_copied(
            &c,
            &no_ex,
            Sheet::Numbers,
            (0, 0),
            (DIGIT_X[0], DIGIT_Y),
            (9, 13),
        );
        // Without NUMS_EX the 5×1 NUMBERS minus sprite sits at (38, 32).
        assert_copied(&c, &no_ex, Sheet::Numbers, (20, 6), (38, 32), (5, 1));

        // With NUMS_EX the 9×13 minus sprite sits at (38, 26).
        let full = testkit::synthetic_skin();
        let c = render(&full, &st);
        assert_copied(&c, &full, Sheet::NumsEx, (99, 0), (38, DIGIT_Y), (9, 13));
    }

    #[test]
    fn stopped_hides_time_bitrate_and_position_thumb() {
        let skin = testkit::synthetic_skin();
        let st = MainState {
            status: Status::Stop,
            ..base_state()
        };
        let c = render(&skin, &st);
        // Time digits gone: the pixel at the first digit is the MAIN background.
        assert_eq!(
            px(&c, DIGIT_X[0], DIGIT_Y),
            sheet_coord(&skin, Sheet::Main, (DIGIT_X[0]) as u32, DIGIT_Y as u32)
        );
        // Position thumb hidden: the track shows through at the thumb's position.
        let thumb_x = POSITION.x + (83.0f32 / 245.0 * POSITION_TRAVEL as f32).round() as i32;
        let track_x = (thumb_x - POSITION.x) as u32;
        assert_eq!(
            px(&c, thumb_x, POSITION.y + 3),
            sheet_coord(&skin, Sheet::Posbar, track_x, 3)
        );
    }

    #[test]
    fn volume_frame_follows_the_value() {
        let skin = testkit::synthetic_skin();
        // 75% -> round(0.75 * 28) = 21 -> frame y = (21 - 1) * 15 = 300.
        let c = render(&skin, &base_state());
        assert_copied(
            &c,
            &skin,
            Sheet::Volume,
            (0, 300),
            (VOLUME.x, VOLUME.y),
            (10, 13),
        );

        // Zero volume is frame 0. The thumb sits at the left edge over rows 1..=11,
        // so compare the top and bottom rows of the slider only.
        let zero = MainState {
            volume: 0,
            ..base_state()
        };
        let c0 = render(&skin, &zero);
        assert_copied(
            &c0,
            &skin,
            Sheet::Volume,
            (0, 0),
            (VOLUME.x, VOLUME.y),
            (10, 1),
        );
        assert_copied(
            &c0,
            &skin,
            Sheet::Volume,
            (0, 12),
            (VOLUME.x, VOLUME.y + 12),
            (10, 1),
        );
    }

    #[test]
    fn balance_frame_follows_the_absolute_value() {
        let skin = testkit::synthetic_skin();
        // |−100| -> floor(27) = 27 -> frame y = 405.
        let left = MainState {
            balance: -100,
            ..base_state()
        };
        // The thumb is at the left edge (rows 1..=11), so check the top and bottom rows.
        let c = render(&skin, &left);
        assert_copied(
            &c,
            &skin,
            Sheet::Balance,
            (9, 405),
            (BALANCE.x, BALANCE.y),
            (10, 1),
        );
        assert_copied(
            &c,
            &skin,
            Sheet::Balance,
            (9, 417),
            (BALANCE.x, BALANCE.y + 12),
            (10, 1),
        );
        // Centre -> frame 0.
        let c0 = render(&skin, &base_state());
        assert_copied(
            &c0,
            &skin,
            Sheet::Balance,
            (9, 0),
            (BALANCE.x, BALANCE.y),
            (10, 13),
        );
    }

    #[test]
    fn titlebar_uses_focus_and_close_shows_pressed_sprite() {
        let skin = testkit::synthetic_skin();
        let focused = render(&skin, &base_state());
        assert_copied(
            &focused,
            &skin,
            Sheet::Titlebar,
            (127, 0),
            (100, 0),
            (20, 14),
        );

        let unfocused = render(
            &skin,
            &MainState {
                focused: false,
                ..base_state()
            },
        );
        assert_copied(
            &unfocused,
            &skin,
            Sheet::Titlebar,
            (127, 15),
            (100, 0),
            (20, 14),
        );

        let pressed = render(
            &skin,
            &MainState {
                pressed: Some(MainButton::Close),
                ..base_state()
            },
        );
        assert_copied(
            &pressed,
            &skin,
            Sheet::Titlebar,
            (18, 9),
            (CLOSE_BTN.x, CLOSE_BTN.y),
            (9, 9),
        );
    }

    #[test]
    fn toggles_and_shuffle_repeat_pick_their_states() {
        let skin = testkit::synthetic_skin();
        let st = MainState {
            eq_open: true,
            shuffle: true,
            repeat: false,
            pressed: Some(MainButton::Repeat),
            ..base_state()
        };
        let c = render(&skin, &st);
        // EQ selected: (0, 73) 23×12 at (219, 58).
        assert_copied(
            &c,
            &skin,
            Sheet::Shufrep,
            (0, 73),
            (EQ_TOGGLE.x, EQ_TOGGLE.y),
            (23, 12),
        );
        // PL not open: (23, 61) at (242, 58).
        assert_copied(
            &c,
            &skin,
            Sheet::Shufrep,
            (23, 61),
            (PL_TOGGLE.x, PL_TOGGLE.y),
            (23, 12),
        );
        // Shuffle on, not pressed: (28, 30) at (164, 89). Its last column is under the
        // repeat button (which starts at x=210), so only the first 46 columns are checked.
        assert_copied(
            &c,
            &skin,
            Sheet::Shufrep,
            (28, 30),
            (SHUFFLE.x, SHUFFLE.y),
            (46, 15),
        );
        // Repeat off but pressed: (0, 15) 28×15 at (210, 89).
        assert_copied(
            &c,
            &skin,
            Sheet::Shufrep,
            (0, 15),
            (REPEAT.x, REPEAT.y),
            (28, 15),
        );
    }

    #[test]
    fn mono_stereo_shows_stereo_when_playing_and_both_dim_when_stopped() {
        let skin = testkit::synthetic_skin();
        let c = render(&skin, &base_state());
        assert_copied(&c, &skin, Sheet::Monoster, (29, 12), (212, 41), (27, 12));
        assert_copied(&c, &skin, Sheet::Monoster, (0, 0), (239, 41), (29, 12));

        let stopped = render(
            &skin,
            &MainState {
                status: Status::Stop,
                ..base_state()
            },
        );
        assert_copied(
            &stopped,
            &skin,
            Sheet::Monoster,
            (0, 12),
            (239, 41),
            (29, 12),
        );
    }

    #[test]
    fn status_indicator_follows_playback_state() {
        let skin = testkit::synthetic_skin();
        let c = render(&skin, &base_state());
        // The working slot (x 24..27) overlays the first columns of the indicator.
        assert_copied(&c, &skin, Sheet::Playpaus, (3, 0), (29, 28), (6, 9));
        assert_copied(&c, &skin, Sheet::Playpaus, (36, 0), (24, 28), (3, 9));
        let paused = render(
            &skin,
            &MainState {
                status: Status::Pause,
                ..base_state()
            },
        );
        assert_copied(&paused, &skin, Sheet::Playpaus, (9, 0), (26, 28), (9, 9));
    }

    #[test]
    fn marquee_text_is_clipped_to_its_element() {
        let skin = testkit::synthetic_skin();
        let st = MainState {
            title: "x".repeat(60),
            ..base_state()
        };
        let c = render(&skin, &st);
        let background = Canvas::new(WIDTH, HEIGHT);
        let mut bg = background;
        draw(&skin, &mut bg, Sheet::Main, "MAIN_WINDOW_BACKGROUND", 0, 0);
        draw_titlebar(&skin, &st, &mut bg);
        // Right of the marquee (x >= 265) nothing from the text may appear.
        for y in 24..36 {
            assert_eq!(
                px(&c, 265, y),
                px(&bg, 265, y),
                "marquee bled at x=265, y={y}"
            );
        }
        // Left of the marquee (x < 111) is untouched too.
        for y in 24..36 {
            assert_eq!(
                px(&c, 110, y),
                px(&bg, 110, y),
                "marquee bled at x=110, y={y}"
            );
        }
    }

    #[test]
    fn position_thumb_moves_with_position() {
        let skin = testkit::synthetic_skin();
        let st = MainState {
            position: 1.0,
            ..base_state()
        };
        let c = render(&skin, &st);
        assert_copied(
            &c,
            &skin,
            Sheet::Posbar,
            (248, 0),
            (16 + POSITION_TRAVEL as i32, 72),
            (29, 10),
        );
    }

    #[test]
    fn render_is_deterministic() {
        let skin = testkit::synthetic_skin();
        let st = base_state();
        assert_eq!(render(&skin, &st), render(&skin, &st));
    }

    #[test]
    fn undersized_sheets_render_without_panicking() {
        let bytes = testkit::zip_bytes(&[("MAIN.BMP", testkit::bmp_bytes(&tiny_sheet()))]);
        let skin = Skin::load(&bytes).expect("only MAIN is required");
        let st = MainState {
            title: "tiny skin".to_string(),
            ..base_state()
        };
        let c = render(&skin, &st);
        assert_eq!(decode_coord(px(&c, 0, 0)), (0, 0));
    }

    fn tiny_sheet() -> tubeamp_skin::Rgba {
        tubeamp_skin::Rgba {
            w: 10,
            h: 10,
            px: vec![0xFF00_0000; 100],
        }
    }

    #[test]
    fn hit_test_finds_buttons_titlebar_and_sliders() {
        assert_eq!(
            hit_test_main(8, 5),
            Some(MainHit::Button(MainButton::Option))
        );
        assert_eq!(
            hit_test_main(270, 5),
            Some(MainHit::Button(MainButton::Close))
        );
        assert_eq!(
            hit_test_main(246, 5),
            Some(MainHit::Button(MainButton::Minimize))
        );
        assert_eq!(hit_test_main(150, 5), Some(MainHit::TitleBar));
        assert_eq!(
            hit_test_main(30, 95),
            Some(MainHit::Button(MainButton::Previous))
        );
        assert_eq!(
            hit_test_main(45, 95),
            Some(MainHit::Button(MainButton::Play))
        );
        assert_eq!(
            hit_test_main(140, 95),
            Some(MainHit::Button(MainButton::Eject))
        );
        assert_eq!(
            hit_test_main(200, 75),
            Some(MainHit::Button(MainButton::Position))
        );
        assert_eq!(
            hit_test_main(130, 62),
            Some(MainHit::Button(MainButton::Volume))
        );
        assert_eq!(
            hit_test_main(190, 62),
            Some(MainHit::Button(MainButton::Balance))
        );
        assert_eq!(hit_test_main(50, 30), Some(MainHit::Time));
        assert_eq!(hit_test_main(120, 30), Some(MainHit::Marquee));
        assert_eq!(
            hit_test_main(14, 50),
            Some(MainHit::Button(MainButton::ClutterD))
        );
    }

    #[test]
    fn hit_test_finds_the_visualiser_area() {
        assert_eq!(hit_test_main(24, 43), Some(MainHit::Vis));
        assert_eq!(hit_test_main(99, 58), Some(MainHit::Vis));
        assert_eq!(hit_test_main(100, 58), None);
    }

    #[test]
    fn doubled_window_lights_the_d_button() {
        let skin = testkit::synthetic_skin();
        let off = render(&skin, &base_state());
        let on = render(
            &skin,
            &MainState {
                doubled: true,
                ..base_state()
            },
        );
        let sprite = skin
            .sprite(Sheet::Titlebar, "MAIN_CLUTTER_BAR_BUTTON_D_SELECTED")
            .expect("D selected sprite");
        assert_eq!(px(&on, CLUTTER_D.x, CLUTTER_D.y), sprite.pixel(0, 0));
        assert_ne!(
            px(&off, CLUTTER_D.x, CLUTTER_D.y),
            px(&on, CLUTTER_D.x, CLUTTER_D.y)
        );
    }

    #[test]
    fn hit_test_misses_empty_areas_and_outside_points() {
        assert_eq!(hit_test_main(100, 110), None);
        assert_eq!(hit_test_main(60, 60), None);
        assert_eq!(hit_test_main(-1, 5), None);
        assert_eq!(hit_test_main(275, 5), None);
        assert_eq!(hit_test_main(5, 116), None);
    }

    #[test]
    fn hit_test_clutter_bar_letters_are_stacked_in_order() {
        assert_eq!(
            hit_test_main(14, 26),
            Some(MainHit::Button(MainButton::ClutterO))
        );
        assert_eq!(
            hit_test_main(14, 34),
            Some(MainHit::Button(MainButton::ClutterA))
        );
        assert_eq!(
            hit_test_main(14, 41),
            Some(MainHit::Button(MainButton::ClutterI))
        );
        assert_eq!(
            hit_test_main(14, 56),
            Some(MainHit::Button(MainButton::ClutterV))
        );
    }

    #[test]
    fn time_digits_wrap_minutes_at_100() {
        assert_eq!(time_digits(83), [0, 1, 2, 3]);
        assert_eq!(time_digits(59 * 60 + 59), [5, 9, 5, 9]);
        assert_eq!(time_digits(100 * 60), [0, 0, 0, 0]);
    }

    #[test]
    fn marquee_period_matches_the_loop_length() {
        assert_eq!(marquee_period_px("x"), 0);
        assert_eq!(marquee_period_px(&"x".repeat(31)), (31 + 7) * 5);
    }
}
