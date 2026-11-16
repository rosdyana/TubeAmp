//! The windows and their event loop: the main window, the equalizer and the playlist editor (all
//! skinned, see `skinned.rs`), the YouTube Music bridge, the audio tap, the EQ and the visualiser,
//! wired together. The playlist decides what plays next: the main window's previous and next
//! buttons, Z and B, and the end of each track all go through it.
//!
//! The bridge webview is a child of the video window. The Eject button, the L key and Alt+V show
//! and hide that window. Closing it only hides it, so the playback goes on.
//!
//! Windows are dragged by their title bars with our own drag (not the OS drag), so they snap to
//! each other and to the screen edges (see `layout.rs`). Dragging the main window moves the group
//! of windows connected to it. Changing a window's size (double size, windowshade) moves the
//! windows touching it, as Webamp does.

mod session;

use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tubeamp_audio::vis::{Vis, VisFrame};
#[cfg(target_os = "macos")]
use tubeamp_audio::guard::{denial_suspected, DENIAL_WINDOW};
use tubeamp_audio::{params as eq_params, AudioEngine, AudioError, Capture, CaptureEvent};
use tubeamp_bridge::{Bridge, Cmd, Evt};
use tubeamp_eq::model::ZERO_VALUE;
use tubeamp_eq::{Preset, BANDS_HZ, BUILTIN};
use tubeamp_playlist::{parse_links, Entry, Link, Playlist};
use tubeamp_skin::Skin;
use tubeamp_ui::eq_window as eqw;
use tubeamp_ui::gen_window as gen;
use tubeamp_ui::library_window as lw;
use tubeamp_ui::main_window::{self as mw, BALANCE, CLUTTER_O, POSITION, VOLUME};
use tubeamp_ui::playlist_window as plw;
use tubeamp_ui::video_window::{self as vw, VideoHit, VideoWindowState};
use tubeamp_ui::{
    displayed_secs, hit_test_eq, hit_test_eq_shade, hit_test_library, hit_test_main,
    hit_test_main_shade, hit_test_playlist, hit_test_playlist_shade, labels, marquee_period_px,
    menu_button_rect, press_grab, render_eq, render_library, render_main, render_playlist,
    render_vis, row_index_at, slider_value_at, sliders, Canvas, EqHit, EqSlider, EqWindowState,
    LibHit, LibKind, MainButton, MainHit, MainState, PlHit, PlMenu, PlTransport,
    PlaylistWindowState, Status, HEIGHT, PRESETS_BTN, WIDTH,
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{WindowAttributes, WindowId};

use crate::config::{self, Loaded, Setup};
use crate::eq::{self, EqCommand};
use crate::hotkeys::{self, KeyName, Shortcut, Stroke};
use crate::layout::{self, Bx, Win, SNAP_DISTANCE};
#[cfg(target_os = "windows")]
use crate::nowplaying::MediaAction;
use crate::library::{self, Action, LibCommand, Library};
use crate::milkdrop::{self, Effect as MilkdropEffect, Msg as MilkdropMsg, MilkdropWindow};
use crate::player::{Playback, RateMeter, FALLBACK_KHZ};
use crate::playlist::{self as pl_app, PlAction, SortCommand};
use crate::skinned::{self, scaled, SkinnedWindow};

/// Every window, in the order the layout visits them.
const ALL_WINDOWS: [Win; 6] = [
    Win::Main,
    Win::Eq,
    Win::Playlist,
    Win::Library,
    Win::Video,
    Win::Milkdrop,
];

/// Marquee step: one character cell, as in Webamp's `Marquee.tsx`.
const MARQUEE_STEP_PX: u32 = 5;
/// Webamp steps the marquee every 220 ms.
const MARQUEE_INTERVAL: Duration = Duration::from_millis(220);
/// Redraw interval while playing or dragging (about 30 fps).
const FRAME_ACTIVE: Duration = Duration::from_millis(33);
/// Analyser step interval while playing: 60 Hz, like Webamp's `requestAnimationFrame` loop. The
/// bar falloff is a fixed amount per step, so stepping at the redraw rate would make the bars fall
/// at half the wall-clock speed of Webamp's.
const VIS_STEP: Duration = Duration::from_millis(16);
/// Redraw interval otherwise (about 4 fps).
const FRAME_IDLE: Duration = Duration::from_millis(250);
/// How often to retry the audio tap while playing.
const AUDIO_RETRY: Duration = Duration::from_secs(2);
/// The marquee message when the tap is silent while the page plays (no System Audio Recording).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const AUDIO_OFF_NOTICE: &str = "EQ OFF: ALLOW SYSTEM AUDIO RECORDING FOR TUBEAMP";
/// How long that message stays in the marquee.
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const AUDIO_OFF_SHOWN: Duration = Duration::from_secs(8);
/// The marquee note while the tap's start takes longer than `AUDIO_STARTING_AFTER`. A start can
/// wait on the System Audio Recording prompt, so the user needs to see that it is in progress.
const AUDIO_STARTING_NOTICE: &str = "STARTING AUDIO CAPTURE...";
/// How long a start may take before `AUDIO_STARTING_NOTICE` is shown.
const AUDIO_STARTING_AFTER: Duration = Duration::from_secs(1);
/// Delay before `TUBEAMP_AUTOLOAD` loads its video.
const AUTOLOAD_DELAY: Duration = Duration::from_secs(6);
/// Arrow-key seek step, in seconds (Winamp).
const SEEK_STEP_SECS: f64 = 5.0;
/// Arrow-key volume step (the plan's ±2).
const VOLUME_STEP: i32 = 2;
/// The 1× and 2× buttons' content sizes, in skin pixels (the video itself, not the frame).
const VIDEO_1X: (u32, u32) = (320, 180);
const VIDEO_2X: (u32, u32) = (640, 360);
/// The browser view makes the video window at least this big on screen (logical pixels).
const BROWSER_MIN: (u32, u32) = (800, 600);
/// Two clicks on the same playlist row this close together play it.
const DOUBLE_CLICK: Duration = Duration::from_millis(500);
/// Rows scrolled per wheel notch, as Winamp does.
const WHEEL_ROWS: isize = 3;

/// The modifier keys of a key the page forwarded.
#[derive(Debug, Clone, Copy)]
struct PageMods {
    ctrl: bool,
    alt: bool,
    meta: bool,
}

/// Settings from the command line and environment.
#[derive(Debug, Clone)]
pub struct Options {
    /// Integer scale of the main window (1 or 2; 2 is double size).
    pub zoom: u32,
    /// Save the live windows (2×) after the delay, then keep running. The equalizer goes to
    /// `OUT-eq.png` and is written only while it is open.
    pub screenshot: Option<(Duration, PathBuf)>,
    /// Exit after this long.
    pub quit_after: Option<Duration>,
    /// Video to load six seconds after start.
    pub autoload: Option<String>,
    /// Print bridge events and state changes to stderr.
    pub log: bool,
    /// Open the equalizer window at startup, below the main window.
    pub open_eq: bool,
    /// Apply this built-in preset at startup.
    pub eq_preset: Option<Preset>,
    /// Open the playlist editor at startup, below the equalizer or the main window.
    pub open_pl: bool,
    /// Video ids (or links, already validated) appended to the playlist at startup.
    pub add: Vec<String>,
    /// Open the media library window at startup.
    pub open_library: bool,
    /// Run this search in the media library once the page is ready.
    pub library_search: Option<String>,
    /// Show the video window at startup.
    pub open_video: bool,
    /// Show the Milkdrop window at startup.
    pub open_milkdrop: bool,
    /// Start the video window in the browser view (the full YouTube Music page), not video only.
    pub video_browser: bool,
    /// Which windows start in windowshade mode.
    pub shade: ShadeSet,
    /// Print the window rectangles and connections to stderr when a drag ends.
    pub layout_dump: bool,
    /// Where the settings are kept, and whether they are used (`--config-dir`, `--no-config`).
    pub setup: Setup,
    /// The settings read at startup (see `config.rs`).
    pub loaded: Loaded,
    /// A note for the marquee at startup, such as a skin that could not be loaded.
    pub startup_notice: Option<String>,
}

/// The windows that start in windowshade mode (`--shade`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShadeSet {
    pub main: bool,
    pub eq: bool,
    pub pl: bool,
}

/// Events posted to the event loop from outside it.
#[derive(Debug)]
pub enum UserEvent {
    /// A message from `bridge.js`.
    Bridge(Evt),
    /// A PRESETS menu item was chosen. The payload is its menu id.
    Menu(String),
    /// A message from the Milkdrop window's page.
    Milkdrop(MilkdropMsg),
    /// A media key or Now Playing request from the OS (Windows' SMTC, see `nowplaying.rs`).
    #[cfg(target_os = "windows")]
    Media(MediaAction),
    /// The classic skin download finished: the file, or why it failed.
    Download(Result<PathBuf, String>),
    /// Asks the first-run question (posted once, when there is no config yet).
    FirstRun,
    /// The first-run question's answer (Windows asks it on a worker thread).
    #[cfg_attr(not(target_os = "windows"), allow(dead_code))]
    FirstRunAnswer(crate::firstrun::Answer),
}

/// A slider or seek drag in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Drag {
    /// Seeking; `percent` is the target in `0..=100`.
    Position {
        percent: u32,
    },
    Volume,
    Balance,
}

/// An equalizer slider drag in progress. `grab` is the pointer's offset inside the thumb.
#[derive(Debug, Clone, Copy, PartialEq)]
struct EqDrag {
    slider: EqSlider,
    grab: f64,
}

/// A video window resize in progress: the size at the press, and where the pointer was.
#[derive(Debug, Clone, Copy, PartialEq)]
struct VideoResize {
    start: (u32, u32),
    start_cursor: PhysicalPosition<f64>,
}

/// A media library drag in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
enum LibDrag {
    /// The scrollbar thumb follows the pointer.
    Scroll,
    /// The bottom-right grip resizes the window, as for the playlist.
    Resize {
        start_steps: (u32, u32),
        start_cursor: PhysicalPosition<f64>,
    },
}

/// A playlist window drag in progress.
#[derive(Debug, Clone, Copy, PartialEq)]
enum PlDrag {
    /// The scrollbar thumb follows the pointer.
    Scroll,
    /// The bottom-right corner resizes the window. The size is the start size plus the
    /// pointer's movement since the press.
    Resize {
        start_steps: (u32, u32),
        start_cursor: PhysicalPosition<f64>,
    },
    /// Selected rows follow the pointer. `index` is the row pressed, `last` the row under the
    /// pointer, and `moved` whether the selection has moved at all.
    Rows {
        index: usize,
        last: usize,
        moved: bool,
    },
}

/// A window dragged by its title bar. The window pressed follows the pointer, and the windows in
/// `moving` keep their offsets from it, snapping to `stationary` and to the screen.
struct WinDrag {
    /// The window whose title bar was pressed. Its events carry the pointer.
    win: Win,
    /// That window's origin at the press, in physical pixels.
    win_start: (f64, f64),
    /// The pointer on the screen at the press, in physical pixels.
    start_pointer: (f64, f64),
    /// The move applied so far, relative to the start.
    last_move: (f64, f64),
    /// The windows that move, with their rectangles at the press.
    moving: Vec<(Win, Bx)>,
    /// The other visible windows, which the moving ones snap to.
    stationary: Vec<Bx>,
    /// The monitor's rectangle (the screen edges snap too).
    screen: Bx,
    /// The snap distance in physical pixels.
    snap: f64,
}

/// A shade slider drag in progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShadeSlider {
    Volume,
    Balance,
}

struct App {
    opts: Options,
    skin: Skin,
    proxy: EventLoopProxy<UserEvent>,
    state: MainState,
    playback: Playback,
    main: Option<SkinnedWindow>,
    eq: Option<SkinnedWindow>,
    eq_visible: bool,
    eq_state: EqWindowState,
    eq_press: Option<EqHit>,
    eq_drag: Option<EqDrag>,
    /// The shade's volume or balance slider being dragged.
    eq_shade_drag: Option<ShadeSlider>,
    /// The window drag in progress, if a title bar is held.
    win_drag: Option<WinDrag>,
    /// The last title bar press, for double-click detection (it toggles the windowshade).
    title_click: Option<(Win, Instant)>,
    pl: Option<SkinnedWindow>,
    pl_visible: bool,
    pl_state: PlaylistWindowState,
    playlist: Playlist,
    /// The playlist row the pointer pressed, and the hit it was pressed on.
    pl_press: Option<PlHit>,
    pl_drag: Option<PlDrag>,
    /// The last row press, for double-click detection.
    pl_click: Option<(usize, Instant)>,
    /// The media library window, and whether it is showing.
    lib: Option<SkinnedWindow>,
    lib_visible: bool,
    /// Whether the library has been placed beside the main window (on its first show).
    lib_placed: bool,
    /// The library's state and requests (see `library.rs`).
    library: Library,
    lib_press: Option<LibHit>,
    lib_drag: Option<LibDrag>,
    /// The last row press in the library, for double-click detection.
    lib_click: Option<(usize, Instant)>,
    /// Random order for shuffle and randomize.
    rng: fastrand::Rng,
    /// Whether the current entry's end has already moved the playlist on. Cleared once the entry
    /// is seen playing again, so a late end report cannot skip a track twice.
    ended_done: bool,
    // Dropped before `video`, because the webview is a child of the video window.
    bridge: Option<Bridge>,
    /// The video window, which hosts the webview. It is a skinned frame; closing it hides it.
    video: Option<SkinnedWindow>,
    video_visible: bool,
    /// Whether the video window has been placed beside the main window (on its first show).
    video_placed: bool,
    /// The video frame's size, focus, pressed control and info line.
    video_state: VideoWindowState,
    /// The video frame's control under a press that has not been released yet.
    video_press: Option<VideoHit>,
    video_resize: Option<VideoResize>,
    /// Whether the page shows the whole YouTube Music page (the browser view) instead of the video.
    video_browser: bool,
    /// The video window's size before the browser view enlarged it.
    video_saved_size: Option<(u32, u32)>,
    video_fullscreen: bool,
    /// The Milkdrop window and its webview (see `milkdrop.rs`).
    milkdrop: MilkdropWindow,
    /// Gaps between the bridge's state updates, for the log.
    state_rate: RateMeter,
    /// The audio tap: its start runs on a worker thread, and `Denied` means no automatic retry.
    capture: Capture<AudioEngine>,
    audio_given_up: bool,
    audio_error_shown: bool,
    /// Whether `AUDIO_STARTING_NOTICE` is on the marquee (it must be cleared when the start ends).
    audio_note_shown: bool,
    audio_retry_at: Instant,
    /// When the page began reporting playing (`ps == 1`), or `None` while it does not.
    playing_since: Option<Instant>,
    /// The EQ version last sent to the page's Web Audio graph (not macOS). `None` until it is sent,
    /// and again after the page reloads (see `sync_page_eq`).
    #[cfg(not(target_os = "macos"))]
    page_eq_version: Option<u64>,
    vis: Vis,
    vis_frame: VisFrame,
    samples: Vec<f32>,
    modifiers: ModifiersState,
    press_target: Option<MainHit>,
    drag: Option<Drag>,
    volume_synced: bool,
    autoload_at: Option<Instant>,
    screenshot_at: Option<Instant>,
    quit_at: Option<Instant>,
    next_marquee: Instant,
    next_frame: Instant,
    next_vis: Instant,
    /// The saved settings, skin, media controls and tray (see `window/session.rs`).
    session: session::Session,
}

