//! The windows and their event loop: the main window and the equalizer window (both skinned, see
//! `skinned.rs`), the YouTube Music bridge, the audio tap, the EQ and the visualiser, wired
//! together.
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
use tubeamp_skin::Skin;
use tubeamp_ui::eq_window as eqw;
use tubeamp_ui::main_window::{BALANCE, POSITION, VOLUME};
use tubeamp_ui::{
    hit_test_eq, hit_test_main, labels, marquee_period_px, press_grab, render_eq, render_main,
    render_vis, sliders, slider_value_at, Canvas, EqHit, EqSlider, EqWindowState, MainButton,
    MainHit, MainState, Status, HEIGHT, PRESETS_BTN, WIDTH,
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition, PhysicalSize};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{WindowAttributes, WindowId};

use crate::eq::{self, EqCommand};
use crate::player::{Playback, FALLBACK_KHZ};
use crate::skinned::{scaled, SharedWindow, SkinnedWindow};

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
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let name = match path.extension() {
        Some(ext) => format!("{stem}-eq.{}", ext.to_string_lossy()),
        None => format!("{stem}-eq"),
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

    /// Requests a redraw of the main window, and of the equalizer while it is open.
    fn request_redraw(&self) {
        if let Some(main) = &self.main {
            main.request_redraw();
        }
        if self.eq_visible {
            if let Some(eq) = &self.eq {
                eq.request_redraw();
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
                MainButton::Previous => self.send(Cmd::Prev),
                MainButton::Play => self.send(Cmd::Play),
                MainButton::Pause => self.send(Cmd::Pause),
                MainButton::Stop => self.send(Cmd::Stop),
                MainButton::Next => self.send(Cmd::Next),
                MainButton::Eject => self.toggle_host(),
                MainButton::Shuffle => self.state.shuffle = !self.state.shuffle,
                MainButton::Repeat => self.state.repeat = !self.state.repeat,
                MainButton::EqToggle => self.toggle_eq(),
                MainButton::PlToggle => self.state.pl_open = !self.state.pl_open,
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
                _ => return,
            }
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
                "z" => self.send(Cmd::Prev),
                "x" => self.send(Cmd::Play),
                "c" => self.send(Cmd::Pause),
                "v" => self.send(Cmd::Stop),
                "b" => self.send(Cmd::Next),
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

    // Equalizer window.

    fn toggle_eq(&mut self) {
        if self.eq_visible {
            self.hide_eq();
        } else {
            self.show_eq();
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

    /// The main window moved: a docked equalizer moves with it. This is a simple attach; full
    /// snapping comes in M5.
    fn on_main_moved(&mut self, pos: PhysicalPosition<i32>) {
        match self.main_rect {
            Some((old, size)) => {
                if self.eq_visible && self.eq_docked_to(old, size) {
                    if let Some(eq_pos) = self.eq_outer_position() {
                        let delta_x = pos.x - old.x;
                        let delta_y = pos.y - old.y;
                        self.move_eq(PhysicalPosition::new(eq_pos.x + delta_x, eq_pos.y + delta_y));
                    }
                }
                self.main_rect = Some((pos, size));
            }
            None => {
                self.main_rect = self
                    .main
                    .as_ref()
                    .map(|m| (pos, m.window.inner_size()));
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
        if self.eq_visible && self.eq_docked_to(pos, old_size) {
            if let Some(eq_pos) = self.eq_outer_position() {
                self.move_eq(PhysicalPosition::new(eq_pos.x, pos.y + size.height as i32));
            }
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
                self.log_transitions(&before);
                // Push the slider's volume to the player once the player exists.
                if !self.volume_synced && self.playback.ps.is_some() {
                    self.volume_synced = true;
                    let level = self.state.volume;
                    self.send(Cmd::Volume { level });
                }
            }
            Evt::Ended => self.log("track ended".to_string()),
            Evt::Error { at, msg } => eprintln!("tubeamp: YouTube Music error at {at}: {msg}"),
        }
        self.request_redraw();
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
                _ => p.marquee_text(),
            }
        };
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
        let eq_attributes = WindowAttributes::default()
            .with_title("TubeAmp Equalizer")
            .with_decorations(false)
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(eqw::WIDTH * zoom, eqw::HEIGHT * zoom));
        let eq = match SkinnedWindow::new(
            event_loop,
            eq_attributes,
            (eqw::WIDTH, eqw::HEIGHT),
            zoom,
        ) {
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
        self.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Bridge(evt) => self.on_bridge_event(evt),
            UserEvent::Menu(id) => {
                self.log(format!("menu item {id}"));
                if let Some(cmd) = eq::command_for_id(&id) {
                    self.run_eq_command(cmd);
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
