//! The main window: winit events, softbuffer presentation, the YouTube Music bridge, the audio
//! tap and the visualiser, wired together.
//!
//! The bridge webview lives in a hidden host window. The Eject button and the L key show it.

use std::num::NonZeroU32;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::{Duration, Instant};

use softbuffer::{Context, Surface};
use tubeamp_audio::vis::{Vis, VisFrame};
use tubeamp_audio::{params as eq_params, AudioEngine, AudioError};
use tubeamp_bridge::{Bridge, Cmd, Evt};
use tubeamp_skin::Skin;
use tubeamp_ui::main_window::{BALANCE, POSITION, VOLUME};
use tubeamp_ui::{
    hit_test_main, labels, marquee_period_px, render_main, render_vis, sliders, Canvas, MainButton,
    MainHit, MainState, Status, HEIGHT, WIDTH,
};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowAttributes, WindowId};

use crate::player::{Playback, FALLBACK_KHZ};

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

type SharedWindow = Rc<Window>;

/// Settings from the command line and environment.
#[derive(Debug, Clone)]
pub struct Options {
    /// Integer scale of the main window (1 or 2; 2 is double size).
    pub zoom: u32,
    /// Save the live main window (2×) to this PNG after the delay, then keep running.
    pub screenshot: Option<(Duration, PathBuf)>,
    /// Exit after this long.
    pub quit_after: Option<Duration>,
    /// Video to load six seconds after start.
    pub autoload: Option<String>,
    /// Print bridge events and state changes to stderr.
    pub log: bool,
}

