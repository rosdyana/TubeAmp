//! The windows and their event loop: the main window, the equalizer and the playlist editor (all
//! skinned, see `skinned.rs`), the YouTube Music bridge, the audio tap, the EQ and the visualiser,
//! wired together. The playlist decides what plays next: the main window's previous and next
//! buttons, Z and B, and the end of each track all go through it.
//!
//! The bridge webview lives in a hidden host window. The Eject button and the L key show it.

use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::{Duration, Instant};

use tubeamp_audio::vis::{Vis, VisFrame};
use tubeamp_audio::{params as eq_params, AudioEngine, AudioError};
use tubeamp_bridge::{Bridge, Cmd, Evt};
use tubeamp_eq::model::ZERO_VALUE;
use tubeamp_eq::{Preset, BANDS_HZ, BUILTIN};
use tubeamp_playlist::{Entry, Playlist};
use tubeamp_skin::Skin;
use tubeamp_ui::eq_window as eqw;
use tubeamp_ui::gen_window as gen;
use tubeamp_ui::library_window as lw;
use tubeamp_ui::main_window::{BALANCE, CLUTTER_O, POSITION, VOLUME};
use tubeamp_ui::playlist_window as plw;
use tubeamp_ui::{
    displayed_secs, hit_test_eq, hit_test_library, hit_test_main, hit_test_playlist, labels,
    marquee_period_px, menu_button_rect, press_grab, render_eq, render_library, render_main,
    render_playlist, render_vis, row_index_at, slider_value_at, sliders, Canvas, EqHit, EqSlider,
    EqWindowState, LibHit, LibKind, MainButton, MainHit, MainState, PlHit, PlMenu, PlTransport,
    PlaylistWindowState, Status, HEIGHT, PRESETS_BTN, WIDTH,
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{WindowAttributes, WindowId};

use crate::eq::{self, EqCommand};
use crate::library::{self, Action, LibCommand, Library};
use crate::player::{Playback, FALLBACK_KHZ};
use crate::playlist::{self as pl_app, PlAction, SortCommand};
use crate::skinned::{self, scaled, SharedWindow, SkinnedWindow};

/// Marquee step: one character cell, as in Webamp's `Marquee.tsx`.
const MARQUEE_STEP_PX: u32 = 5;
/// Webamp steps the marquee every 220 ms.
const MARQUEE_INTERVAL: Duration = Duration::from_millis(220);
/// Redraw interval while playing or dragging (about 30 fps).
const FRAME_ACTIVE: Duration = Duration::from_millis(33);
/// Redraw interval otherwise (about 4 fps).
const FRAME_IDLE: Duration = Duration::from_millis(250);
/// How often to retry the audio tap while playing.
const AUDIO_RETRY: Duration = Duration::from_secs(2);
/// Delay before `TUBEAMP_AUTOLOAD` loads its video.
const AUTOLOAD_DELAY: Duration = Duration::from_secs(6);
/// Arrow-key seek step, in seconds (Winamp).
const SEEK_STEP_SECS: f64 = 5.0;
/// Arrow-key volume step (the plan's ±2).
const VOLUME_STEP: i32 = 2;
/// Host window size (logical pixels).
const HOST_WIDTH: f64 = 480.0;
const HOST_HEIGHT: f64 = 360.0;
/// Two clicks on the same playlist row this close together play it.
const DOUBLE_CLICK: Duration = Duration::from_millis(500);
/// Rows scrolled per wheel notch, as Winamp does.
const WHEEL_ROWS: isize = 3;

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
}

/// Events posted to the event loop from outside it.
#[derive(Debug)]
pub enum UserEvent {
    /// A message from `bridge.js`.
    Bridge(Evt),
    /// A PRESETS menu item was chosen. The payload is its menu id.
    Menu(String),
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
    /// The main window's outer position and inner size, as last seen. The equalizer docks to it.
    main_rect: Option<(PhysicalPosition<i32>, PhysicalSize<u32>)>,
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
    // Dropped before `host`, because the webview is a child of the host window.
    bridge: Option<Bridge>,
    host: Option<SharedWindow>,
    host_visible: bool,
    audio: Option<AudioEngine>,
    audio_given_up: bool,
    audio_error_shown: bool,
    audio_retry_at: Instant,
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
}