/// Opens the windows and runs the event loop until the main window is closed.
pub fn run(skin: Skin, opts: Options) -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let started = Instant::now();
    let mut opts = opts;
    // The saved double size applies unless `--zoom` asked for a size.
    if opts.zoom == 1 && opts.loaded.config.main.doubled {
        opts.zoom = 2;
    }
    let zoom = opts.zoom;
    let session = session::Session::new(
        opts.setup.clone(),
        opts.loaded.clone(),
        opts.startup_notice.clone(),
        started,
    );
    let screenshot_at = opts.screenshot.as_ref().map(|(after, _)| started + *after);
    let quit_at = opts.quit_after.map(|after| started + after);
    let autoload_at = opts.autoload.as_ref().map(|_| started + AUTOLOAD_DELAY);
    let mut library = Library::new();
    if let Some(query) = opts.library_search.clone() {
        library.set_startup_search(query);
    }

    // PRESETS menu items arrive as menu events. Forward them into the event loop.
    let menu_proxy = event_loop.create_proxy();
    muda::MenuEvent::set_event_handler(Some(move |event: muda::MenuEvent| {
        let _ = menu_proxy.send_event(UserEvent::Menu(event.id.0));
    }));

    let mut app = App {
        opts,
        skin,
        proxy: event_loop.create_proxy(),
        state: MainState {
            doubled: zoom == 2,
            // YouTube Music starts at full volume; the slider is the source of truth from here.
            volume: 100,
            ..MainState::default()
        },
        playback: Playback::default(),
        main: None,
        eq: None,
        eq_visible: false,
        eq_state: EqWindowState::default(),
        eq_press: None,
        eq_drag: None,
        eq_shade_drag: None,
        win_drag: None,
        title_click: None,
        pl: None,
        pl_visible: false,
        pl_state: PlaylistWindowState::default(),
        playlist: Playlist::default(),
        pl_press: None,
        pl_drag: None,
        pl_click: None,
        lib: None,
        lib_visible: false,
        lib_placed: false,
        library,
        lib_press: None,
        lib_drag: None,
        lib_click: None,
        rng: fastrand::Rng::new(),
        ended_done: false,
        bridge: None,
        video: None,
        video_visible: false,
        video_placed: false,
        video_state: VideoWindowState::default(),
        video_press: None,
        video_resize: None,
        // Set in `resumed`, which can only send the view to a page that exists.
        video_browser: false,
        video_saved_size: None,
        video_fullscreen: false,
        milkdrop: MilkdropWindow::default(),
        state_rate: RateMeter::default(),
        capture: Capture::new(),
        audio_given_up: false,
        audio_error_shown: false,
        audio_note_shown: false,
        audio_retry_at: started,
        playing_since: None,
        #[cfg(not(target_os = "macos"))]
        page_eq_version: None,
        vis: Vis::new(),
        vis_frame: VisFrame::default(),
        samples: Vec::new(),
        modifiers: ModifiersState::empty(),
        press_target: None,
        drag: None,
        volume_synced: false,
        autoload_at,
        screenshot_at,
        quit_at,
        next_marquee: started + MARQUEE_INTERVAL,
        next_frame: started,
        next_vis: started,
        session,
    };
    let result = event_loop.run_app(&mut app);
    // However the loop ended, the settings of the last state are written.
    app.save_config_now();
    result
}

/// `shot.png` → `shot-eq.png`, in the same folder.
pub fn eq_screenshot_path(path: &Path) -> PathBuf {
    suffixed_path(path, "eq")
}

/// `shot.png` → `shot-<suffix>.png`, in the same folder.
pub fn suffixed_path(path: &Path, suffix: &str) -> PathBuf {
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = match path.extension() {
        Some(ext) => format!("{stem}-{suffix}.{}", ext.to_string_lossy()),
        None => format!("{stem}-{suffix}"),
    };
    path.with_file_name(name)
}

fn save_one(pixels: &[u32], width: u32, height: u32, path: &Path) {
    match crate::save_png(pixels, width, height, path) {
        Ok(()) => eprintln!("tubeamp: wrote {}", path.display()),
        Err(e) => eprintln!("tubeamp: could not write {}: {e}", path.display()),
    }
}

/// The shortcut key of a winit key press, ignoring the modifiers' effect on the character.
fn key_name_of(event: &KeyEvent) -> Option<KeyName> {
    match event.key_without_modifiers() {
        Key::Named(NamedKey::ArrowLeft) => Some(KeyName::Left),
        Key::Named(NamedKey::ArrowRight) => Some(KeyName::Right),
        Key::Named(NamedKey::ArrowUp) => Some(KeyName::Up),
        Key::Named(NamedKey::ArrowDown) => Some(KeyName::Down),
        Key::Named(NamedKey::Escape) => Some(KeyName::Escape),
        Key::Character(c) => {
            let mut chars = c.chars();
            match (chars.next(), chars.next()) {
                (Some(ch), None) => Some(KeyName::Char(ch.to_ascii_lowercase())),
                _ => None,
            }
        }
        _ => None,
    }
}

impl App {
    fn log(&self, msg: String) {
        if self.opts.log {
            eprintln!("tubeamp: {msg}");
        }
    }

    /// Requests a redraw of the main window, and of the equalizer and playlist while open.
    fn request_redraw(&self) {
        if let Some(main) = &self.main {
            main.request_redraw();
        }
        if self.eq_visible {
            if let Some(eq) = &self.eq {
                eq.request_redraw();
            }
        }
        if self.pl_visible {
            if let Some(pl) = &self.pl {
                pl.request_redraw();
            }
        }
        if self.lib_visible {
            if let Some(lib) = &self.lib {
                lib.request_redraw();
            }
        }
        if self.video_visible {
            if let Some(video) = &self.video {
                video.request_redraw();
            }
        }
    }

    fn send(&self, cmd: Cmd) {
        let Some(bridge) = &self.bridge else {
            return;
        };
        self.log(format!("-> {cmd:?}"));
        if let Err(e) = bridge.send(&cmd) {
            eprintln!("tubeamp: could not send a command to YouTube Music: {e}");
        }
    }

    /// The main window's hit target under the pointer.
    fn main_hit(&self) -> Option<MainHit> {
        let (x, y) = self.main.as_ref()?.skin_pixel()?;
        if self.state.shade {
            hit_test_main_shade(x, y)
        } else {
            hit_test_main(x, y)
        }
    }

    /// The equalizer's hit target under the pointer.
    fn eq_hit_under_cursor(&self) -> Option<EqHit> {
        let (x, y) = self.eq.as_ref()?.skin_pixel()?;
        if self.eq_state.shade {
            hit_test_eq_shade(x, y)
        } else {
            hit_test_eq(x, y)
        }
    }

    // Window geometry and the layout (snapping, connections, size changes).

    fn window(&self, win: Win) -> Option<&SkinnedWindow> {
        match win {
            Win::Main => self.main.as_ref(),
            Win::Eq => self.eq.as_ref(),
            Win::Playlist => self.pl.as_ref(),
            Win::Library => self.lib.as_ref(),
            Win::Video => self.video.as_ref(),
            Win::Milkdrop => self.milkdrop.window(),
        }
    }

    fn window_visible(&self, win: Win) -> bool {
        match win {
            Win::Main => true,
            Win::Eq => self.eq_visible,
            Win::Playlist => self.pl_visible,
            Win::Library => self.lib_visible,
            Win::Video => self.video_visible,
            Win::Milkdrop => self.milkdrop.visible(),
        }
    }

    /// The window's size in skin pixels, from the state (not from the OS, which applies a resize
    /// later).
    fn base_size(&self, win: Win) -> (u32, u32) {
        match win {
            Win::Main => (WIDTH, mw::window_height(self.state.shade)),
            Win::Eq => (eqw::WIDTH, eqw::window_height(self.eq_state.shade)),
            Win::Playlist => plw::window_size_for(self.pl_state.size_steps, self.pl_state.shade),
            Win::Library => gen::window_size(self.library.state.size_steps),
            Win::Video => (self.video_state.w, self.video_state.h),
            Win::Milkdrop => gen::window_size(self.milkdrop.steps()),
        }
    }

    /// The window's size in physical pixels, as it should be now.
    fn window_px_size(&self, win: Win) -> (f64, f64) {
        let (w, h) = self.base_size(win);
        let factor = self.window(win).map_or(1, |sw| sw.factor());
        (f64::from(w * factor), f64::from(h * factor))
    }

    /// The window's origin in physical pixels, if it exists.
    fn window_origin(&self, win: Win) -> Option<(f64, f64)> {
        let pos = self.window(win)?.window.outer_position().ok()?;
        Some((f64::from(pos.x), f64::from(pos.y)))
    }

    fn set_window_origin(&self, win: Win, origin: (f64, f64)) {
        if let Some(sw) = self.window(win) {
            sw.window.set_outer_position(PhysicalPosition::new(
                origin.0.round() as i32,
                origin.1.round() as i32,
            ));
        }
    }

    /// The rectangles of the visible windows, physical pixels.
    fn visible_rects(&self) -> Vec<(Win, Bx)> {
        ALL_WINDOWS
            .iter()
            .filter(|&&w| self.window_visible(w))
            .filter_map(|&w| {
                let (x, y) = self.window_origin(w)?;
                let (sw, sh) = self.window_px_size(w);
                Some((w, Bx::new(x, y, sw, sh)))
            })
            .collect()
    }

    /// The monitor the main window is on, in physical pixels. Without one, the screen is unbounded.
    fn screen_rect(&self) -> Bx {
        self.main
            .as_ref()
            .and_then(|m| m.window.current_monitor())
            .map_or(Bx::new(-1.0e7, -1.0e7, 2.0e7, 2.0e7), |m| {
                let p = m.position();
                let s = m.size();
                Bx::new(
                    f64::from(p.x),
                    f64::from(p.y),
                    f64::from(s.width),
                    f64::from(s.height),
                )
            })
    }

    /// The snap distance in physical pixels for the window's monitor.
    fn snap_px(&self, win: Win) -> f64 {
        let scale = self.window(win).map_or(1.0, |sw| sw.window.scale_factor());
        SNAP_DISTANCE * scale
    }

    /// Starts a drag of `win` by its title bar. Dragging the main window takes its connected group
    /// along; any other window moves alone.
    fn begin_window_drag(&mut self, win: Win) {
        let Some(origin) = self.window_origin(win) else {
            return;
        };
        let Some(cursor) = self.window(win).and_then(|sw| sw.cursor_position()) else {
            return;
        };
        let all = self.visible_rects();
        let snap = self.snap_px(win);
        let group: BTreeSet<Win> = if win == Win::Main {
            layout::connected_group(&all, Win::Main, snap)
        } else {
            [win].into_iter().collect()
        };
        let moving: Vec<(Win, Bx)> = all
            .iter()
            .filter(|(w, _)| group.contains(w))
            .copied()
            .collect();
        let stationary: Vec<Bx> = all
            .iter()
            .filter(|(w, _)| !group.contains(w))
            .map(|(_, b)| *b)
            .collect();
        self.log(format!(
            "drag: start {win:?} origin {origin:?} cursor ({:.1}, {:.1}) factor {}",
            cursor.x,
            cursor.y,
            self.window(win).map_or(0, |sw| sw.factor())
        ));
        self.win_drag = Some(WinDrag {
            win,
            win_start: origin,
            start_pointer: (origin.0 + cursor.x, origin.1 + cursor.y),
            last_move: (0.0, 0.0),
            moving,
            stationary,
            screen: self.screen_rect(),
            snap,
        });
    }

    /// Moves the dragged windows to the pointer, snapped. Called by the dragged window's events.
    fn update_window_drag(&mut self) {
        let Some(drag) = self.win_drag.as_ref() else {
            return;
        };
        let Some(cursor) = self.window(drag.win).and_then(|sw| sw.cursor_position()) else {
            return;
        };
        // The dragged window's own position is where we last put it, so the pointer's screen
        // position is that origin plus the pointer's offset inside the window.
        let pointer = (
            drag.win_start.0 + drag.last_move.0 + cursor.x,
            drag.win_start.1 + drag.last_move.1 + cursor.y,
        );
        let proposed = (
            pointer.0 - drag.start_pointer.0,
            pointer.1 - drag.start_pointer.1,
        );
        // A press that has not moved the pointer moves nothing, even if the group would snap.
        if proposed == (0.0, 0.0) {
            return;
        }
        let boxes: Vec<Bx> = drag.moving.iter().map(|(_, b)| *b).collect();
        let delta = layout::drag_diff(proposed, &boxes, &drag.stationary, &drag.screen, drag.snap);
        let moves: Vec<(Win, (f64, f64))> = drag
            .moving
            .iter()
            .map(|(w, b)| (*w, (b.x + delta.0, b.y + delta.1)))
            .collect();
        if let Some(d) = self.win_drag.as_mut() {
            d.last_move = delta;
        }
        for (w, origin) in moves {
            self.set_window_origin(w, origin);
        }
    }

    /// Ends the window drag, if one is running. With `--layout-dump` it prints the layout. A press
    /// that moved the window is not the first half of a double-click.
    fn end_window_drag(&mut self) {
        let Some(drag) = self.win_drag.take() else {
            return;
        };
        if drag.last_move != (0.0, 0.0) {
            self.title_click = None;
        }
        if self.opts.layout_dump {
            self.dump_layout();
        }
    }

    /// Prints the visible windows' rectangles and the connections to stderr.
    fn dump_layout(&self) {
        let all = self.visible_rects();
        for (w, b) in &all {
            eprintln!("layout: {w:?} x={} y={} w={} h={}", b.x, b.y, b.w, b.h);
        }
        let edges = layout::adjacency(&all);
        let mut touching = Vec::new();
        for (w, e) in &edges {
            if let Some(t) = e.top {
                touching.push(format!("{w:?} below {t:?}"));
            }
            if let Some(l) = e.left {
                touching.push(format!("{w:?} right of {l:?}"));
            }
        }
        touching.sort();
        let group = layout::connected_group(&all, Win::Main, self.snap_px(Win::Main));
        eprintln!(
            "layout: edges [{}]; connected to main [{}]",
            touching.join(", "),
            group
                .iter()
                .map(|w| format!("{w:?}"))
                .collect::<Vec<_>>()
                .join(", ")
        );
    }

    /// Changes a window's size or shade with `change`, and moves the windows that touch it, the
    /// way Webamp does: the windows below or to the right move by the change in size. The sizes
    /// are read from the state, before and after.
    fn relayout(&mut self, change: impl FnOnce(&mut App)) {
        let before = self.visible_rects();
        let graph = layout::adjacency(&before);
        let before_sizes: HashMap<Win, (f64, f64)> =
            before.iter().map(|(w, b)| (*w, (b.w, b.h))).collect();
        let origins: Vec<(Win, (f64, f64))> =
            before.iter().map(|(w, b)| (*w, (b.x, b.y))).collect();
        change(self);
        let size_diff: HashMap<Win, (f64, f64)> = before
            .iter()
            .map(|(w, _)| {
                let (nw, nh) = self.window_px_size(*w);
                let (ow, oh) = before_sizes[w];
                (*w, (nw - ow, nh - oh))
            })
            .collect();
        let diff = layout::position_diff(&graph, &size_diff);
        for (w, (x, y)) in origins {
            let d = diff.get(&w).copied().unwrap_or((0.0, 0.0));
            self.set_window_origin(w, (x + d.0, y + d.1));
        }
        self.request_redraw();
    }

    /// Gives every window its skin size and zoom, from the state.
    fn apply_sizes(&mut self) {
        let zoom = if self.state.doubled { 2 } else { 1 };
        for win in ALL_WINDOWS {
            let base = self.base_size(win);
            let slot = match win {
                Win::Main => self.main.as_mut(),
                Win::Eq => self.eq.as_mut(),
                Win::Playlist => self.pl.as_mut(),
                Win::Library => self.lib.as_mut(),
                Win::Video => self.video.as_mut(),
                Win::Milkdrop => self.milkdrop.window_mut(),
            };
            if let Some(sw) = slot {
                sw.set_zoom(zoom);
                sw.set_base(base);
            }
        }
    }