/// Events posted to the event loop from outside it.
#[derive(Debug)]
pub enum UserEvent {
    /// A message from `bridge.js`.
    Bridge(Evt),
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

struct App {
    opts: Options,
    skin: Skin,
    proxy: EventLoopProxy<UserEvent>,
    zoom: u32,
    state: MainState,
    playback: Playback,
    window: Option<SharedWindow>,
    context: Option<Context<SharedWindow>>,
    surface: Option<Surface<SharedWindow, SharedWindow>>,
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
    cursor: Option<PhysicalPosition<f64>>,
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
    let mut app = App {
        opts,
        skin,
        proxy: event_loop.create_proxy(),
        zoom,
        state: MainState {
            doubled: zoom == 2,
            // YouTube Music starts at full volume; the slider is the source of truth from here.
            volume: 100,
            ..MainState::default()
        },
        playback: Playback::default(),
        window: None,
        context: None,
        surface: None,
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
        cursor: None,
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

/// Centres `size` pixels within `total`, cropping if it does not fit.
/// Returns `(offset, visible)`. Shared by presentation and cursor mapping.
fn fit(total: u32, size: u32) -> (u32, u32) {
    let visible = size.min(total);
    ((total - visible) / 2, visible)
}

/// Copies a `sw` × `sh` frame into the centre of a `pw` × `ph` buffer, black elsewhere.
fn present(dst: &mut [u32], pw: u32, ph: u32, src: &[u32], sw: u32, sh: u32) {
    dst.fill(0);
    let (ox, cw) = fit(pw, sw);
    let (oy, ch) = fit(ph, sh);
    let stride = pw as usize;
    for (row, src_row) in src.chunks_exact(sw as usize).take(ch as usize).enumerate() {
        let start = (oy as usize + row) * stride + ox as usize;
        dst[start..start + cw as usize].copy_from_slice(&src_row[..cw as usize]);
    }
}

impl App {
    fn log(&self, msg: String) {
        if self.opts.log {
            eprintln!("tubeamp: {msg}");
        }
    }

    fn logical_size(&self) -> LogicalSize<u32> {
        LogicalSize::new(WIDTH * self.zoom, HEIGHT * self.zoom)
    }

    /// Integer scale from main-window pixels to physical pixels: round(scale factor) × zoom.
    fn factor(&self) -> u32 {
        let scale = self.window.as_ref().map_or(1.0, |w| w.scale_factor());
        (scale.round().max(1.0) as u32) * self.zoom
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
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

    /// Pointer position in main-window pixels, not clamped to the window. Drags use this so they
    /// keep tracking past the edge.
    fn main_point(&self) -> Option<(f64, f64)> {
        let pos = self.cursor?;
        let size = self.window.as_ref()?.inner_size();
        let f = self.factor();
        let (ox, _) = fit(size.width, WIDTH * f);
        let (oy, _) = fit(size.height, HEIGHT * f);
        Some((
            (pos.x - f64::from(ox)) / f64::from(f),
            (pos.y - f64::from(oy)) / f64::from(f),
        ))
    }

    /// Pointer position in main-window pixels, or `None` outside the window.
    fn cursor_main(&self) -> Option<(i32, i32)> {
        let (x, y) = self.main_point()?;
        if x < 0.0 || y < 0.0 || x >= f64::from(WIDTH) || y >= f64::from(HEIGHT) {
            return None;
        }
        Some((x as i32, y as i32))
    }

    fn hit_under_cursor(&self) -> Option<MainHit> {
        let (x, y) = self.cursor_main()?;
        hit_test_main(x, y)
    }

    fn on_press(&mut self) {
        let hit = self.hit_under_cursor();
        self.press_target = hit;
        match hit {
            Some(MainHit::TitleBar) => {
                if let Some(window) = &self.window {
                    if let Err(e) = window.drag_window() {
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
        let Some((x, _)) = self.main_point() else {
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
        let hit = self.hit_under_cursor();
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
                    if let Some(window) = &self.window {
                        window.set_minimized(true);
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
                MainButton::EqToggle => self.state.eq_open = !self.state.eq_open,
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

    fn set_doubled(&mut self, on: bool) {
        self.zoom = if on { 2 } else { 1 };
        self.state.doubled = on;
        if let Some(window) = &self.window {
            let _ = window.request_inner_size(self.logical_size());
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
        self.state.title = match self.drag {
            Some(Drag::Position { percent }) if p.duration_secs > 0 => {
                labels::seek_text(p.duration_secs, percent)
            }
            Some(Drag::Volume) => labels::volume_text(self.state.volume),
            Some(Drag::Balance) => labels::balance_text(self.state.balance),
            _ => p.marquee_text(),
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

    fn render_frame(&self, factor: u32) -> Vec<u32> {
        let mut canvas = Canvas::new(WIDTH, HEIGHT);
        render_main(&self.skin, &self.state, &mut canvas);
        render_vis(&self.skin, &self.state, &self.vis_frame, &mut canvas);
        let mut out = vec![0u32; ((WIDTH * factor) * (HEIGHT * factor)) as usize];
        canvas.scale_into(&mut out, factor);
        out
    }

    fn redraw(&mut self) {
        let Some(window) = self.window.clone() else {
            return;
        };
        let size = window.inner_size();
        let (Some(pw), Some(ph)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        let f = self.factor();
        let frame = self.render_frame(f);
        let Some(surface) = self.surface.as_mut() else {
            return;
        };
        if let Err(e) = surface.resize(pw, ph) {
            eprintln!("tubeamp: surface resize failed: {e}");
            return;
        }
        let mut buffer = match surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(e) => {
                eprintln!("tubeamp: could not map the window buffer: {e}");
                return;
            }
        };
        present(
            &mut buffer,
            size.width,
            size.height,
            &frame,
            WIDTH * f,
            HEIGHT * f,
        );
        if let Err(e) = buffer.present() {
            eprintln!("tubeamp: present failed: {e}");
        }
    }

    fn save_screenshot(&self) {
        let Some((_, path)) = &self.opts.screenshot else {
            return;
        };
        let pixels = self.render_frame(2);
        match crate::save_png(&pixels, WIDTH * 2, HEIGHT * 2, path) {
            Ok(()) => eprintln!("tubeamp: wrote {}", path.display()),
            Err(e) => eprintln!("tubeamp: could not write {}: {e}", path.display()),
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
            let active = playing || self.drag.is_some();
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
}

impl ApplicationHandler<UserEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = WindowAttributes::default()
            .with_title("TubeAmp")
            .with_decorations(false)
            .with_resizable(false)
            .with_inner_size(self.logical_size());
        let window: SharedWindow = match event_loop.create_window(attributes) {
            Ok(window) => Rc::new(window),
            Err(e) => {
                eprintln!("tubeamp: could not create the window: {e}");
                event_loop.exit();
                return;
            }
        };
        let context = match Context::new(window.clone()) {
            Ok(context) => context,
            Err(e) => {
                eprintln!("tubeamp: could not create the softbuffer context: {e}");
                event_loop.exit();
                return;
            }
        };
        let surface = match Surface::new(&context, window.clone()) {
            Ok(surface) => surface,
            Err(e) => {
                eprintln!("tubeamp: could not create the softbuffer surface: {e}");
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

        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
        self.host = Some(host);
        self.bridge = Some(bridge);
        self.host_visible = false;
        self.request_redraw();
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: UserEvent) {
        match event {
            UserEvent::Bridge(evt) => self.on_bridge_event(evt),
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
            return;
        }
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::RedrawRequested => self.redraw(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.request_redraw()
            }
            WindowEvent::Focused(focused) => {
                self.state.focused = focused;
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor = Some(position);
                if self.drag.is_some() {
                    self.update_drag();
                }
            }
            WindowEvent::CursorLeft { .. } => self.cursor = None,
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

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        self.tick(event_loop);
    }
}