/// Opens the windows and runs the event loop until the main window is closed.
pub fn run(skin: Skin, opts: Options) -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::<UserEvent>::with_user_event().build()?;
    let started = Instant::now();
    let zoom = opts.zoom;
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
        main_rect: None,
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
        host: None,
        host_visible: false,
        audio: None,
        audio_given_up: false,
        audio_error_shown: false,
        audio_retry_at: started,
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
    };
    event_loop.run_app(&mut app)
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
        hit_test_main(x, y)
    }

    /// The equalizer's hit target under the pointer.
    fn eq_hit_under_cursor(&self) -> Option<EqHit> {
        let (x, y) = self.eq.as_ref()?.skin_pixel()?;
        hit_test_eq(x, y)
    }

    fn on_press(&mut self) {
        let hit = self.main_hit();
        self.press_target = hit;
        match hit {
            Some(MainHit::TitleBar) => {
                if let Some(main) = &self.main {
                    if let Err(e) = main.window.drag_window() {
                        eprintln!("tubeamp: window drag failed: {e}");
                    }
                }
            }
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
                let fraction = sliders::position_fraction(x - f64::from(POSITION.x));
                self.drag = Some(Drag::Position {
                    percent: sliders::percent(fraction),
                });
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
                MainButton::Eject => self.toggle_host(),
                MainButton::Shuffle => self.state.shuffle = !self.state.shuffle,
                MainButton::Repeat => self.state.repeat = !self.state.repeat,
                MainButton::EqToggle => self.toggle_eq(),
                MainButton::PlToggle => self.toggle_pl(),
                MainButton::ClutterO => self.open_main_menu(),
                MainButton::ClutterD => {
                    let on = !self.state.doubled;
                    self.set_doubled(on);
                }
                _ => {}
            },
            MainHit::Time => self.state.remaining_mode = !self.state.remaining_mode,
            MainHit::Vis => self.state.vis_mode = self.state.vis_mode.next(),
            _ => {}
        }
        self.request_redraw();
    }

    fn on_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        if self.modifiers.control_key() {
            if event.repeat {
                return;
            }
            match event.key_without_modifiers() {
                Key::Character(c) if c.eq_ignore_ascii_case("d") => {
                    let on = !self.state.doubled;
                    self.set_doubled(on);
                }
                Key::Character(c) if c.eq_ignore_ascii_case("t") => {
                    self.state.remaining_mode = !self.state.remaining_mode;
                }
                Key::Character(c) if c.eq_ignore_ascii_case("l") => self.toggle_library(),
                _ => return,
            }
            self.request_redraw();
            return;
        }
        // Alt+L opens the media library (Winamp's shortcut). Other Alt keys are left alone.
        if self.modifiers.alt_key()
            && !event.repeat
            && matches!(event.key_without_modifiers(), Key::Character(c) if c.eq_ignore_ascii_case("l"))
        {
            self.toggle_library();
            self.request_redraw();
            return;
        }
        match event.key_without_modifiers() {
            Key::Named(NamedKey::ArrowLeft) => self.seek_relative(-SEEK_STEP_SECS),
            Key::Named(NamedKey::ArrowRight) => self.seek_relative(SEEK_STEP_SECS),
            Key::Named(NamedKey::ArrowUp) => self.nudge_volume(VOLUME_STEP),
            Key::Named(NamedKey::ArrowDown) => self.nudge_volume(-VOLUME_STEP),
            _ if event.repeat => return,
            Key::Character(c) => match c.to_ascii_lowercase().as_str() {
                "z" => self.prev_track(),
                "x" => self.send(Cmd::Play),
                "c" => self.send(Cmd::Pause),
                "v" => self.send(Cmd::Stop),
                "b" => self.next_track(),
                "l" => self.toggle_host(),
                _ => return,
            },
            _ => return,
        }
        self.request_redraw();
    }

    /// Doubles (or halves) both windows. The docked equalizer follows the main window's new
    /// bottom edge in `on_main_resized`.
    fn set_doubled(&mut self, on: bool) {
        let zoom = if on { 2 } else { 1 };
        self.state.doubled = on;
        if let Some(main) = self.main.as_mut() {
            main.set_zoom(zoom);
        }
        if let Some(eq) = self.eq.as_mut() {
            eq.set_zoom(zoom);
        }
        if let Some(pl) = self.pl.as_mut() {
            pl.set_zoom(zoom);
        }
        if let Some(lib) = self.lib.as_mut() {
            lib.set_zoom(zoom);
        }
        self.request_redraw();
    }

    fn toggle_host(&mut self) {
        self.host_visible = !self.host_visible;
        self.apply_host_visibility();
    }

    fn apply_host_visibility(&self) {
        if let Some(host) = &self.host {
            host.set_visible(self.host_visible);
            if self.host_visible {
                host.focus_window();
            }
        }
        self.log(format!("video host visible = {}", self.host_visible));
    }

    // Playlist window.

    /// The playlist's hit target under the pointer.
    fn pl_hit_under_cursor(&self) -> Option<PlHit> {
        let (x, y) = self.pl.as_ref()?.skin_pixel()?;
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
        self.dock_playlist();
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

    /// Where a playlist docked to a main window at `main_pos` and `main_height` sits: under the
    /// equalizer when it is open, otherwise under the main window. Both share the main window's
    /// left edge.
    fn dock_point(
        &self,
        main_pos: PhysicalPosition<i32>,
        main_height: u32,
    ) -> PhysicalPosition<i32> {
        if self.eq_visible {
            if let Some(eq) = self.eq.as_ref() {
                if let Ok(eq_pos) = eq.window.outer_position() {
                    let eq_height = eq.window.inner_size().height as i32;
                    return PhysicalPosition::new(main_pos.x, eq_pos.y + eq_height);
                }
            }
        }
        PhysicalPosition::new(main_pos.x, main_pos.y + main_height as i32)
    }

    /// Moves the playlist to its dock point for the main window as it is now.
    fn dock_playlist(&self) {
        let Some(main) = self.main.as_ref() else {
            return;
        };
        let Ok(main_pos) = main.window.outer_position() else {
            return;
        };
        let point = self.dock_point(main_pos, main.window.inner_size().height);
        if let Some(pl) = self.pl.as_ref() {
            pl.window.set_outer_position(point);
        }
    }

    /// Whether the playlist is at its dock point for a main window at `main_pos` and
    /// `main_height`. Call this before the windows move, so the equalizer is still where it was.
    fn pl_docked_for(&self, main_pos: PhysicalPosition<i32>, main_height: u32) -> bool {
        let Some(pl) = self.pl.as_ref() else {
            return false;
        };
        let Ok(pos) = pl.window.outer_position() else {
            return false;
        };
        pos == self.dock_point(main_pos, main_height)
    }

    fn pl_docked_now(&self) -> bool {
        let Some(main) = self.main.as_ref() else {
            return false;
        };
        match main.window.outer_position() {
            Ok(pos) => self.pl_docked_for(pos, main.window.inner_size().height),
            Err(_) => false,
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
        if let Some(pl) = self.pl.as_mut() {
            pl.set_base(plw::window_size(steps));
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
            Some(PlHit::TitleBar) => {
                if let Some(pl) = self.pl.as_ref() {
                    if let Err(e) = pl.window.drag_window() {
                        eprintln!("tubeamp: playlist drag failed: {e}");
                    }
                }
            }
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
            // Windowshade for the playlist comes with M5; the button is drawn but does nothing yet.
            PlHit::Shade => {}
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
            PlTransport::Eject => self.toggle_host(),
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
                let steps = plw::steps_for_size(w.round() as i32, h.round() as i32);
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
            PlAction::AddUrl => match pl_app::clipboard_video_id() {
                Ok(id) => {
                    let index = self.playlist.add(Entry::from_id(&id));
                    self.show_row(index);
                    self.log(format!("playlist: added {id}"));
                }
                Err(msg) => eprintln!("tubeamp: {msg}"),
            },
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

    /// LIST ▸ SAVE: the playlist as a JSON array of entries.
    fn save_list(&mut self) {
        let Some(path) = pl_app::pick_list_to_save() else {
            return;
        };
        match std::fs::write(&path, self.playlist.to_json()) {
            Ok(()) => eprintln!("tubeamp: saved the playlist to {}", path.display()),
            Err(e) => eprintln!("tubeamp: could not write {}: {e}", path.display()),
        }
    }

    /// LIST ▸ LOAD: replaces the playlist with a JSON file written by SAVE.
    fn load_list(&mut self) {
        let Some(path) = pl_app::pick_list_to_load() else {
            return;
        };
        let loaded = std::fs::read_to_string(&path)
            .map_err(|e| e.to_string())
            .and_then(|text| Playlist::from_json(&text).map_err(|e| e.to_string()));
        match loaded {
            Ok(list) => {
                let count = list.len();
                self.playlist = list;
                self.pl_state.scroll = 0;
                eprintln!("tubeamp: loaded {count} tracks from {}", path.display());
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

    /// Shows the library. The first time, it goes to the right of the main window.
    fn show_library(&mut self) {
        let Some(lib) = self.lib.as_ref() else {
            return;
        };
        if !self.lib_placed {
            if let Some(main) = self.main.as_ref() {
                if let Ok(pos) = main.window.outer_position() {
                    let x = pos.x + main.window.inner_size().width as i32 + 8;
                    lib.window
                        .set_outer_position(PhysicalPosition::new(x, pos.y));
                }
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
        let menu = match library::build_main_menu() {
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
            Some(LibHit::TitleBar) => {
                if let Some(lib) = self.lib.as_ref() {
                    if let Err(e) = lib.window.drag_window() {
                        eprintln!("tubeamp: media library drag failed: {e}");
                    }
                }
            }
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

    // Equalizer window.

    fn toggle_eq(&mut self) {
        // A playlist docked under the equalizer (or the main window) follows the toggle.
        let attached = self.pl_visible && self.pl_docked_now();
        if self.eq_visible {
            self.hide_eq();
        } else {
            self.show_eq();
        }
        if attached {
            self.dock_playlist();
        }
    }

    /// Shows the equalizer directly below the main window, at the main window's left edge.
    fn show_eq(&mut self) {
        let Some(main) = self.main.as_ref() else {
            return;
        };
        if let Ok(pos) = main.window.outer_position() {
            let height = main.window.inner_size().height as i32;
            self.move_eq(PhysicalPosition::new(pos.x, pos.y + height));
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

    fn eq_outer_position(&self) -> Option<PhysicalPosition<i32>> {
        self.eq.as_ref()?.window.outer_position().ok()
    }

    /// Whether the equalizer's top edge sits exactly on the bottom edge of a main window at
    /// `main_pos` with `main_size`.
    fn eq_docked_to(&self, main_pos: PhysicalPosition<i32>, main_size: PhysicalSize<u32>) -> bool {
        self.eq_outer_position()
            .is_some_and(|p| p.y == main_pos.y + main_size.height as i32)
    }

    /// The main window moved: a docked equalizer and a docked playlist move with it. This is a
    /// simple attach; full snapping comes in M5.
    fn on_main_moved(&mut self, pos: PhysicalPosition<i32>) {
        match self.main_rect {
            Some((old, size)) => {
                let pl_attached = self.pl_visible && self.pl_docked_for(old, size.height);
                let delta_x = pos.x - old.x;
                let delta_y = pos.y - old.y;
                if self.eq_visible && self.eq_docked_to(old, size) {
                    if let Some(eq_pos) = self.eq_outer_position() {
                        self.move_eq(PhysicalPosition::new(
                            eq_pos.x + delta_x,
                            eq_pos.y + delta_y,
                        ));
                    }
                }
                if pl_attached {
                    if let Some(pl) = self.pl.as_ref() {
                        if let Ok(pl_pos) = pl.window.outer_position() {
                            pl.window.set_outer_position(PhysicalPosition::new(
                                pl_pos.x + delta_x,
                                pl_pos.y + delta_y,
                            ));
                        }
                    }
                }
                self.main_rect = Some((pos, size));
            }
            None => {
                self.main_rect = self.main.as_ref().map(|m| (pos, m.window.inner_size()));
            }
        }
    }

    /// The main window changed size (zoom): a docked equalizer keeps its top on the new bottom.
    fn on_main_resized(&mut self, size: PhysicalSize<u32>) {
        let Some((pos, old_size)) = self.main_rect else {
            self.main_rect = self
                .main
                .as_ref()
                .and_then(|m| m.window.outer_position().ok())
                .map(|p| (p, size));
            return;
        };
        let pl_attached = self.pl_visible && self.pl_docked_for(pos, old_size.height);
        if self.eq_visible && self.eq_docked_to(pos, old_size) {
            if let Some(eq_pos) = self.eq_outer_position() {
                self.move_eq(PhysicalPosition::new(eq_pos.x, pos.y + size.height as i32));
            }
        }
        if pl_attached {
            self.dock_playlist();
        }
        self.main_rect = Some((pos, size));
    }

    fn on_eq_press(&mut self) {
        let hit = self.eq_hit_under_cursor();
        self.eq_press = hit;
        self.eq_state.pressed = hit;
        match hit {
            Some(EqHit::TitleBar) => {
                if let Some(eq) = self.eq.as_ref() {
                    if let Err(e) = eq.window.drag_window() {
                        eprintln!("tubeamp: equalizer drag failed: {e}");
                    }
                }
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
            EqHit::On => self.toggle_power(),
            EqHit::Auto => self.toggle_auto(),
            EqHit::Presets => self.open_presets_menu(),
            EqHit::TitleBar | EqHit::Slider(_) => {}
        }
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

    /// ON: the bypass switch. The audio tap applies the curve only while this is on.
    fn toggle_power(&mut self) {
        let on = !self.eq_state.settings.on;
        self.eq_state.settings.on = on;
        eq_params().set_on(on);
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
        self.log(format!("event {evt:?}"));
        match &evt {
            Evt::State { .. } => {
                let before = self.playback.clone();
                self.playback.apply(&evt);
                self.library.set_playing(self.playback.video_id.as_deref());
                self.log_transitions(&before);
                // Push the slider's volume to the player once the player exists.
                if !self.volume_synced && self.playback.ps.is_some() {
                    self.volume_synced = true;
                    let level = self.state.volume;
                    self.send(Cmd::Volume { level });
                }
                self.follow_playlist();
            }
            Evt::Ended => {
                self.log("track ended".to_string());
                self.on_track_ended();
            }
            Evt::Error { at, msg } => eprintln!("tubeamp: YouTube Music error at {at}: {msg}"),
            Evt::Ready { signed_in } => {
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
        self.request_redraw();
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
    fn refresh_display(&mut self) {
        let p = &self.playback;
        self.state.status = p.status;
        self.state.time_secs = p.time_secs;
        self.state.duration_secs = p.duration_secs;
        self.state.kbps = p.kbps_for_display().or(self.state.kbps);
        self.state.khz = Some(
            self.audio
                .as_ref()
                .map_or(FALLBACK_KHZ, |engine| engine.sample_rate() / 1000),
        );
        // The bridge does not report the channel count, so a playing track is treated as stereo.
        self.state.stereo = p.status == Status::Play;
        if !matches!(self.drag, Some(Drag::Position { .. })) {
            self.state.position = if p.duration_secs > 0 {
                p.time_secs as f32 / p.duration_secs as f32
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
        // The playlist's mini time follows the main window's time mode while playing or paused.
        self.pl_state.mini_time_secs =
            (self.state.status != Status::Stop).then(|| displayed_secs(&self.state));
        self.pl_state.mini_remaining = self.state.remaining_mode;
    }

    fn try_start_audio(&mut self, now: Instant) {
        match AudioEngine::start() {
            Ok(engine) => {
                eprintln!("tubeamp: audio tap running at {} Hz", engine.sample_rate());
                self.audio = Some(engine);
            }
            Err(e @ AudioError::Unsupported) => {
                eprintln!("tubeamp: {e}; the EQ and visualiser audio are off");
                self.audio_given_up = true;
            }
            Err(e) => {
                if !self.audio_error_shown || self.opts.log {
                    eprintln!("tubeamp: audio tap not ready ({e}); retrying");
                }
                self.audio_error_shown = true;
                self.audio_retry_at = now + AUDIO_RETRY;
            }
        }
    }

    /// Feeds the analyser with the samples since the last frame, and advances it one step.
    fn step_vis(&mut self) {
        match self.audio.as_mut() {
            Some(engine) => {
                engine.drain_samples(&mut self.samples);
                self.vis_frame = self.vis.update(&self.samples).clone();
            }
            None => self.vis_frame = self.vis.update(&[]).clone(),
        }
    }

    /// The main window as it looks now, at skin size.
    fn main_canvas(&self) -> Canvas {
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        render_main(&self.skin, &self.state, &mut canvas);
        render_vis(&self.skin, &self.state, &self.vis_frame, &mut canvas);
        canvas
    }

    /// The equalizer window as it looks now, at skin size.
    fn eq_canvas(&self) -> Canvas {
        let mut canvas = Canvas::new(eqw::WIDTH, eqw::HEIGHT);
        render_eq(&self.skin, &self.eq_state, &mut canvas);
        canvas
    }

    /// The playlist window as it looks now, at its current size.
    fn pl_canvas(&self) -> Canvas {
        let (w, h) = plw::window_size(self.pl_state.size_steps);
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
        save_one(&scaled(&self.main_canvas(), 2), WIDTH * 2, HEIGHT * 2, path);
        if self.eq_visible {
            let eq_path = eq_screenshot_path(path);
            save_one(
                &scaled(&self.eq_canvas(), 2),
                eqw::WIDTH * 2,
                eqw::HEIGHT * 2,
                &eq_path,
            );
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
    }

    fn next_wake(&self, playing: bool) -> Instant {
        let mut wake = self.next_frame.min(self.next_marquee);
        if playing && self.audio.is_none() && !self.audio_given_up {
            wake = wake.min(self.audio_retry_at);
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
        if playing && self.audio.is_none() && !self.audio_given_up && now >= self.audio_retry_at {
            self.try_start_audio(now);
        }

        if now >= self.next_frame {
            if playing {
                self.step_vis();
            }
            self.refresh_display();
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

        el.set_control_flow(ControlFlow::WaitUntil(self.next_wake(playing)));
    }

    fn host_event(&mut self, event: WindowEvent) {
        match event {
            // Closing the video window hides it. The playback keeps going.
            WindowEvent::CloseRequested => {
                self.host_visible = false;
                self.apply_host_visibility();
            }
            WindowEvent::Resized(size) => {
                if let (Some(bridge), Some(host)) = (&self.bridge, &self.host) {
                    let logical = size.to_logical::<f64>(host.scale_factor());
                    if let Err(e) = bridge.set_bounds(logical.width, logical.height) {
                        eprintln!("tubeamp: could not resize the video view: {e}");
                    }
                }
            }
            _ => {}
        }
    }

    fn main_event(&mut self, event_loop: &ActiveEventLoop, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw_main(),
            WindowEvent::Resized(size) => {
                self.on_main_resized(size);
                self.request_redraw();
            }
            WindowEvent::Moved(pos) => self.on_main_moved(pos),
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

        let host_attributes = WindowAttributes::default()
            .with_title("TubeAmp Video")
            .with_inner_size(LogicalSize::new(HOST_WIDTH, HOST_HEIGHT))
            .with_visible(false);
        let host: SharedWindow = match event_loop.create_window(host_attributes) {
            Ok(host) => Rc::new(host),
            Err(e) => {
                eprintln!("tubeamp: could not create the video window: {e}");
                event_loop.exit();
                return;
            }
        };
        let proxy = self.proxy.clone();
        let bridge = match Bridge::new(&host, move |evt| {
            let _ = proxy.send_event(UserEvent::Bridge(evt));
        }) {
            Ok(bridge) => bridge,
            Err(e) => {
                eprintln!("tubeamp: could not create the YouTube Music view: {e}");
                event_loop.exit();
                return;
            }
        };

        self.main_rect = main
            .window
            .outer_position()
            .ok()
            .map(|p| (p, main.window.inner_size()));
        self.main = Some(main);
        self.eq = Some(eq);
        self.host = Some(host);
        self.bridge = Some(bridge);
        self.host_visible = false;

        // The audio starts from the flat curve, or from the preset named on the command line.
        self.apply_eq_settings();
        if let Some(preset) = self.opts.eq_preset {
            self.apply_curve(preset.preamp, preset.bands);
        }
        if self.opts.open_eq {
            self.show_eq();
        }
        self.pl = Some(pl);
        self.lib = Some(lib);
        for id in self.opts.add.clone() {
            self.playlist.add(Entry::from_id(&id));
        }
        if self.opts.open_pl {
            self.show_pl();
        }
        if self.opts.open_library {
            self.show_library();
        }
        self.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Bridge(evt) => self.on_bridge_event(evt),
            UserEvent::Menu(id) => {
                self.log(format!("menu item {id}"));
                if let Some(cmd) = eq::command_for_id(&id) {
                    self.run_eq_command(cmd);
                } else if let Some(cmd) = pl_app::sort_command_for_id(&id) {
                    self.run_sort(cmd);
                } else if let Some(cmd) = library::command_for_id(&id) {
                    self.run_library_command(cmd);
                } else if id == library::MAIN_LIBRARY {
                    self.show_library();
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
        if self.host.as_ref().is_some_and(|h| h.id() == window_id) {
            self.host_event(event);
        } else if self.eq.as_ref().is_some_and(|e| e.id() == window_id) {
            self.eq_event(event);
        } else if self.pl.as_ref().is_some_and(|p| p.id() == window_id) {
            self.pl_event(event);
        } else if self.lib.as_ref().is_some_and(|l| l.id() == window_id) {
            self.lib_event(event);
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