    /// A title bar was pressed. A second press within [`DOUBLE_CLICK`] toggles the windowshade of
    /// the main window, the equalizer and the playlist; otherwise the press starts a drag.
    fn title_press(&mut self, win: Win) {
        let now = Instant::now();
        if let Some((last, at)) = self.title_click {
            if last == win && now.duration_since(at) <= DOUBLE_CLICK {
                self.title_click = None;
                self.toggle_shade_of(win);
                return;
            }
        }
        self.title_click = Some((win, now));
        self.begin_window_drag(win);
    }

    /// The windowshade toggle for `win`. Only the main window, the equalizer and the playlist have
    /// one; the library and the video window do nothing.
    fn toggle_shade_of(&mut self, win: Win) {
        match win {
            Win::Main => self.toggle_main_shade(),
            Win::Eq => self.toggle_eq_shade(),
            Win::Playlist => self.toggle_pl_shade(),
            Win::Library | Win::Video | Win::Milkdrop => {}
        }
    }

    fn toggle_main_shade(&mut self) {
        let on = !self.state.shade;
        self.relayout(|app| {
            app.state.shade = on;
            app.apply_sizes();
        });
    }

    fn toggle_eq_shade(&mut self) {
        let on = !self.eq_state.shade;
        self.relayout(|app| {
            app.eq_state.shade = on;
            app.apply_sizes();
        });
    }

    fn toggle_pl_shade(&mut self) {
        let on = !self.pl_state.shade;
        self.relayout(|app| {
            app.pl_state.shade = on;
            app.apply_sizes();
        });
    }

    /// Snaps a resize of `win` to `(w, h)` skin pixels: its right and bottom edges move onto the
    /// other windows' and the screen's edges when they are close. Returns the snapped skin size.
    fn snapped_skin_size(&self, win: Win, w: f64, h: f64) -> (f64, f64) {
        let Some(origin) = self.window_origin(win) else {
            return (w, h);
        };
        let factor = self.window(win).map_or(1, |sw| sw.factor());
        let factor = f64::from(factor);
        let rect = Bx::new(origin.0, origin.1, w * factor, h * factor);
        let others: Vec<Bx> = self
            .visible_rects()
            .into_iter()
            .filter(|(o, _)| *o != win)
            .map(|(_, b)| b)
            .collect();
        let snapped = layout::snap_resize(&rect, &others, &self.screen_rect(), self.snap_px(win));
        (snapped.w / factor, snapped.h / factor)
    }

    fn on_press(&mut self) {
        let hit = self.main_hit();
        self.press_target = hit;
        match hit {
            Some(MainHit::TitleBar) => self.title_press(Win::Main),
            Some(MainHit::Button(button)) => {
                self.state.pressed = Some(button);
                let drag = match button {
                    MainButton::Position => Some(Drag::Position { percent: 0 }),
                    MainButton::Volume => Some(Drag::Volume),
                    MainButton::Balance => Some(Drag::Balance),
                    _ => None,
                };
                if drag.is_some() {
                    self.drag = drag;
                    self.update_drag();
                }
            }
            _ => {}
        }
        self.request_redraw();
    }

    /// Moves the active slider drag to the pointer.
    fn update_drag(&mut self) {
        let Some((x, _)) = self.main.as_ref().and_then(|m| m.skin_point()) else {
            return;
        };
        match self.drag {
            Some(Drag::Position { .. }) => {
                let (percent, fraction) = if self.state.shade {
                    let percent = mw::shade_position_percent(x);
                    (percent, percent as f32 / 100.0)
                } else {
                    let fraction = sliders::position_fraction(x - f64::from(POSITION.x));
                    (sliders::percent(fraction), fraction)
                };
                self.drag = Some(Drag::Position { percent });
                self.state.position = fraction;
            }
            Some(Drag::Volume) => {
                self.set_volume(sliders::volume_from_x(x - f64::from(VOLUME.x)));
            }
            Some(Drag::Balance) => {
                self.set_balance(sliders::balance_from_x(x - f64::from(BALANCE.x)));
            }
            None => return,
        }
        self.request_redraw();
    }

    fn set_volume(&mut self, level: u8) {
        if level != self.state.volume {
            self.state.volume = level;
            self.send(Cmd::Volume { level });
        }
    }

    fn set_balance(&mut self, balance: i8) {
        if balance != self.state.balance {
            self.state.balance = balance;
            eq_params().set_balance(balance);
        }
    }

    fn nudge_volume(&mut self, delta: i32) {
        let level = (i32::from(self.state.volume) + delta).clamp(0, 100) as u8;
        self.set_volume(level);
    }

    fn seek_percent(&mut self, percent: u32) {
        let duration = self.playback.duration_secs;
        if duration == 0 {
            return;
        }
        let secs = f64::from(duration) * f64::from(percent) / 100.0;
        self.send(Cmd::Seek { secs });
    }

    fn seek_relative(&mut self, delta: f64) {
        let duration = self.playback.duration_secs;
        if duration == 0 {
            return;
        }
        let target = (f64::from(self.playback.time_secs) + delta).clamp(0.0, f64::from(duration));
        self.send(Cmd::Seek { secs: target });
    }

    fn on_release(&mut self, el: &ActiveEventLoop) {
        self.end_window_drag();
        let target = self.press_target.take();
        let hit = self.main_hit();
        self.state.pressed = None;
        if let Some(Drag::Position { percent }) = self.drag.take() {
            self.seek_percent(percent);
        }
        if let Some(target) = target {
            if Some(target) == hit {
                self.activate(el, target);
            }
        }
        self.request_redraw();
    }

    /// Runs the action of a control that was pressed and released on the same spot.
    fn activate(&mut self, el: &ActiveEventLoop, hit: MainHit) {
        match hit {
            MainHit::Button(button) => match button {
                MainButton::Close => el.exit(),
                MainButton::Minimize => {
                    if let Some(main) = &self.main {
                        main.window.set_minimized(true);
                    }
                }
                MainButton::Previous => self.prev_track(),
                MainButton::Play => self.send(Cmd::Play),
                MainButton::Pause => self.send(Cmd::Pause),
                MainButton::Stop => self.send(Cmd::Stop),
                MainButton::Next => self.next_track(),
                MainButton::Eject => self.toggle_video(),
                MainButton::Shuffle => self.state.shuffle = !self.state.shuffle,
                MainButton::Repeat => self.state.repeat = !self.state.repeat,
                MainButton::EqToggle => self.toggle_eq(),
                MainButton::PlToggle => self.toggle_pl(),
                MainButton::ClutterO => self.open_main_menu(),
                MainButton::ClutterD => {
                    let on = !self.state.doubled;
                    self.set_doubled(on);
                }
                MainButton::Shade => self.toggle_main_shade(),
                _ => {}
            },
            MainHit::Time | MainHit::MiniTime => {
                self.state.remaining_mode = !self.state.remaining_mode
            }
            MainHit::Vis => self.state.vis_mode = self.state.vis_mode.next(),
            _ => {}
        }
        self.request_redraw();
    }

    /// A native key press: a shortcut (see `hotkeys.rs`), unless the window handles the key
    /// itself. Key repeats only move the arrows.
    fn on_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let Some(key) = key_name_of(event) else {
            return;
        };
        let stroke = Stroke {
            key,
            ctrl: self.modifiers.control_key(),
            alt: self.modifiers.alt_key(),
            meta: self.modifiers.super_key(),
        };
        if let Some(shortcut) = hotkeys::shortcut_for(&stroke, event.repeat) {
            if self.run_shortcut(shortcut) {
                self.request_redraw();
            }
        }
    }

    /// Runs a shortcut. Returns whether it was handled: Escape is not, outside fullscreen.
    fn run_shortcut(&mut self, shortcut: Shortcut) -> bool {
        match shortcut {
            Shortcut::ToggleDouble => {
                let on = !self.state.doubled;
                self.set_doubled(on);
            }
            Shortcut::ToggleRemaining => self.state.remaining_mode = !self.state.remaining_mode,
            Shortcut::ToggleLibrary => self.toggle_library(),
            Shortcut::ToggleVideo => self.toggle_video(),
            Shortcut::ToggleMilkdrop => self.toggle_milkdrop(),
            Shortcut::SeekBack => self.seek_relative(-SEEK_STEP_SECS),
            Shortcut::SeekForward => self.seek_relative(SEEK_STEP_SECS),
            Shortcut::VolumeUp => self.nudge_volume(VOLUME_STEP),
            Shortcut::VolumeDown => self.nudge_volume(-VOLUME_STEP),
            Shortcut::Previous => self.prev_track(),
            Shortcut::Play => self.send(Cmd::Play),
            Shortcut::Pause => self.send(Cmd::Pause),
            Shortcut::Stop => self.send(Cmd::Stop),
            Shortcut::Next => self.next_track(),
            Shortcut::Escape => {
                if !self.video_fullscreen {
                    return false;
                }
                self.toggle_video_fullscreen();
            }
        }
        true
    }

    /// A key the YouTube Music page forwarded (video view). The same shortcut as a native press.
    /// When TubeAmp used the key, its main window takes the keyboard, so the page does not keep
    /// it: the next keys come to TubeAmp's own windows.
    fn on_page_key(
        &mut self,
        key: &str,
        code: &str,
        mods: PageMods,
        repeat: bool,
        source_shown: bool,
    ) {
        let Some(shortcut) = hotkeys::page_shortcut(
            key,
            code,
            mods.ctrl,
            mods.alt,
            mods.meta,
            repeat,
            source_shown,
        ) else {
            return;
        };
        if self.run_shortcut(shortcut) {
            if let Some(main) = self.main.as_ref() {
                main.window.focus_window();
            }
            self.request_redraw();
        }
    }

    /// Doubles (or halves) the windows. The windows touching them move with them (`relayout`).
    fn set_doubled(&mut self, on: bool) {
        self.relayout(|app| {
            app.state.doubled = on;
            app.apply_sizes();
        });
        self.update_video_bounds();
    }

    // Playlist window.

    /// The playlist's hit target under the pointer.
    fn pl_hit_under_cursor(&self) -> Option<PlHit> {
        let (x, y) = self.pl.as_ref()?.skin_pixel()?;
        if self.pl_state.shade {
            return hit_test_playlist_shade(self.pl_state.size_steps, x, y);
        }
        hit_test_playlist(
            self.pl_state.size_steps,
            self.pl_state.scroll,
            self.playlist.len(),
            self.pl_state.open_menu,
            x,
            y,
        )
    }

    fn modifier_held(&self) -> bool {
        self.modifiers.shift_key() || self.modifiers.control_key() || self.modifiers.super_key()
    }

    fn show_pl(&mut self) {
        if self.pl.is_none() {
            return;
        }
        self.place_playlist_below_stack();
        if let Some(pl) = self.pl.as_ref() {
            pl.window.set_visible(true);
        }
        self.pl_visible = true;
        self.state.pl_open = true;
        self.request_redraw();
    }

    fn hide_pl(&mut self) {
        self.pl_visible = false;
        self.state.pl_open = false;
        self.pl_press = None;
        self.pl_drag = None;
        self.pl_state.pressed = None;
        self.pl_state.hover = None;
        self.pl_state.open_menu = None;
        if let Some(pl) = self.pl.as_ref() {
            pl.window.set_visible(false);
        }
        self.request_redraw();
    }

    fn toggle_pl(&mut self) {
        if self.pl_visible {
            self.hide_pl();
        } else {
            self.show_pl();
        }
    }

    /// Where the playlist goes when it is shown: under the equalizer when that is open, otherwise
    /// under the main window, both at the main window's left edge. The heights come from the state.
    fn below_stack_point(&self) -> Option<(f64, f64)> {
        let (x, y) = self.window_origin(Win::Main)?;
        if self.eq_visible {
            let (_, eq_y) = self.window_origin(Win::Eq)?;
            let (_, eq_h) = self.window_px_size(Win::Eq);
            return Some((x, eq_y + eq_h));
        }
        let (_, main_h) = self.window_px_size(Win::Main);
        Some((x, y + main_h))
    }

    /// Moves the playlist to where it goes when shown (see `below_stack_point`).
    fn place_playlist_below_stack(&self) {
        if let Some(point) = self.below_stack_point() {
            self.set_window_origin(Win::Playlist, point);
        }
    }

    /// Resizes the playlist to `steps`: the skin size changes and the window follows.
    fn set_pl_steps(&mut self, steps: (u32, u32)) {
        self.pl_state.size_steps = steps;
        self.pl_state.scroll = plw::clamp_scroll(
            self.pl_state.scroll,
            self.playlist.len(),
            plw::visible_rows(steps),
        );
        let shade = self.pl_state.shade;
        if let Some(pl) = self.pl.as_mut() {
            pl.set_base(plw::window_size_for(steps, shade));
        }
        self.request_redraw();
    }

    fn scroll_pl_by(&mut self, rows: isize) {
        let max = self
            .playlist
            .len()
            .saturating_sub(plw::visible_rows(self.pl_state.size_steps));
        let next = (self.pl_state.scroll as isize + rows).clamp(0, max as isize);
        self.pl_state.scroll = next as usize;
        self.request_redraw();
    }

    /// Scrolls so that row `index` is visible.
    fn show_row(&mut self, index: usize) {
        self.pl_state.scroll = plw::scroll_to_show(
            self.pl_state.scroll,
            index,
            self.playlist.len(),
            plw::visible_rows(self.pl_state.size_steps),
        );
    }

    /// After the list changed: keeps the scroll within the list and redraws.
    fn pl_changed(&mut self) {
        self.pl_state.scroll = plw::clamp_scroll(
            self.pl_state.scroll,
            self.playlist.len(),
            plw::visible_rows(self.pl_state.size_steps),
        );
        self.request_redraw();
    }

    fn on_pl_press(&mut self) {
        let hit = self.pl_hit_under_cursor();
        self.pl_press = hit;
        self.pl_state.pressed = hit;
        let was_open = self.pl_state.open_menu;
        // A press anywhere but on an open popup's item closes the popup.
        if !matches!(hit, Some(PlHit::MenuItem(..))) {
            self.pl_state.open_menu = None;
        }
        match hit {
            Some(PlHit::TitleBar) => self.title_press(Win::Playlist),
            Some(PlHit::Resize) => {
                if let Some(cursor) = self.pl.as_ref().and_then(|p| p.cursor_position()) {
                    self.pl_drag = Some(PlDrag::Resize {
                        start_steps: self.pl_state.size_steps,
                        start_cursor: cursor,
                    });
                }
            }
            Some(PlHit::Scrollbar) => {
                self.pl_drag = Some(PlDrag::Scroll);
                self.update_pl_drag();
            }
            Some(PlHit::Row(index)) => self.press_pl_row(index),
            Some(PlHit::List) => {
                if !self.modifier_held() {
                    self.playlist.select_none();
                }
            }
            Some(PlHit::Menu(menu)) => {
                // Clicking the open button again closes it.
                if was_open != Some(menu) {
                    self.pl_state.open_menu = Some(menu);
                }
            }
            Some(PlHit::MenuItem(menu, index)) => {
                self.pl_state.open_menu = None;
                self.run_pl_menu_item(menu, index);
            }
            _ => {}
        }
        self.pl_changed();
    }

    /// A press on a row selects it, the way Webamp does: a plain press on a row outside the
    /// selection selects only that row, ctrl-press toggles it, shift-press extends from the
    /// anchor. A second plain press on the same row within [`DOUBLE_CLICK`] plays it.
    fn press_pl_row(&mut self, index: usize) {
        if self.modifiers.shift_key() {
            self.playlist.shift_click(index);
            return;
        }
        if self.modifiers.control_key() || self.modifiers.super_key() {
            self.playlist.ctrl_click(index);
            return;
        }
        let now = Instant::now();
        if let Some((last, at)) = self.pl_click {
            if last == index && now.duration_since(at) <= DOUBLE_CLICK {
                self.pl_click = None;
                self.play_index(index);
                return;
            }
        }
        self.pl_click = Some((index, now));
        if !self.playlist.is_selected(index) {
            self.playlist.click(index);
        }
        self.pl_drag = Some(PlDrag::Rows {
            index,
            last: index,
            moved: false,
        });
    }

    fn on_pl_release(&mut self) {
        self.end_window_drag();
        let target = self.pl_press.take();
        let hit = self.pl_hit_under_cursor();
        self.pl_state.pressed = None;
        if let Some(PlDrag::Rows {
            index,
            moved: false,
            ..
        }) = self.pl_drag.take()
        {
            // A plain click on one row of a larger selection leaves only that row selected.
            if self.playlist.selected().len() > 1 && !self.modifier_held() {
                self.playlist.click(index);
            }
        }
        if let Some(target) = target {
            if Some(target) == hit {
                self.activate_pl(target);
            }
        }
        self.pl_changed();
    }

    /// Runs the action of a control pressed and released on the same spot.
    fn activate_pl(&mut self, hit: PlHit) {
        match hit {
            PlHit::Close => self.hide_pl(),
            PlHit::Shade => self.toggle_pl_shade(),
            PlHit::Transport(t) => self.run_pl_transport(t),
            PlHit::MiniTime => self.state.remaining_mode = !self.state.remaining_mode,
            _ => {}
        }
    }

    fn run_pl_transport(&mut self, t: PlTransport) {
        match t {
            PlTransport::Previous => self.prev_track(),
            PlTransport::Play => self.send(Cmd::Play),
            PlTransport::Pause => self.send(Cmd::Pause),
            PlTransport::Stop => self.send(Cmd::Stop),
            PlTransport::Next => self.next_track(),
            PlTransport::Eject => self.toggle_video(),
        }
    }

    /// Moves whatever the pointer is dragging: the scrollbar thumb, the resize corner, or the
    /// selected rows.
    fn update_pl_drag(&mut self) {
        let Some(drag) = self.pl_drag else {
            return;
        };
        match drag {
            PlDrag::Scroll => {
                if let Some((_, y)) = self.pl.as_ref().and_then(|p| p.skin_point()) {
                    self.pl_state.scroll = plw::scroll_at(
                        self.pl_state.size_steps,
                        self.playlist.len(),
                        y.floor() as i32,
                    );
                }
            }
            PlDrag::Resize {
                start_steps,
                start_cursor,
            } => {
                let Some(pl) = self.pl.as_ref() else {
                    return;
                };
                let Some(cursor) = pl.cursor_position() else {
                    return;
                };
                let factor = f64::from(pl.factor());
                let (w0, h0) = plw::window_size(start_steps);
                let w = f64::from(w0) + (cursor.x - start_cursor.x) / factor;
                let h = f64::from(h0) + (cursor.y - start_cursor.y) / factor;
                let (w, h) = self.snapped_skin_size(Win::Playlist, w, h);
                let fitted = plw::steps_for_size(w.round() as i32, h.round() as i32);
                // The shade is one strip tall: only the width changes.
                let steps = if self.pl_state.shade {
                    (fitted.0, start_steps.1)
                } else {
                    fitted
                };
                if steps != self.pl_state.size_steps {
                    self.log(format!(
                        "playlist: resize to {steps:?} (w {w:.1}, h {h:.1}, factor {factor}, cursor {:.1},{:.1} from {:.1},{:.1})",
                        cursor.x, cursor.y, start_cursor.x, start_cursor.y
                    ));
                    self.set_pl_steps(steps);
                }
            }
            PlDrag::Rows {
                index,
                last,
                moved: _,
            } => {
                let Some((_, y)) = self.pl.as_ref().and_then(|p| p.skin_point()) else {
                    return;
                };
                let Some(target) = row_index_at(
                    self.pl_state.size_steps,
                    self.pl_state.scroll,
                    self.playlist.len(),
                    y,
                ) else {
                    return;
                };
                if target != last {
                    self.playlist.move_selected(target as isize - last as isize);
                    self.show_row(target);
                    self.pl_drag = Some(PlDrag::Rows {
                        index,
                        last: target,
                        moved: true,
                    });
                }
            }
        }
        self.pl_changed();
    }

    /// Moves the window drag when the window being dragged is `win` (its events carry the pointer).
    fn drag_moved(&mut self, win: Win) {
        if self.win_drag.as_ref().is_some_and(|d| d.win == win) {
            self.update_window_drag();
        }
    }

    fn pl_event(&mut self, event: WindowEvent) {
        match event {
            // Closing the playlist window hides it, like the equalizer.
            WindowEvent::CloseRequested => self.hide_pl(),
            WindowEvent::RedrawRequested => self.redraw_pl(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.request_redraw()
            }
            WindowEvent::Focused(focused) => {
                self.pl_state.focused = focused;
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(pl) = self.pl.as_mut() {
                    pl.set_cursor(Some(position));
                }
                if self.pl_drag.is_some() {
                    self.update_pl_drag();
                }
                self.drag_moved(Win::Playlist);
                self.pl_state.hover = self.pl_hit_under_cursor();
                self.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(pl) = self.pl.as_mut() {
                    pl.set_cursor(None);
                }
                self.pl_state.hover = None;
                self.request_redraw();
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => self.on_pl_press(),
                ElementState::Released => self.on_pl_release(),
            },
            WindowEvent::MouseWheel { delta, .. } => {
                let rows = match delta {
                    MouseScrollDelta::LineDelta(_, y) => -(y.round() as isize) * WHEEL_ROWS,
                    MouseScrollDelta::PixelDelta(p) => {
                        -((p.y / f64::from(plw::ROW_H)).round() as isize)
                    }
                };
                self.scroll_pl_by(rows);
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // The playlist's own keys first; everything else is the global hotkeys.
                if self.pl_key(&event) {
                    self.request_redraw();
                } else {
                    self.on_key(&event);
                }
            }
            _ => {}
        }
    }

    /// Keys for a focused playlist: Up and Down move the selection (with Shift they extend it),
    /// Enter plays, Delete or Backspace removes the selected rows, and Ctrl+A selects all. Returns
    /// whether the key was used.
    fn pl_key(&mut self, event: &KeyEvent) -> bool {
        if event.state != ElementState::Pressed {
            return false;
        }
        if self.modifiers.control_key() {
            let is_a = matches!(event.key_without_modifiers(), Key::Character(c) if c.eq_ignore_ascii_case("a"));
            if is_a && !event.repeat {
                self.playlist.select_all();
                return true;
            }
            return false;
        }
        match event.key_without_modifiers() {
            Key::Named(NamedKey::ArrowUp) => self.move_pl_cursor(-1),
            Key::Named(NamedKey::ArrowDown) => self.move_pl_cursor(1),
            Key::Named(NamedKey::Enter) if !event.repeat => {
                let first_selected = self.playlist.selected().iter().next().copied();
                if let Some(index) = first_selected.or(self.playlist.current()) {
                    self.play_index(index);
                }
            }
            // Mac keyboards send Backspace for the key labelled Delete.
            Key::Named(NamedKey::Delete | NamedKey::Backspace) if !event.repeat => {
                self.playlist.remove_selected()
            }
            _ => return false,
        }
        self.pl_changed();
        true
    }

    /// Moves the selection cursor by `delta` rows from the anchor (or the current entry).
    fn move_pl_cursor(&mut self, delta: isize) {
        let len = self.playlist.len();
        if len == 0 {
            return;
        }
        let base = self.playlist.anchor().or(self.playlist.current());
        let target = match base {
            Some(base) => (base as isize + delta).clamp(0, len as isize - 1) as usize,
            None if delta > 0 => 0,
            None => len - 1,
        };
        if self.modifiers.shift_key() && self.playlist.anchor().is_some() {
            self.playlist.shift_click(target);
        } else {
            self.playlist.click(target);
        }
        self.show_row(target);
    }

    /// Runs the action of popup item `index` of `menu`, if it has one.
    fn run_pl_menu_item(&mut self, menu: PlMenu, index: usize) {
        if let Some(action) = pl_app::action_for(menu, index) {
            self.run_pl_action(action);
        }
    }

    fn run_pl_action(&mut self, action: PlAction) {
        match action {
            PlAction::AddUrl => match pl_app::clipboard_links() {
                Ok(links) => self.add_links(links),
                Err(msg) => eprintln!("tubeamp: {msg}"),
            },
            PlAction::AddFile => self.add_files(),
            PlAction::RemoveAll | PlAction::NewList => {
                self.playlist.clear();
                self.pl_state.scroll = 0;
            }
            PlAction::Crop => self.playlist.crop(),
            PlAction::RemoveSelected => self.playlist.remove_selected(),
            PlAction::SelectAll => self.playlist.select_all(),
            PlAction::SelectNone => self.playlist.select_none(),
            PlAction::InvertSelection => self.playlist.invert_selection(),
            PlAction::Sort => self.open_sort_menu(),
            PlAction::SaveList => self.save_list(),
            PlAction::LoadList => self.load_list(),
        }
        self.pl_changed();
    }

    /// Adds links from ADD ▸ URL or ADD ▸ FILE. A video is appended at once. A playlist, album or
    /// artist is imported by the library, and its tracks are appended as its pages arrive.
    fn add_links(&mut self, links: Vec<Link>) {
        let mut last = None;
        let mut videos = 0;
        for link in &links {
            if let Link::Video(id) = link {
                last = Some(self.playlist.add(Entry::from_id(id)));
                videos += 1;
            }
        }
        if let Some(index) = last {
            self.show_row(index);
            self.log(format!("playlist: added {videos} video(s)"));
        }
        let actions = self.library.import(links);
        self.apply_library(actions);
    }

    /// ADD ▸ FILE: the entries of a JSON file are appended as they are. The links of the other
    /// files are added together, so that one import does not supersede another.
    fn add_files(&mut self) {
        let Some(paths) = pl_app::pick_files_to_add() else {
            return;
        };
        let mut links = Vec::new();
        for path in paths {
            let text = match std::fs::read_to_string(&path) {
                Ok(text) => text,
                Err(e) => {
                    eprintln!("tubeamp: could not read {}: {e}", path.display());
                    continue;
                }
            };
            if pl_app::is_json_list(&text) {
                match Playlist::from_json(&text) {
                    Ok(list) => {
                        for entry in list.entries() {
                            self.playlist.add(entry.clone());
                        }
                    }
                    Err(e) => eprintln!("tubeamp: could not read {}: {e}", path.display()),
                }
            } else {
                links.extend(parse_links(&text));
            }
        }
        if links.is_empty() {
            eprintln!("tubeamp: the files have no YouTube links");
        }
        self.add_links(links);
    }

    /// Shows the MISC ▸ SORT menu at its button. Its choices come back as `UserEvent::Menu`.
    fn open_sort_menu(&mut self) {
        let Some(pl) = self.pl.as_ref() else {
            return;
        };
        let zoom = f64::from(pl.zoom());
        let button = menu_button_rect(self.pl_state.size_steps, PlMenu::Misc);
        let menu = match pl_app::build_sort_menu() {
            Ok(menu) => menu,
            Err(e) => {
                eprintln!("tubeamp: could not build the sort menu: {e}");
                return;
            }
        };
        let x = f64::from(button.x) * zoom;
        let y = f64::from(button.y) * zoom;
        if let Err(e) = skinned::show_context_menu(&pl.window, &menu, x, y) {
            eprintln!("tubeamp: could not show the sort menu: {e}");
        }
        self.request_redraw();
    }

    fn run_sort(&mut self, cmd: SortCommand) {
        match cmd {
            SortCommand::Title => self.playlist.sort_by_title(),
            SortCommand::Artist => self.playlist.sort_by_artist(),
            SortCommand::Reverse => self.playlist.reverse(),
            SortCommand::Randomize => self.playlist.randomize(&mut self.rng),
        }
        self.pl_changed();
    }

    /// LIST ▸ SAVE: the playlist as JSON entries, or as text links when the name ends in `.txt`.
    fn save_list(&mut self) {
        let Some(path) = pl_app::pick_list_to_save() else {
            return;
        };
        match std::fs::write(&path, pl_app::list_text(&self.playlist, &path)) {
            Ok(()) => eprintln!("tubeamp: saved the playlist to {}", path.display()),
            Err(e) => eprintln!("tubeamp: could not write {}: {e}", path.display()),
        }
    }

    /// LIST ▸ LOAD: replaces the playlist with a file written by SAVE, in either format.
    fn load_list(&mut self) {
        let Some(path) = pl_app::pick_list_to_load() else {
            return;
        };
        let loaded = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| pl_app::list_from_text(&text));
        match loaded {
            Ok((list, skipped)) => {
                let count = list.len();
                self.playlist = list;
                self.pl_state.scroll = 0;
                eprintln!("tubeamp: loaded {count} tracks from {}", path.display());
                if skipped > 0 {
                    eprintln!(
                        "tubeamp: left out {skipped} links that are not videos (use ADD ▸ FILE)"
                    );
                }
            }
            Err(msg) => eprintln!("tubeamp: could not load {}: {msg}", path.display()),
        }
    }

    // Media library window.

    /// The library's hit target under the pointer.
    fn lib_hit_under_cursor(&self) -> Option<LibHit> {
        let (x, y) = self.lib.as_ref()?.skin_pixel()?;
        hit_test_library(&self.library.state, x, y)
    }

    /// Shows the library. The first time, it goes to its default place (see `default_origin`).
    fn show_library(&mut self) {
        let Some(lib) = self.lib.as_ref() else {
            return;
        };
        if !self.lib_placed {
            if let Some((x, y)) = self.default_origin(Win::Library) {
                self.set_window_origin(Win::Library, (x, y));
            }
            self.lib_placed = true;
        }
        lib.window.set_visible(true);
        self.lib_visible = true;
        self.request_redraw();
    }

    /// Closing hides the library; its state and requests stay.
    fn hide_library(&mut self) {
        self.lib_visible = false;
        self.library.state.search_active = false;
        self.lib_press = None;
        self.lib_drag = None;
        if let Some(lib) = self.lib.as_ref() {
            lib.window.set_visible(false);
        }
        self.request_redraw();
    }

    fn toggle_library(&mut self) {
        if self.lib_visible {
            self.hide_library();
        } else {
            self.show_library();
        }
    }

    /// Carries out what the library asked for: commands to the page, and changes to the playlist.
    fn apply_library(&mut self, actions: Vec<Action>) {
        for action in actions {
            match action {
                Action::Send(cmd) => self.send(cmd),
                Action::AddAndPlay(entry) => {
                    let index = self.playlist.add(entry);
                    self.show_row(index);
                    self.play_index(index);
                }
                Action::ReplaceAndPlay { entries, start } => {
                    if entries.is_empty() {
                        continue;
                    }
                    self.playlist = Playlist::from_entries(entries);
                    self.pl_state.scroll = 0;
                    let start = start.min(self.playlist.len() - 1);
                    self.play_index(start);
                }
                Action::Append(entries) => {
                    for entry in entries {
                        self.playlist.add(entry);
                    }
                    self.pl_changed();
                }
            }
        }
        self.request_redraw();
    }

    fn run_library_command(&mut self, command: LibCommand) {
        let actions = self.library.run_command(command);
        self.apply_library(actions);
    }

    /// The main window's O menu: Media Library, and room for more items.
    fn open_main_menu(&mut self) {
        let Some(main) = self.main.as_ref() else {
            return;
        };
        let zoom = f64::from(main.zoom());
        let menu = match self.main_menu() {
            Ok(menu) => menu,
            Err(e) => {
                eprintln!("tubeamp: could not build the menu: {e}");
                return;
            }
        };
        let x = f64::from(CLUTTER_O.x) * zoom;
        let y = f64::from(CLUTTER_O.y + CLUTTER_O.h) * zoom;
        if let Err(e) = skinned::show_context_menu(&main.window, &menu, x, y) {
            eprintln!("tubeamp: could not show the menu: {e}");
        }
        self.request_redraw();
    }

    fn on_lib_press(&mut self) {
        let hit = self.lib_hit_under_cursor();
        self.lib_press = hit;
        if hit != Some(LibHit::Search) {
            self.library.state.search_active = false;
        }
        match hit {
            Some(LibHit::TitleBar) => self.begin_window_drag(Win::Library),
            Some(LibHit::Close) => self.hide_library(),
            Some(LibHit::Resize) => {
                if let Some(cursor) = self.lib.as_ref().and_then(|l| l.cursor_position()) {
                    self.lib_drag = Some(LibDrag::Resize {
                        start_steps: self.library.state.size_steps,
                        start_cursor: cursor,
                    });
                }
            }
            Some(LibHit::Search) => {
                self.library.state.search_active = true;
                let end = self.library.state.query.chars().count();
                self.library.state.set_caret(end);
            }
            Some(LibHit::Nav(index)) => {
                let actions = self.library.open_nav(index);
                self.apply_library(actions);
            }
            Some(LibHit::Row(index)) => self.press_lib_row(index),
            Some(LibHit::Scrollbar) => {
                self.lib_drag = Some(LibDrag::Scroll);
                self.update_lib_drag();
            }
            Some(LibHit::List) => {
                if !self.modifier_held() {
                    self.library.state.select_none();
                }
            }
            Some(LibHit::Client | LibHit::Frame) | None => {}
        }
        self.request_redraw();
    }

    /// A press on a row. The back row goes back at once and a header does nothing. A second press
    /// on the same row within [`DOUBLE_CLICK`] activates it (plays it, or opens the album or
    /// playlist). Otherwise the press selects the row, as the modifier keys say.
    fn press_lib_row(&mut self, index: usize) {
        let Some(kind) = self.library.state.rows.get(index).map(|r| r.kind) else {
            return;
        };
        match kind {
            LibKind::Header => return,
            LibKind::Back => {
                let actions = self.library.activate_row(index);
                self.apply_library(actions);
                return;
            }
            _ => {}
        }
        let now = Instant::now();
        if let Some((last, at)) = self.lib_click {
            if last == index && now.duration_since(at) <= DOUBLE_CLICK {
                self.lib_click = None;
                let actions = self.library.activate_row(index);
                self.apply_library(actions);
                return;
            }
        }
        self.lib_click = Some((index, now));
        let shift = self.modifiers.shift_key();
        let ctrl = self.modifiers.control_key() || self.modifiers.super_key();
        let state = &mut self.library.state;
        if shift {
            state.shift_click(index);
        } else if ctrl {
            state.ctrl_click(index);
        } else if !state.selected.contains(&index) {
            state.click(index);
        }
    }

    fn on_lib_release(&mut self) {
        self.end_window_drag();
        self.lib_press = None;
        self.lib_drag = None;
        self.request_redraw();
    }

    /// Moves whatever the pointer is dragging: the scrollbar thumb or the resize grip. Then asks
    /// for the next page if the list has come near its end.
    fn update_lib_drag(&mut self) {
        let Some(drag) = self.lib_drag else {
            return;
        };
        match drag {
            LibDrag::Scroll => {
                if let Some((_, y)) = self.lib.as_ref().and_then(|l| l.skin_point()) {
                    self.library.state.scroll = lw::scroll_at(&self.library.state, y);
                }
            }
            LibDrag::Resize {
                start_steps,
                start_cursor,
            } => {
                let Some(lib) = self.lib.as_ref() else {
                    return;
                };
                let Some(cursor) = lib.cursor_position() else {
                    return;
                };
                let factor = f64::from(lib.factor());
                let (w0, h0) = gen::window_size(start_steps);
                let w = f64::from(w0) + (cursor.x - start_cursor.x) / factor;
                let h = f64::from(h0) + (cursor.y - start_cursor.y) / factor;
                let (w, h) = self.snapped_skin_size(Win::Library, w, h);
                let steps = gen::steps_for_size(w.round() as i32, h.round() as i32);
                if steps != self.library.state.size_steps {
                    self.set_lib_steps(steps);
                }
            }
        }
        let actions = self.library.maybe_more();
        self.apply_library(actions);
        self.request_redraw();
    }

    /// Resizes the library to `steps`: the state and the window follow.
    fn set_lib_steps(&mut self, steps: (u32, u32)) {
        self.library.state.size_steps = steps;
        self.library.state.clamp_scroll();
        if let Some(lib) = self.lib.as_mut() {
            lib.set_base(gen::window_size(steps));
        }
        self.request_redraw();
    }

    /// The wheel scrolls the list, or the nav column when the pointer is over it.
    fn lib_wheel(&mut self, delta: MouseScrollDelta) {
        let rows = match delta {
            MouseScrollDelta::LineDelta(_, y) => -(y.round() as isize) * WHEEL_ROWS,
            MouseScrollDelta::PixelDelta(p) => -((p.y / f64::from(lw::ROW_H)).round() as isize),
        };
        if matches!(self.lib_hit_under_cursor(), Some(LibHit::Nav(_))) {
            self.library.state.scroll_nav_by(rows);
        } else {
            self.library.state.scroll_by(rows);
        }
        let actions = self.library.maybe_more();
        self.apply_library(actions);
        self.request_redraw();
    }

    /// A right press on a track row selects it (if it is not selected) and shows the row menu.
    fn on_lib_right_press(&mut self) {
        let Some(LibHit::Row(index)) = self.lib_hit_under_cursor() else {
            return;
        };
        if !self.library.state.is_selectable(index) {
            return;
        }
        if !self.library.state.selected.contains(&index) {
            self.library.state.click(index);
        }
        let Some(lib) = self.lib.as_ref() else {
            return;
        };
        let Some((x, y)) = lib.skin_point() else {
            return;
        };
        let zoom = f64::from(lib.zoom());
        let menu = match library::build_row_menu() {
            Ok(menu) => menu,
            Err(e) => {
                eprintln!("tubeamp: could not build the menu: {e}");
                return;
            }
        };
        if let Err(e) = skinned::show_context_menu(&lib.window, &menu, x * zoom, y * zoom) {
            eprintln!("tubeamp: could not show the menu: {e}");
        }
        self.request_redraw();
    }

    /// Keys for the library. While the search field is active it takes the typed text and the
    /// editing keys, so no Winamp hotkey fires. Otherwise Up and Down move the selection and Enter
    /// activates it. Returns whether the key was used. Ctrl and Alt combinations go to the global
    /// hotkeys.
    fn lib_key(&mut self, event: &KeyEvent) -> bool {
        if event.state != ElementState::Pressed {
            return false;
        }
        if self.modifiers.control_key() || self.modifiers.alt_key() || self.modifiers.super_key() {
            return false;
        }
        if self.library.state.search_active {
            let actions = match event.key_without_modifiers() {
                Key::Named(NamedKey::Enter) => {
                    if event.repeat {
                        Vec::new()
                    } else {
                        self.library.submit_search()
                    }
                }
                Key::Named(NamedKey::Escape) => {
                    self.library.state.search_active = false;
                    Vec::new()
                }
                Key::Named(NamedKey::Backspace) => {
                    self.library.state.backspace();
                    Vec::new()
                }
                Key::Named(NamedKey::Delete) => {
                    self.library.state.delete_forward();
                    Vec::new()
                }
                Key::Named(NamedKey::ArrowLeft) => {
                    self.library.state.move_caret(-1);
                    Vec::new()
                }
                Key::Named(NamedKey::ArrowRight) => {
                    self.library.state.move_caret(1);
                    Vec::new()
                }
                Key::Named(NamedKey::Home) => {
                    self.library.state.set_caret(0);
                    Vec::new()
                }
                Key::Named(NamedKey::End) => {
                    self.library.state.set_caret(usize::MAX);
                    Vec::new()
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.library.state.search_active = false;
                    self.library.state.move_cursor(1, false);
                    self.library.maybe_more()
                }
                _ => {
                    let Some(text) = event.text.as_ref() else {
                        return false;
                    };
                    if text.chars().all(char::is_control) {
                        return false;
                    }
                    self.library.state.insert_text(text);
                    Vec::new()
                }
            };
            self.apply_library(actions);
            return true;
        }
        let shift = self.modifiers.shift_key();
        let actions = match event.key_without_modifiers() {
            Key::Named(NamedKey::ArrowUp) => {
                self.library.state.move_cursor(-1, shift);
                Vec::new()
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.library.state.move_cursor(1, shift);
                self.library.maybe_more()
            }
            Key::Named(NamedKey::Enter) if !event.repeat => {
                match self.library.state.first_selected() {
                    Some(row) => self.library.activate_row(row),
                    None => return false,
                }
            }
            _ => return false,
        };
        self.apply_library(actions);
        true
    }

    fn lib_canvas(&self) -> Canvas {
        let (w, h) = gen::window_size(self.library.state.size_steps);
        let mut canvas = Canvas::new(w, h);
        render_library(&self.skin, &self.library.state, &mut canvas);
        canvas
    }

    fn redraw_lib(&mut self) {
        if !self.lib_visible {
            return;
        }
        let canvas = self.lib_canvas();
        if let Some(lib) = self.lib.as_mut() {
            lib.present(&canvas);
        }
    }

    fn lib_event(&mut self, event: WindowEvent) {
        match event {
            // Closing the library hides it, like the playlist.
            WindowEvent::CloseRequested => self.hide_library(),
            WindowEvent::RedrawRequested => self.redraw_lib(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.request_redraw()
            }
            WindowEvent::Focused(focused) => {
                self.library.state.focused = focused;
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(lib) = self.lib.as_mut() {
                    lib.set_cursor(Some(position));
                }
                if self.lib_drag.is_some() {
                    self.update_lib_drag();
                }
                self.drag_moved(Win::Library);
                self.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(lib) = self.lib.as_mut() {
                    lib.set_cursor(None);
                }
                self.request_redraw();
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => self.on_lib_press(),
                ElementState::Released => self.on_lib_release(),
            },
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => self.on_lib_right_press(),
            WindowEvent::MouseWheel { delta, .. } => self.lib_wheel(delta),
            WindowEvent::KeyboardInput { event, .. } => {
                if self.lib_key(&event) {
                    self.request_redraw();
                } else {
                    self.on_key(&event);
                }
            }
            _ => {}
        }
    }

    // Video window.

    /// The video frame's control under the pointer.
    fn video_hit_under_cursor(&self) -> Option<VideoHit> {
        let (x, y) = self.video.as_ref()?.skin_pixel()?;
        vw::hit_test_video(x, y, self.video_state.w, self.video_state.h)
    }

    /// The video window's zoom (1 or 2), or 1 before the window exists.
    fn video_zoom(&self) -> u32 {
        self.video.as_ref().map_or(1, |video| video.zoom())
    }

    /// Where the video window goes the first time it is shown: to the right of the main window,
    /// top aligned with it.
    fn video_default_origin(&self) -> Option<(f64, f64)> {
        let (x, y) = self.window_origin(Win::Main)?;
        let (main_w, _) = self.window_px_size(Win::Main);
        Some((x + main_w, y))
    }

    /// Where the library goes the first time it is shown: to the right of the video window, top
    /// aligned with it. The video window is where it is when placed, or where it goes otherwise.
    fn library_default_origin(&self) -> Option<(f64, f64)> {
        let (x, y) = if self.video_placed {
            self.window_origin(Win::Video)?
        } else {
            self.video_default_origin()?
        };
        let (video_w, _) = self.window_px_size(Win::Video);
        Some((x + video_w, y))
    }

    /// Where Milkdrop goes the first time it is shown: below the library window, left aligned with
    /// it. The library window is where it is when placed, or where it goes otherwise.
    fn milkdrop_default_origin(&self) -> Option<(f64, f64)> {
        let (x, y) = if self.lib_placed {
            self.window_origin(Win::Library)?
        } else {
            self.library_default_origin()?
        };
        let (_, lib_h) = self.window_px_size(Win::Library);
        Some((x, y + lib_h))
    }

    /// The default origin of a window with no saved place: the video window to the right of the
    /// main one, the library to the right of the video one, Milkdrop below the library. Each is
    /// moved onto the main window's monitor when it would be off it. `None` for the other windows
    /// and before the main window exists.
    fn default_origin(&self, win: Win) -> Option<(f64, f64)> {
        let (x, y) = match win {
            Win::Video => self.video_default_origin()?,
            Win::Library => self.library_default_origin()?,
            Win::Milkdrop => self.milkdrop_default_origin()?,
            Win::Main | Win::Eq | Win::Playlist => return None,
        };
        let screen = self.screen_rect();
        let monitor = config::Rect {
            x: screen.x.round() as i32,
            y: screen.y.round() as i32,
            w: screen.w.round() as i32,
            h: screen.h.round() as i32,
        };
        let (w, h) = self.window_px_size(win);
        let (x, y) = config::clamp_origin(
            (x.round() as i32, y.round() as i32),
            (w.round() as i32, h.round() as i32),
            &[monitor],
        );
        Some((f64::from(x), f64::from(y)))
    }

    /// Shows the video window. The first time, it goes to its default place (see `default_origin`).
    fn show_video(&mut self) {
        if self.video.is_none() {
            return;
        }
        if !self.video_placed {
            if let Some((x, y)) = self.default_origin(Win::Video) {
                self.set_window_origin(Win::Video, (x, y));
            }
            self.video_placed = true;
        }
        if let Some(video) = self.video.as_ref() {
            video.window.set_visible(true);
        }
        self.video_visible = true;
        self.update_video_bounds();
        self.request_redraw();
    }

    /// Hides the video window. The page keeps playing: the window is only hidden, and it is
    /// kept out of fullscreen first, so a hidden window never stays fullscreen.
    fn hide_video(&mut self) {
        if self.video_fullscreen {
            self.toggle_video_fullscreen();
        }
        self.video_visible = false;
        self.video_press = None;
        self.video_resize = None;
        self.video_state.pressed = None;
        if let Some(video) = self.video.as_ref() {
            video.window.set_visible(false);
        }
        self.request_redraw();
    }

    fn toggle_video(&mut self) {
        if self.video_visible {
            self.hide_video();
        } else {
            self.show_video();
        }
    }

    /// Sets the video frame's size in skin pixels. The window and the webview follow.
    fn set_video_size(&mut self, size: (u32, u32)) {
        let (w, h) = vw::clamp_size(size.0 as i32, size.1 as i32);
        self.video_state.w = w;
        self.video_state.h = h;
        if let Some(video) = self.video.as_mut() {
            video.set_base((w, h));
        }
        self.update_video_bounds();
        self.request_redraw();
    }

    /// Puts the webview where the frame's content area is, in logical pixels of the video window.
    /// In fullscreen it covers the whole screen instead.
    fn update_video_bounds(&self) {
        let (Some(bridge), Some(video)) = (self.bridge.as_ref(), self.video.as_ref()) else {
            return;
        };
        let (x, y, w, h) = if self.video_fullscreen {
            let size = video
                .window
                .inner_size()
                .to_logical::<f64>(video.window.scale_factor());
            (0.0, 0.0, size.width, size.height)
        } else {
            let zoom = f64::from(video.zoom());
            let r = vw::content_rect(self.video_state.w, self.video_state.h);
            (
                f64::from(r.x) * zoom,
                f64::from(r.y) * zoom,
                f64::from(r.w) * zoom,
                f64::from(r.h) * zoom,
            )
        };
        if let Err(e) = bridge.set_bounds(x, y, w, h) {
            eprintln!("tubeamp: could not place the YouTube Music view: {e}");
        }
        // The video view keeps the page's CSS width at most VIDEO_CSS_WIDTH, by zooming the page
        // in when the view is wider (see `tubeamp_bridge::video_page_zoom`). The browser view
        // shows the page at its normal size.
        let page_zoom = if self.video_browser {
            1.0
        } else {
            tubeamp_bridge::video_page_zoom(w)
        };
        if let Err(e) = bridge.set_page_zoom(page_zoom) {
            eprintln!("tubeamp: could not zoom the YouTube Music view: {e}");
        }
    }

    /// Fullscreen for the video window (borderless). Esc leaves it.
    fn toggle_video_fullscreen(&mut self) {
        let Some(video) = self.video.as_mut() else {
            return;
        };
        self.video_fullscreen = !self.video_fullscreen;
        video.set_fullscreen(self.video_fullscreen);
        self.update_video_bounds();
        self.request_redraw();
    }

    /// Switches the page between the video view and the browser view, and tells the page. The
    /// browser view makes the window at least 800 × 600 on screen, and leaving it restores the
    /// size it had.
    fn set_video_browser(&mut self, browser: bool) {
        let was_browser = self.video_browser;
        // Set first: the size changes below place the page, and its zoom depends on the view.
        self.video_browser = browser;
        if browser && !was_browser {
            self.video_saved_size = Some((self.video_state.w, self.video_state.h));
            let zoom = self.video_zoom();
            let min_w = BROWSER_MIN.0.div_ceil(zoom);
            let min_h = BROWSER_MIN.1.div_ceil(zoom);
            let size = (self.video_state.w.max(min_w), self.video_state.h.max(min_h));
            self.set_video_size(size);
        } else if !browser && was_browser {
            if let Some(size) = self.video_saved_size.take() {
                self.set_video_size(size);
            }
        }
        self.update_video_bounds();
        self.send(Cmd::SetViewMode { video: !browser });
    }

    /// Runs the bottom button `index` of the video frame.
    fn video_button(&mut self, index: usize) {
        match index {
            0 => self.toggle_video_fullscreen(),
            1 => {
                let (w, h) = vw::window_for_content(VIDEO_1X.0, VIDEO_1X.1);
                self.set_video_size((w, h));
            }
            2 => {
                let (w, h) = vw::window_for_content(VIDEO_2X.0, VIDEO_2X.1);
                self.set_video_size((w, h));
            }
            // The TV/misc button has no action yet.
            3 => {}
            _ => {
                let browser = !self.video_browser;
                self.set_video_browser(browser);
            }
        }
    }

    fn on_video_press(&mut self) {
        let hit = self.video_hit_under_cursor();
        self.video_press = hit;
        self.video_state.pressed = match hit {
            Some(VideoHit::Close | VideoHit::Button(_)) => hit,
            _ => None,
        };
        match hit {
            Some(VideoHit::TitleBar) => self.begin_window_drag(Win::Video),
            Some(VideoHit::Resize) => {
                let start = (self.video_state.w, self.video_state.h);
                if let Some(cursor) = self.video.as_ref().and_then(|v| v.cursor_position()) {
                    self.video_resize = Some(VideoResize {
                        start,
                        start_cursor: cursor,
                    });
                }
            }
            _ => {}
        }
        self.request_redraw();
    }

    fn on_video_release(&mut self) {
        self.end_window_drag();
        let target = self.video_press.take();
        let hit = self.video_hit_under_cursor();
        self.video_state.pressed = None;
        self.video_resize = None;
        if let Some(target) = target {
            if Some(target) == hit {
                self.activate_video(target);
            }
        }
        self.request_redraw();
    }

    /// Runs the action of a control pressed and released on the same spot.
    fn activate_video(&mut self, hit: VideoHit) {
        match hit {
            VideoHit::Close => self.hide_video(),
            VideoHit::Button(index) => self.video_button(index),
            _ => {}
        }
    }

    /// Moves the video window's size to the pointer while its corner is dragged. One skin pixel
    /// per step, as the playlist's resize does, and never below the frame's minimum.
    fn update_video_resize(&mut self) {
        let Some(drag) = self.video_resize else {
            return;
        };
        let Some(video) = self.video.as_ref() else {
            return;
        };
        let Some(cursor) = video.cursor_position() else {
            return;
        };
        let factor = f64::from(video.factor());
        let w = f64::from(drag.start.0) + (cursor.x - drag.start_cursor.x) / factor;
        let h = f64::from(drag.start.1) + (cursor.y - drag.start_cursor.y) / factor;
        let (w, h) = self.snapped_skin_size(Win::Video, w, h);
        let size = vw::clamp_size(w.round() as i32, h.round() as i32);
        if size != (self.video_state.w, self.video_state.h) {
            self.set_video_size(size);
        }
    }

    /// Copies the playing track into the video frame's info line.
    fn refresh_video_info(&mut self) {
        self.video_state.info_text =
            labels::video_info_text(&self.playback.author, &self.playback.title);
    }

    fn video_canvas(&self) -> Canvas {
        let mut canvas = Canvas::new(self.video_state.w, self.video_state.h);
        vw::render_video_frame(&self.skin, &self.video_state, &mut canvas);
        canvas
    }

    fn redraw_video(&mut self) {
        if !self.video_visible {
            return;
        }
        let canvas = self.video_canvas();
        if let Some(video) = self.video.as_mut() {
            video.present(&canvas);
        }
    }

    fn video_event(&mut self, event: WindowEvent) {
        match event {
            // Closing the video window hides it. The playback keeps going.
            WindowEvent::CloseRequested => self.hide_video(),
            WindowEvent::RedrawRequested => self.redraw_video(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.update_video_bounds();
                self.request_redraw();
            }
            WindowEvent::Focused(focused) => {
                self.video_state.focused = focused;
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(video) = self.video.as_mut() {
                    video.set_cursor(Some(position));
                }
                if self.video_resize.is_some() {
                    self.update_video_resize();
                }
                self.drag_moved(Win::Video);
                self.request_redraw();
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(video) = self.video.as_mut() {
                    video.set_cursor(None);
                }
                self.request_redraw();
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => self.on_video_press(),
                ElementState::Released => self.on_video_release(),
            },
            WindowEvent::KeyboardInput { event, .. } => self.on_key(&event),
            _ => {}
        }
    }

    // Milkdrop window (the logic is in `milkdrop.rs`; these carry out what it asks of the app).

    /// Shows the Milkdrop window. The first time, it goes to its default place (see `default_origin`).
    fn show_milkdrop(&mut self) {
        let beside = self
            .default_origin(Win::Milkdrop)
            .map(|(x, y)| (x.round() as i32, y.round() as i32));
        self.milkdrop.show(beside);
        self.request_redraw();
    }

    fn toggle_milkdrop(&mut self) {
        if self.milkdrop.visible() {
            self.milkdrop.hide();
        } else {
            self.show_milkdrop();
        }
    }

    /// A message from the Milkdrop page. Its keys that the page does not use run as page keys,
    /// so the global shortcuts work from the Milkdrop window too.
    fn on_milkdrop_msg(&mut self, msg: MilkdropMsg) {
        match msg {
            MilkdropMsg::Key {
                key,
                code,
                ctrl,
                alt,
                meta,
            } => {
                // The Milkdrop page has no repeat flag, so its keys count as presses.
                let shown = self.milkdrop.visible();
                let mods = PageMods {
                    ctrl,
                    alt,
                    meta,
                };
                self.on_page_key(&key, &code, mods, false, shown);
            }
            other => self.milkdrop.on_msg(other),
        }
    }

    /// A window event of the Milkdrop window. The move it asks for is carried out here, because
    /// the other windows are the app's.
    fn milkdrop_event(&mut self, event: WindowEvent) {
        if matches!(event, WindowEvent::RedrawRequested) {
            self.milkdrop.redraw(&self.skin);
            return;
        }
        match self.milkdrop.event(event) {
            Some(MilkdropEffect::BeginDrag) => self.begin_window_drag(Win::Milkdrop),
            Some(MilkdropEffect::DragMoved) => self.drag_moved(Win::Milkdrop),
            Some(MilkdropEffect::EndDrag) => self.end_window_drag(),
            Some(MilkdropEffect::Resize(w, h)) => {
                let (w, h) = self.snapped_skin_size(Win::Milkdrop, w, h);
                let steps = gen::steps_for_size(w.round() as i32, h.round() as i32);
                self.milkdrop.set_steps(steps);
            }
            None => {}
        }
    }

    // Equalizer window.

    fn toggle_eq(&mut self) {
        if self.eq_visible {
            self.hide_eq();
        } else {
            self.show_eq();
        }
    }

    /// Shows the equalizer directly below the main window, at the main window's left edge. Its
    /// place after that is whatever the user (or the snapping) left it at.
    fn show_eq(&mut self) {
        if let Some((x, y)) = self.window_origin(Win::Main) {
            let (_, height) = self.window_px_size(Win::Main);
            self.move_eq(PhysicalPosition::new(x as i32, (y + height) as i32));
        }
        let Some(eq) = self.eq.as_ref() else {
            return;
        };
        eq.window.set_visible(true);
        self.eq_visible = true;
        self.state.eq_open = true;
        self.request_redraw();
    }

    fn hide_eq(&mut self) {
        self.eq_visible = false;
        self.state.eq_open = false;
        self.eq_press = None;
        self.eq_drag = None;
        self.eq_shade_drag = None;
        self.eq_state.pressed = None;
        self.eq_state.dragging = None;
        if let Some(eq) = self.eq.as_ref() {
            eq.window.set_visible(false);
        }
        self.request_redraw();
    }

    fn move_eq(&self, pos: PhysicalPosition<i32>) {
        if let Some(eq) = self.eq.as_ref() {
            eq.window.set_outer_position(pos);
        }
    }

    fn on_eq_press(&mut self) {
        let hit = self.eq_hit_under_cursor();
        self.eq_press = hit;
        self.eq_state.pressed = hit;
        match hit {
            Some(EqHit::TitleBar) => self.title_press(Win::Eq),
            Some(EqHit::ShadeVolume) => {
                self.eq_shade_drag = Some(ShadeSlider::Volume);
                self.update_eq_shade_drag();
            }
            Some(EqHit::ShadeBalance) => {
                self.eq_shade_drag = Some(ShadeSlider::Balance);
                self.update_eq_shade_drag();
            }
            Some(EqHit::Slider(slider)) => {
                if let Some((x, y)) = self.eq.as_ref().and_then(|e| e.skin_point()) {
                    let grab = press_grab(slider, &self.eq_state.settings, x, y);
                    self.eq_drag = Some(EqDrag { slider, grab });
                    self.eq_state.dragging = Some(slider);
                    self.update_eq_drag();
                }
            }
            _ => {}
        }
        self.request_redraw();
    }

    fn on_eq_release(&mut self) {
        self.end_window_drag();
        self.eq_shade_drag = None;
        let target = self.eq_press.take();
        let hit = self.eq_hit_under_cursor();
        self.eq_state.pressed = None;
        self.eq_drag = None;
        self.eq_state.dragging = None;
        if let Some(target) = target {
            if Some(target) == hit {
                self.activate_eq(target);
            }
        }
        self.request_redraw();
    }

    fn activate_eq(&mut self, hit: EqHit) {
        match hit {
            EqHit::Close => self.hide_eq(),
            EqHit::Shade => self.toggle_eq_shade(),
            EqHit::On => self.toggle_power(),
            EqHit::Auto => self.toggle_auto(),
            EqHit::Presets => self.open_presets_menu(),
            EqHit::TitleBar | EqHit::Slider(_) | EqHit::ShadeVolume | EqHit::ShadeBalance => {}
        }
    }

    /// Moves the shade's volume or balance to the pointer. The value goes to the main window's
    /// state, and to the player or the audio like the main window's own sliders.
    fn update_eq_shade_drag(&mut self) {
        let Some(drag) = self.eq_shade_drag else {
            return;
        };
        let Some((x, _)) = self.eq.as_ref().and_then(|e| e.skin_point()) else {
            return;
        };
        match drag {
            ShadeSlider::Volume => self.set_volume(eqw::shade_volume_from_x(x)),
            ShadeSlider::Balance => self.set_balance(eqw::shade_balance_from_x(x)),
        }
        self.request_redraw();
    }

    /// Moves the slider being dragged to the pointer. The value goes straight to the audio.
    fn update_eq_drag(&mut self) {
        let Some(drag) = self.eq_drag else {
            return;
        };
        let Some((_, y)) = self.eq.as_ref().and_then(|e| e.skin_point()) else {
            return;
        };
        let value = slider_value_at(drag.slider, y, drag.grab);
        self.set_slider(drag.slider, value);
        self.request_redraw();
    }

    fn set_slider(&mut self, slider: EqSlider, value: u8) {
        let settings = &mut self.eq_state.settings;
        match slider {
            EqSlider::Preamp => {
                settings.preamp = value;
                eq_params().set_preamp(value);
            }
            EqSlider::Band(i) => {
                settings.bands[i] = value;
                eq_params().set_band(i, value);
            }
        }
    }

    /// The main window's marquee while an equalizer slider is dragged (Webamp's `EQ:` text).
    fn eq_marquee(&self, slider: EqSlider) -> String {
        let settings = &self.eq_state.settings;
        match slider {
            EqSlider::Preamp => labels::eq_preamp_text(settings.preamp_db()),
            EqSlider::Band(i) => labels::eq_band_text(BANDS_HZ[i] as u32, settings.band_db(i)),
        }
    }

    /// ON: the bypass switch. The audio tap applies the curve only while this is on. Turning it
    /// on also asks for the audio tap again, if a silent tap was switched off.
    fn toggle_power(&mut self) {
        let on = !self.eq_state.settings.on;
        self.eq_state.settings.on = on;
        eq_params().set_on(on);
        if on {
            self.retry_audio_capture(Instant::now());
        }
        self.request_redraw();
    }

    /// The tap was silent while the page played, so it is torn down. Dropping the engine stops the
    /// device, and YouTube Music's own output plays again (the EQ is bypassed). The marquee says
    /// what to allow. No automatic retry follows: see `retry_audio_capture`.
    #[cfg_attr(not(target_os = "macos"), allow(dead_code))]
    fn audio_tap_silent(&mut self) {
        eprintln!(
            "tubeamp: the audio tap is silent while YouTube Music plays; tearing it down (EQ off). \
             Allow System Audio Recording for TubeAmp, then turn the EQ on or use Retry audio capture."
        );
        self.capture.deny();
        self.playing_since = None;
        self.show_message(AUDIO_OFF_NOTICE.to_string(), Some(AUDIO_OFF_SHOWN));
        self.request_redraw();
    }

    /// Asks for the audio tap again: from the EQ's ON switch or the O menu's Retry audio capture.
    /// The tap starts on the next tick while the page plays. A tap that is still silent is torn
    /// down again after the window.
    fn retry_audio_capture(&mut self, now: Instant) {
        if self.capture.engine().is_some() {
            return;
        }
        eprintln!("tubeamp: audio capture retry requested");
        self.capture.retry();
        self.audio_error_shown = false;
        self.audio_retry_at = now;
        self.request_redraw();
    }

    /// AUTO only stores the flag for now. TODO: per-track auto presets are a later feature.
    fn toggle_auto(&mut self) {
        self.eq_state.settings.auto = !self.eq_state.settings.auto;
        self.request_redraw();
    }

    /// Shows the PRESETS menu at the button. Its choices come back as `UserEvent::Menu`.
    fn open_presets_menu(&mut self) {
        let Some(eq) = self.eq.as_ref() else {
            return;
        };
        let zoom = eq.zoom() as i32;
        let x = f64::from(PRESETS_BTN.x * zoom);
        let y = f64::from((PRESETS_BTN.y + PRESETS_BTN.h) * zoom);
        if let Err(e) = eq::show_presets_menu(&eq.window, x, y) {
            eprintln!("tubeamp: could not show the PRESETS menu: {e}");
        }
        self.request_redraw();
    }

    fn run_eq_command(&mut self, cmd: EqCommand) {
        match cmd {
            EqCommand::Builtin(i) => {
                let preset = BUILTIN[i];
                self.apply_curve(preset.preamp, preset.bands);
            }
            EqCommand::Reset => self.apply_curve(ZERO_VALUE, [ZERO_VALUE; 10]),
            EqCommand::LoadEqf => self.load_eqf(),
            EqCommand::SaveEqf => self.save_eqf(),
        }
        self.request_redraw();
    }

    /// Replaces the preamp and bands, and pushes them to the audio. ON and AUTO stay as they are.
    fn apply_curve(&mut self, preamp: u8, bands: [u8; 10]) {
        self.eq_state.settings.preamp = preamp;
        self.eq_state.settings.bands = bands;
        self.apply_eq_settings();
    }

    fn apply_eq_settings(&self) {
        eq_params().set(&self.eq_state.settings);
    }

    /// Sends the EQ to the page's Web Audio graph when it has changed (not macOS, where the tap
    /// does the EQ). Every change goes through `eq_params()`, which counts its changes, so this one
    /// check covers them all. `Evt::Ready` clears the count, so a reloaded page gets the EQ again.
    #[cfg(not(target_os = "macos"))]
    fn sync_page_eq(&mut self) {
        let p = eq_params();
        let v = p.version();
        if Some(v) == self.page_eq_version {
            return;
        }
        let s = p.get();
        self.send(Cmd::Eq {
            on: s.on,
            preamp_db: s.preamp_db(),
            bands_db: std::array::from_fn(|i| s.band_db(i)),
            balance: p.balance(),
        });
        self.page_eq_version = Some(v);
    }

    fn load_eqf(&mut self) {
        let Some(path) = eq::pick_eqf_to_load() else {
            return;
        };
        let loaded = std::fs::read(&path)
            .map_err(|e| format!("could not read {}: {e}", path.display()))
            .and_then(|bytes| {
                eq::first_entry(&bytes).map_err(|e| format!("{}: {e}", path.display()))
            });
        match loaded {
            Ok(entry) => {
                self.apply_curve(entry.preamp, entry.bands);
                eprintln!(
                    "tubeamp: loaded EQ preset \"{}\" from {}",
                    entry.name,
                    path.display()
                );
            }
            Err(msg) => eprintln!("tubeamp: {msg}"),
        }
    }

    fn save_eqf(&mut self) {
        let Some(path) = eq::pick_eqf_to_save() else {
            return;
        };
        let bytes = eq::eqf_bytes(&eq::entry_name(&path), &self.eq_state.settings);
        match std::fs::write(&path, bytes) {
            Ok(()) => eprintln!("tubeamp: saved EQ preset to {}", path.display()),
            Err(e) => eprintln!("tubeamp: could not write {}: {e}", path.display()),
        }
    }

    fn on_bridge_event(&mut self, evt: Evt) {
        // Samples arrive about 47 times a second: they are not logged.
        if !matches!(evt, Evt::Samples { .. }) {
            self.log(format!("event {evt:?}"));
        }
        match &evt {
            Evt::State { .. } => {
                let now = Instant::now();
                if let Some(report) = self.state_rate.record(now) {
                    self.log(format!(
                        "bridge: {report}; video window visible {}",
                        self.video_visible
                    ));
                }
                let before = self.playback.clone();
                self.playback.apply(&evt, now);
                self.library.set_playing(self.playback.video_id.as_deref());
                self.log_transitions(&before);
                // Once the player exists: the slider's volume, and the track restored at startup.
                if !self.volume_synced && self.playback.ps.is_some() {
                    self.volume_synced = true;
                    self.on_player_ready();
                }
                self.follow_playlist();
            }
            Evt::Ended => {
                self.log("track ended".to_string());
                self.on_track_ended();
            }
            Evt::Error { at, msg } => eprintln!("tubeamp: YouTube Music error at {at}: {msg}"),
            // Keys the page forwards in the video view run the same shortcuts as native keys.
            Evt::Key {
                key,
                code,
                ctrl,
                alt,
                meta,
                repeat,
                ..
            } => self.on_page_key(
                key,
                code,
                PageMods {
                    ctrl: *ctrl,
                    alt: *alt,
                    meta: *meta,
                },
                *repeat,
                self.video_visible,
            ),
            // Escape reaches us from the page when the webview has focus. It leaves fullscreen.
            Evt::Escape => {
                if self.video_fullscreen {
                    self.toggle_video_fullscreen();
                }
            }
            // A media key, or a Now Playing control, from the page's media session (macOS).
            Evt::Media { action, seek_time } => {
                if let Some(action) = crate::nowplaying::media_action(action, *seek_time) {
                    self.on_media_action(action);
                }
            }
            // Post-EQ samples from the page's Web Audio graph. macOS gets its samples from the tap.
            #[cfg(not(target_os = "macos"))]
            Evt::Samples { rate, data } => tubeamp_audio::feed_page_samples(*rate, data),
            #[cfg(target_os = "macos")]
            Evt::Samples { .. } => {}
            Evt::Ready { signed_in } => {
                // A page that has just loaded starts in the video view, so tell it the view we
                // are in (the page was loaded from scratch, or the view was changed earlier).
                self.send(Cmd::SetViewMode {
                    video: !self.video_browser,
                });
                // A reloaded page has lost its EQ graph, so the EQ is sent again (not macOS).
                #[cfg(not(target_os = "macos"))]
                {
                    self.page_eq_version = None;
                }
                // The page's media session is new: the track goes to it again.
                self.forget_now_playing();
                let actions = self.library.on_ready(*signed_in);
                self.apply_library(actions);
            }
            Evt::ApiResult {
                req,
                ok,
                status,
                body,
            } => {
                let actions = self.library.on_result(*req, *ok, *status, body);
                self.apply_library(actions);
            }
        }
        // Samples only feed the analyser, which the frame timer redraws.
        if !matches!(evt, Evt::Samples { .. }) {
            self.request_redraw();
        }
    }

    /// The current entry's video is the one that ended: move the playlist on.
    fn on_track_ended(&mut self) {
        let Some(current) = self.playlist.current().and_then(|i| self.playlist.get(i)) else {
            return;
        };
        if self.playback.video_id.as_deref() == Some(current.video_id.as_str()) {
            self.advance_after_end();
        }
    }

    /// Keeps the playlist in step with the player. While the current entry's video is the one
    /// reported, its missing metadata is filled in, and its end moves the playlist on. A report
    /// for some other video (YouTube Music autoplaying, say) is ignored.
    fn follow_playlist(&mut self) {
        let Some(current) = self.playlist.current() else {
            return;
        };
        let Some(video_id) = self.playlist.get(current).map(|e| e.video_id.clone()) else {
            return;
        };
        if self.playback.video_id.as_deref() != Some(video_id.as_str()) {
            return;
        }
        self.fill_metadata(current);
        match self.playback.ps {
            Some(0) => self.advance_after_end(),
            Some(1..=3) => self.ended_done = false,
            _ => {}
        }
    }

    /// Copies title, artist and length from the player into entry `index`, where the entry has
    /// none yet. A title that is only the video id counts as none.
    fn fill_metadata(&mut self, index: usize) {
        let title = self.playback.title.trim().to_string();
        let author = self.playback.author.trim().to_string();
        let duration = self.playback.duration_secs;
        let Some(entry) = self.playlist.get_mut(index) else {
            return;
        };
        if !title.is_empty() && (entry.title.trim().is_empty() || entry.title == entry.video_id) {
            entry.title = title;
        }
        if entry.artist.trim().is_empty() && !author.is_empty() {
            entry.artist = author;
        }
        if entry.duration_secs.is_none() && duration > 0 {
            entry.duration_secs = Some(duration);
        }
    }

    /// Moves the playlist on from the current entry, once per entry. The playlist picks the next
    /// entry (shuffle and repeat included), and nothing plays when the list is finished.
    fn advance_after_end(&mut self) {
        if self.ended_done || self.playlist.current().is_none() {
            return;
        }
        self.ended_done = true;
        match self
            .playlist
            .next(self.state.shuffle, self.state.repeat, &mut self.rng)
        {
            Some(index) => self.play_index(index),
            None => self.log("playlist: end of the list".to_string()),
        }
    }

    /// Loads playlist entry `index` and makes it current.
    fn play_index(&mut self, index: usize) {
        let Some(video_id) = self.playlist.get(index).map(|e| e.video_id.clone()) else {
            return;
        };
        self.playlist.set_current(Some(index));
        self.show_row(index);
        self.log(format!("playlist: play {}", index + 1));
        self.send(Cmd::Load { video_id });
        self.request_redraw();
    }

    /// The main window's Next button and the B key. Without a playlist it is YouTube Music's own.
    fn next_track(&mut self) {
        if self.playlist.is_empty() {
            self.send(Cmd::Next);
            return;
        }
        match self
            .playlist
            .next(self.state.shuffle, self.state.repeat, &mut self.rng)
        {
            Some(index) => self.play_index(index),
            None => self.send(Cmd::Stop),
        }
    }

    /// The main window's Previous button and the Z key. As Winamp does, more than three seconds
    /// into a track rewinds it; at the start of the list the track restarts.
    fn prev_track(&mut self) {
        if self.playlist.is_empty() {
            self.send(Cmd::Prev);
            return;
        }
        if self.playback.time_secs > 3 {
            self.send(Cmd::Seek { secs: 0.0 });
            return;
        }
        match self
            .playlist
            .prev(self.state.shuffle, self.state.repeat, &mut self.rng)
        {
            Some(index) => self.play_index(index),
            None => self.send(Cmd::Seek { secs: 0.0 }),
        }
    }

    fn log_transitions(&self, before: &Playback) {
        let now = &self.playback;
        if before.status != now.status {
            self.log(format!("status {:?} -> {:?}", before.status, now.status));
        }
        if before.video_id != now.video_id {
            self.log(format!(
                "video {:?} \"{}\" by \"{}\"",
                now.video_id, now.title, now.author
            ));
        }
        if before.codec != now.codec {
            self.log(format!("codec {:?}", now.codec));
        }
    }

    /// The main marquee text. A track that is in the playlist shows its playlist number.
    fn marquee_text(&self) -> String {
        let number = self
            .playlist
            .current()
            .filter(|&i| {
                self.playlist
                    .get(i)
                    .is_some_and(|e| self.playback.video_id.as_deref() == Some(e.video_id.as_str()))
            })
            .map_or(labels::TRACK_NUMBER, |i| (i + 1) as u32);
        self.playback.marquee_text_numbered(number)
    }

    /// Copies playback and drag state into the main window's state.
    fn refresh_display(&mut self, now: Instant) {
        self.state.status = self.display_status();
        let p = &self.playback;
        let time = p.time_at(now);
        self.state.time_secs = time;
        self.state.duration_secs = p.duration_secs;
        self.state.kbps = p.kbps_for_display().or(self.state.kbps);
        self.state.khz = Some(
            self.capture
                .engine()
                .map_or(FALLBACK_KHZ, |engine| engine.sample_rate() / 1000),
        );
        // The bridge does not report the channel count, so a playing track is treated as stereo.
        self.state.stereo = p.status == Status::Play;
        if !matches!(self.drag, Some(Drag::Position { .. })) {
            self.state.position = if p.duration_secs > 0 {
                time as f32 / p.duration_secs as f32
            } else {
                0.0
            };
        }
        self.state.title = if let Some(drag) = self.eq_drag {
            self.eq_marquee(drag.slider)
        } else {
            match self.drag {
                Some(Drag::Position { percent }) if p.duration_secs > 0 => {
                    labels::seek_text(p.duration_secs, percent)
                }
                Some(Drag::Volume) => labels::volume_text(self.state.volume),
                Some(Drag::Balance) => labels::balance_text(self.state.balance),
                _ => self.marquee_text(),
            }
        };
        // A skin error or a download note takes the marquee for a few seconds.
        if let Some(text) = self.active_message(now) {
            self.state.title = text;
        }
        // The playlist's mini time follows the main window's time mode while playing or paused.
        self.pl_state.mini_time_secs =
            (self.state.status != Status::Stop).then(|| displayed_secs(&self.state));
        self.pl_state.mini_remaining = self.state.remaining_mode;
        self.eq_state.volume = self.state.volume;
        self.eq_state.balance = self.state.balance;
        self.refresh_video_info();
    }

    /// Asks for the tap to start. The start runs on a worker thread (it can wait on the TCC
    /// prompt), so this returns at once. `poll_audio` picks up the result.
    fn start_audio(&mut self, now: Instant) {
        if self.capture.request_start(now, AudioEngine::start) && self.opts.log {
            eprintln!("tubeamp: audio tap start requested (worker thread)");
        }
    }

    /// Takes the result of a start that has finished, and shows the starting note when a start
    /// takes long. Never blocks.
    fn poll_audio(&mut self, now: Instant) {
        match self.capture.poll() {
            Some(CaptureEvent::Started) => {
                self.clear_starting_note();
                if let Some(engine) = self.capture.engine() {
                    eprintln!("tubeamp: audio tap running at {} Hz", engine.sample_rate());
                }
            }
            Some(CaptureEvent::Failed(AudioError::Unsupported)) => {
                self.clear_starting_note();
                eprintln!("tubeamp: {}; the EQ and visualiser audio are off", AudioError::Unsupported);
                self.audio_given_up = true;
            }
            Some(CaptureEvent::Failed(e)) => {
                self.clear_starting_note();
                if !self.audio_error_shown || self.opts.log {
                    eprintln!("tubeamp: audio tap not ready ({e}); retrying");
                }
                self.audio_error_shown = true;
                self.audio_retry_at = now + AUDIO_RETRY;
            }
            None => {
                let slow = self
                    .capture
                    .starting_for(now)
                    .is_some_and(|d| d >= AUDIO_STARTING_AFTER);
                if slow && !self.audio_note_shown {
                    self.audio_note_shown = true;
                    self.show_message(AUDIO_STARTING_NOTICE.to_string(), None);
                }
            }
        }
    }

    /// Removes the starting note from the marquee, if it is the one showing.
    fn clear_starting_note(&mut self) {
        if self.audio_note_shown {
            self.audio_note_shown = false;
            self.clear_message_if(AUDIO_STARTING_NOTICE);
        }
    }

    /// Feeds the analyser with the samples since the last frame, and advances it one step.
    fn step_vis(&mut self) {
        match self.capture.engine_mut() {
            Some(engine) => {
                engine.drain_samples(&mut self.samples);
                self.vis_frame = self.vis.update(&self.samples).clone();
            }
            None => self.vis_frame = self.vis.update(&[]).clone(),
        }
    }

    /// The main window as it looks now, at skin size.
    fn main_canvas(&self) -> Canvas {
        let mut canvas = Canvas::new(WIDTH, mw::window_height(self.state.shade));
        render_main(&self.skin, &self.state, &mut canvas);
        // The shade has no visualiser (Webamp's shade draws only a small one, not yet done).
        if !self.state.shade {
            render_vis(&self.skin, &self.state, &self.vis_frame, &mut canvas);
        }
        canvas
    }

    /// The equalizer window as it looks now, at skin size.
    fn eq_canvas(&self) -> Canvas {
        let mut canvas = Canvas::new(eqw::WIDTH, eqw::window_height(self.eq_state.shade));
        render_eq(&self.skin, &self.eq_state, &mut canvas);
        canvas
    }

    /// The playlist window as it looks now, at its current size.
    fn pl_canvas(&self) -> Canvas {
        let (w, h) = plw::window_size_for(self.pl_state.size_steps, self.pl_state.shade);
        let mut canvas = Canvas::new(w, h);
        render_playlist(&self.skin, &self.playlist, &self.pl_state, &mut canvas);
        canvas
    }

    fn redraw_main(&mut self) {
        let canvas = self.main_canvas();
        if let Some(main) = self.main.as_mut() {
            main.present(&canvas);
        }
    }

    fn redraw_eq(&mut self) {
        if !self.eq_visible {
            return;
        }
        let canvas = self.eq_canvas();
        if let Some(eq) = self.eq.as_mut() {
            eq.present(&canvas);
        }
    }

    fn redraw_pl(&mut self) {
        if !self.pl_visible {
            return;
        }
        let canvas = self.pl_canvas();
        if let Some(pl) = self.pl.as_mut() {
            pl.present(&canvas);
        }
    }

    fn save_screenshot(&self) {
        let Some((_, path)) = &self.opts.screenshot else {
            return;
        };
        let main = self.main_canvas();
        save_one(&scaled(&main, 2), main.w * 2, main.h * 2, path);
        if self.eq_visible {
            let eq_path = eq_screenshot_path(path);
            let eq = self.eq_canvas();
            save_one(&scaled(&eq, 2), eq.w * 2, eq.h * 2, &eq_path);
        }
        if self.pl_visible {
            let canvas = self.pl_canvas();
            save_one(
                &scaled(&canvas, 2),
                canvas.w * 2,
                canvas.h * 2,
                &suffixed_path(path, "pl"),
            );
        }
        if self.lib_visible {
            let canvas = self.lib_canvas();
            save_one(
                &scaled(&canvas, 2),
                canvas.w * 2,
                canvas.h * 2,
                &suffixed_path(path, "lib"),
            );
        }
        if self.video_visible {
            let canvas = self.video_canvas();
            save_one(
                &scaled(&canvas, 2),
                canvas.w * 2,
                canvas.h * 2,
                &suffixed_path(path, "video"),
            );
            // The real window, webview included. Needs the window number (macOS) and the screen
            // recording permission for the terminal.
            if let Some(number) = self
                .video
                .as_ref()
                .and_then(|v| skinned::window_number(&v.window))
            {
                let live = suffixed_path(path, "video-live");
                match skinned::capture_window(number, &live) {
                    Ok(()) => eprintln!("tubeamp: wrote {}", live.display()),
                    Err(e) => eprintln!("tubeamp: could not capture the video window: {e}"),
                }
            }
        }
        self.milkdrop.save_screenshots(&self.skin, path);
    }

    fn next_wake(&self, playing: bool) -> Instant {
        let mut wake = self.next_frame.min(self.next_marquee);
        if playing {
            wake = wake.min(self.next_vis);
        }
        if playing && self.capture.can_start() && !self.audio_given_up {
            wake = wake.min(self.audio_retry_at);
        }
        if let Some(t) = self.milkdrop.next_wake() {
            wake = wake.min(t);
        }
        for t in [
            self.video.as_ref().and_then(|v| v.restore_wake()),
            self.milkdrop.window().and_then(|w| w.restore_wake()),
        ]
        .into_iter()
        .flatten()
        {
            wake = wake.min(t);
        }
        for t in [self.autoload_at, self.screenshot_at, self.quit_at]
            .into_iter()
            .flatten()
        {
            wake = wake.min(t);
        }
        wake
    }

    /// Runs the timers, the audio retry and the frame: called from `about_to_wait`.
    fn tick(&mut self, el: &ActiveEventLoop) {
        let now = Instant::now();
        if self.autoload_at.is_some_and(|t| now >= t) {
            self.autoload_at = None;
            if let Some(id) = self.opts.autoload.clone() {
                self.send(Cmd::Load { video_id: id });
            }
        }
        if self.screenshot_at.is_some_and(|t| now >= t) {
            self.screenshot_at = None;
            self.save_screenshot();
        }
        if self.quit_at.is_some_and(|t| now >= t) {
            el.exit();
            return;
        }

        let playing = self.playback.status == Status::Play;
        self.playing_since = match (self.playback.ps == Some(1), self.playing_since) {
            (true, since) => since.or(Some(now)),
            (false, _) => None,
        };
        self.poll_audio(now);
        #[cfg(not(target_os = "macos"))]
        self.sync_page_eq();
        // The silence check is for macOS's System Audio Recording permission (see `guard`). Elsewhere
        // the page plays the audio, and there is no tap that could be denied.
        #[cfg(target_os = "macos")]
        {
            let suspect = self.capture.engine().is_some_and(|engine| {
                denial_suspected(
                    now,
                    engine.last_sound_at(),
                    self.playing_since,
                    self.state.volume > 0,
                    DENIAL_WINDOW,
                )
            });
            if suspect {
                self.audio_tap_silent();
            }
        }
        if playing && self.capture.can_start() && !self.audio_given_up && now >= self.audio_retry_at
        {
            self.start_audio(now);
        }
        self.milkdrop.tick(now, self.capture.engine());
        if let Some(video) = self.video.as_mut() {
            video.apply_due_restore(now);
        }
        if let Some(win) = self.milkdrop.window_mut() {
            win.apply_due_restore(now);
        }

        if playing && now >= self.next_vis {
            self.step_vis();
            self.next_vis = now + VIS_STEP;
        }

        if now >= self.next_frame {
            self.refresh_display(now);
            self.request_redraw();
            let active = playing || self.drag.is_some() || self.eq_drag.is_some();
            self.next_frame = now + if active { FRAME_ACTIVE } else { FRAME_IDLE };
        }

        if now >= self.next_marquee {
            let period = marquee_period_px(&self.state.title);
            if period > 0 {
                self.state.marquee_offset = (self.state.marquee_offset + MARQUEE_STEP_PX) % period;
            }
            self.next_marquee = now + MARQUEE_INTERVAL;
            self.request_redraw();
        }

        self.sync_system(now);
        self.save_config_if_due(now);
        el.set_control_flow(ControlFlow::WaitUntil(self.next_wake(playing)));
    }

    fn main_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw_main(),
            WindowEvent::Resized(_) => self.request_redraw(),
            // The windows only move when we move them (drags and `relayout`).
            WindowEvent::Moved(_) => {}
            WindowEvent::ScaleFactorChanged { .. } => self.request_redraw(),
            WindowEvent::Focused(focused) => {
                self.state.focused = focused;
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(main) = self.main.as_mut() {
                    main.set_cursor(Some(position));
                }
                if self.drag.is_some() {
                    self.update_drag();
                }
                self.drag_moved(Win::Main);
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(main) = self.main.as_mut() {
                    main.set_cursor(None);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => self.on_press(),
                ElementState::Released => self.on_release(event_loop),
            },
            // Right click: the menu with the skins (the same as the O button's).
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => self.open_main_context_menu(),
            WindowEvent::KeyboardInput { event, .. } => self.on_key(&event),
            _ => {}
        }
    }

    fn eq_event(&mut self, event: WindowEvent) {
        match event {
            // The equalizer has no native close button. Closing it hides it, like its title button.
            WindowEvent::CloseRequested => self.hide_eq(),
            WindowEvent::RedrawRequested => self.redraw_eq(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.request_redraw()
            }
            WindowEvent::Focused(focused) => {
                self.eq_state.focused = focused;
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(eq) = self.eq.as_mut() {
                    eq.set_cursor(Some(position));
                }
                if self.eq_drag.is_some() {
                    self.update_eq_drag();
                }
                if self.eq_shade_drag.is_some() {
                    self.update_eq_shade_drag();
                }
                self.drag_moved(Win::Eq);
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(eq) = self.eq.as_mut() {
                    eq.set_cursor(None);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => match state {
                ElementState::Pressed => self.on_eq_press(),
                ElementState::Released => self.on_eq_release(),
            },
            WindowEvent::KeyboardInput { event, .. } => self.on_key(&event),
            _ => {}
        }
    }
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.main.is_some() {
            return;
        }
        let zoom = self.opts.zoom;
        let main_attributes = WindowAttributes::default()
            .with_title("TubeAmp")
            .with_decorations(false)
            .with_resizable(false)
            .with_inner_size(LogicalSize::new(WIDTH * zoom, HEIGHT * zoom));
        let main = match SkinnedWindow::new(event_loop, main_attributes, (WIDTH, HEIGHT), zoom) {
            Ok(main) => main,
            Err(e) => {
                eprintln!("tubeamp: {e}");
                event_loop.exit();
                return;
            }
        };
        let (pl_w, pl_h) = plw::window_size(self.pl_state.size_steps);
        let pl_attributes = WindowAttributes::default()
            .with_title("TubeAmp Playlist")
            .with_decorations(false)
            // Not resizable by the OS: the corner handle resizes it in steps (see `PlDrag::Resize`).
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(pl_w * zoom, pl_h * zoom));
        let pl = match SkinnedWindow::new(event_loop, pl_attributes, (pl_w, pl_h), zoom) {
            Ok(pl) => pl,
            Err(e) => {
                eprintln!("tubeamp: {e}");
                event_loop.exit();
                return;
            }
        };
        let (lib_w, lib_h) = gen::window_size(self.library.state.size_steps);
        let lib_attributes = WindowAttributes::default()
            .with_title("TubeAmp Media Library")
            .with_decorations(false)
            // Not resizable by the OS: the corner grip resizes it in steps, as the playlist does.
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(lib_w * zoom, lib_h * zoom));
        let lib = match SkinnedWindow::new(event_loop, lib_attributes, (lib_w, lib_h), zoom) {
            Ok(lib) => lib,
            Err(e) => {
                eprintln!("tubeamp: {e}");
                event_loop.exit();
                return;
            }
        };
        let eq_attributes = WindowAttributes::default()
            .with_title("TubeAmp Equalizer")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(eqw::WIDTH * zoom, eqw::HEIGHT * zoom));
        let eq =
            match SkinnedWindow::new(event_loop, eq_attributes, (eqw::WIDTH, eqw::HEIGHT), zoom) {
                Ok(eq) => eq,
                Err(e) => {
                    eprintln!("tubeamp: {e}");
                    event_loop.exit();
                    return;
                }
            };

        // The video window is a skinned frame. The webview is its child, filling the content area.
        let (vid_w, vid_h) = (self.video_state.w, self.video_state.h);
        let video_attributes = WindowAttributes::default()
            .with_title("TubeAmp Video")
            .with_decorations(false)
            // Not resizable by the OS: the bottom-right corner resizes it, as the playlist does.
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(vid_w * zoom, vid_h * zoom));
        let video = match SkinnedWindow::new(event_loop, video_attributes, (vid_w, vid_h), zoom) {
            Ok(video) => video,
            Err(e) => {
                eprintln!("tubeamp: {e}");
                event_loop.exit();
                return;
            }
        };
        let proxy = self.proxy.clone();
        let bridge = match Bridge::new(&video.window, move |evt| {
            let _ = proxy.send_event(UserEvent::Bridge(evt));
        }) {
            Ok(bridge) => bridge,
            Err(e) => {
                eprintln!("tubeamp: could not create the YouTube Music view: {e}");
                event_loop.exit();
                return;
            }
        };
        self.log(format!(
            "video view: WebKit occlusion detection off = {}",
            bridge.occlusion_detection_off()
        ));
        // The Milkdrop window. Its webview is made when it first shows.
        let milkdrop_proxy = self.proxy.clone();
        self.milkdrop = match MilkdropWindow::create(event_loop, zoom, move |msg| {
            let _ = milkdrop_proxy.send_event(UserEvent::Milkdrop(msg));
        }) {
            Ok(milkdrop) => milkdrop,
            Err(e) => {
                eprintln!("tubeamp: {e}");
                event_loop.exit();
                return;
            }
        };

        self.main = Some(main);
        self.eq = Some(eq);
        self.video = Some(video);
        self.bridge = Some(bridge);
        self.pl = Some(pl);
        self.lib = Some(lib);
        // `--shade`: the windows start in windowshade mode, at their skin sizes. This comes before
        // anything is placed, because the placement uses the sizes.
        self.state.shade = self.opts.shade.main;
        self.eq_state.shade = self.opts.shade.eq;
        self.pl_state.shade = self.opts.shade.pl;
        // The saved settings come back here, before any window is shown (see `window/session.rs`).
        self.restore_session(event_loop);
        self.start_system_integration();
        self.apply_sizes();
        self.update_video_bounds();
        if self.opts.video_browser {
            self.set_video_browser(true);
        }

        // The audio starts from the flat curve, or from the preset named on the command line.
        self.apply_eq_settings();
        if let Some(preset) = self.opts.eq_preset {
            self.apply_curve(preset.preamp, preset.bands);
        }
        if self.opts.open_eq {
            self.show_eq();
        }
        for id in self.opts.add.clone() {
            self.playlist.add(Entry::from_id(&id));
        }
        if self.opts.open_pl {
            self.show_pl();
        }
        if self.opts.open_library {
            self.show_library();
        }
        if self.opts.open_video {
            self.show_video();
        }
        if self.opts.open_milkdrop {
            self.show_milkdrop();
        }
        self.request_redraw();
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Bridge(evt) => self.on_bridge_event(evt),
            UserEvent::Milkdrop(msg) => self.on_milkdrop_msg(msg),
            #[cfg(target_os = "windows")]
            UserEvent::Media(action) => self.on_media_action(action),
            UserEvent::Download(result) => self.on_download_done(result),
            UserEvent::FirstRun => self.first_run_prompt(),
            UserEvent::FirstRunAnswer(answer) => self.on_first_run_answer(answer),
            UserEvent::Menu(id) => {
                self.log(format!("menu item {id}"));
                if self.on_session_menu(event_loop, &id) {
                    return;
                }
                if let Some(cmd) = eq::command_for_id(&id) {
                    self.run_eq_command(cmd);
                } else if let Some(cmd) = pl_app::sort_command_for_id(&id) {
                    self.run_sort(cmd);
                } else if let Some(cmd) = library::command_for_id(&id) {
                    self.run_library_command(cmd);
                } else if id == library::MAIN_LIBRARY {
                    self.show_library();
                } else if id == library::MAIN_VIDEO {
                    self.toggle_video();
                } else if id == library::MAIN_RETRY_AUDIO {
                    self.retry_audio_capture(Instant::now());
                } else if id == milkdrop::MAIN_MILKDROP {
                    self.toggle_milkdrop();
                } else if let Some(cmd) = milkdrop::command_for_id(&id) {
                    self.milkdrop.command(cmd);
                }
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if let WindowEvent::DroppedFile(path) = &event {
            self.on_dropped_file(window_id, path.clone());
            return;
        }
        if self.video.as_ref().is_some_and(|v| v.id() == window_id) {
            self.video_event(event);
        } else if self.eq.as_ref().is_some_and(|e| e.id() == window_id) {
            self.eq_event(event);
        } else if self.pl.as_ref().is_some_and(|p| p.id() == window_id) {
            self.pl_event(event);
        } else if self.lib.as_ref().is_some_and(|l| l.id() == window_id) {
            self.lib_event(event);
        } else if self.milkdrop.owns(window_id) {
            self.milkdrop_event(event);
        } else if self.main.as_ref().is_some_and(|m| m.id() == window_id) {
            self.main_event(event_loop, event);
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.tick(event_loop);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn eq_screenshot_goes_next_to_the_main_one() {
        assert_eq!(
            eq_screenshot_path(Path::new("out/m3.png")),
            PathBuf::from("out/m3-eq.png")
        );
        assert_eq!(eq_screenshot_path(Path::new("m3")), PathBuf::from("m3-eq"));
    }
}
