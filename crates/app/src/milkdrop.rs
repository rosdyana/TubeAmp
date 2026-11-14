//! The Milkdrop window: a gen-frame skinned window (see `tubeamp_ui::gen_window`) whose client area
//! is a child webview running butterchurn, Webamp's WebGL port of Milkdrop (MIT).
//!
//! The page (`assets/milkdrop/milkdrop.html`) and its two scripts are compiled into the binary and
//! served over the `tubeamp:` custom protocol, so it works offline. The page keeps the presets,
//! the keys and the animation loop. Rust feeds it the newest 1024 mono samples of the audio tap
//! about 60 times a second (see `tubeamp_audio::LatestSamples`), handles the native frame (drag,
//! resize, close, right-click menu) and fullscreen, and reports back over `window.ipc`.
//!
//! The page only runs its animation while the window shows: Rust pauses it when the window is
//! hidden, and the webview is created the first time the window shows.

use std::borrow::Cow;
use std::path::Path;
use std::rc::Rc;
use std::time::{Duration, Instant};

use serde::Deserialize;
use tubeamp_audio::AudioEngine;
use tubeamp_skin::Skin;
use tubeamp_ui::gen_window::{self as gen, GenHit};
use tubeamp_ui::Canvas;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::ActiveEventLoop;
use winit::keyboard::{Key, ModifiersState, NamedKey};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{WindowAttributes, WindowId};
use wry::http::header::CONTENT_TYPE;
use wry::http::{Request, Response};
use wry::{Rect, WebView, WebViewBuilder};

use crate::skinned::{self, scaled, SkinnedWindow};
use crate::window::suffixed_path;

/// The O menu's item that opens this window (Winamp's Milkdrop is under its visualization menu).
pub const MAIN_MILKDROP: &str = "main.milkdrop";

/// The frame's size in resize steps when the window first opens: 275 × 232, as Webamp's default.
pub const DEFAULT_STEPS: (u32, u32) = (0, 4);

/// Samples per audio feed. Butterchurn's analyser holds 1024 samples, so the page takes that many.
pub const FEED_SAMPLES: usize = 1024;

/// How often the audio is fed to the page: about 60 times a second.
const FEED_INTERVAL: Duration = Duration::from_millis(16);

/// The custom protocol's name, and the page's address. On Windows wry serves the same page at
/// `http://tubeamp.localhost/`, which is why the page's scripts use relative paths.
const SCHEME: &str = "tubeamp";
const ENTRY_URL: &str = "tubeamp://localhost/milkdrop.html";

/// The frame's title, as Webamp shows it.
const TITLE: &str = "MILKDROP";

/// Menu item ids of the right-click menu.
const MENU_NEXT: &str = "milkdrop.next";
const MENU_PREVIOUS: &str = "milkdrop.previous";
const MENU_RANDOM: &str = "milkdrop.random";
const MENU_FULLSCREEN: &str = "milkdrop.fullscreen";
const MENU_CLOSE: &str = "milkdrop.close";

/// The page's files, compiled in. Paths are as the page asks for them.
struct Asset {
    path: &'static str,
    mime: &'static str,
    bytes: &'static [u8],
}

const ASSETS: [Asset; 3] = [
    Asset {
        path: "/milkdrop.html",
        mime: "text/html; charset=utf-8",
        bytes: include_bytes!("../assets/milkdrop/milkdrop.html"),
    },
    Asset {
        path: "/butterchurn.min.js",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../assets/milkdrop/butterchurn.min.js"),
    },
    Asset {
        path: "/butterchurnPresets.min.js",
        mime: "text/javascript; charset=utf-8",
        bytes: include_bytes!("../assets/milkdrop/butterchurnPresets.min.js"),
    },
];

/// A message from the page, as JSON with the type in `t`. The page sends these in `window.ipc`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Msg {
    /// The page is set up and its API exists.
    Ready,
    /// Double-click, or F: toggle fullscreen.
    Fullscreen,
    /// Escape: leaves fullscreen.
    Escape,
    /// Random cycling changed (the R key).
    Random { on: bool },
    /// Right-click, at a point in the page's CSS pixels.
    Menu { x: f64, y: f64 },
    /// A key the page does not use. The app runs it as a page key (the global shortcuts).
    Key {
        key: String,
        code: String,
        ctrl: bool,
        alt: bool,
        meta: bool,
    },
    /// Once a second, while the animation runs. `size` describes the canvas and viewport.
    Stats {
        fps: f64,
        feeds: f64,
        #[serde(default)]
        size: String,
    },
    /// The page hit an error.
    Error { msg: String },
}

/// What the right-click menu and the O menu ask the window to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Next,
    Previous,
    Random,
    Fullscreen,
    Close,
}

/// The menu item id for a command, if it is one of the Milkdrop menu's items.
pub fn command_for_id(id: &str) -> Option<Command> {
    match id {
        MENU_NEXT => Some(Command::Next),
        MENU_PREVIOUS => Some(Command::Previous),
        MENU_RANDOM => Some(Command::Random),
        MENU_FULLSCREEN => Some(Command::Fullscreen),
        MENU_CLOSE => Some(Command::Close),
        _ => None,
    }
}

/// The right-click menu. Random cycling has a check mark while it is on.
pub fn build_menu(random: bool) -> Result<muda::Menu, muda::Error> {
    use muda::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem};

    let menu = Menu::new();
    menu.append(&MenuItem::with_id(MENU_NEXT, "Next preset", true, None))?;
    menu.append(&MenuItem::with_id(MENU_PREVIOUS, "Previous preset", true, None))?;
    menu.append(&CheckMenuItem::with_id(
        MENU_RANDOM,
        "Random cycling",
        true,
        random,
        None,
    ))?;
    menu.append(&MenuItem::with_id(MENU_FULLSCREEN, "Fullscreen", true, None))?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(MENU_CLOSE, "Close", true, None))?;
    Ok(menu)
}

/// A move of the window that the app carries out, because it owns the other windows.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Effect {
    /// The title bar was pressed: the app starts dragging the window.
    BeginDrag,
    /// The pointer moved: the app moves the dragged window (it ignores this when no drag runs).
    DragMoved,
    /// The button came up: the app ends the drag.
    EndDrag,
    /// The corner grip is dragged to this size, in skin pixels. The app snaps it to the other
    /// windows and calls [`MilkdropWindow::set_steps`].
    Resize(f64, f64),
}

/// A resize in progress: the steps at the press, and where the pointer was.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Resize {
    start_steps: (u32, u32),
    start_cursor: (f64, f64),
}

/// The Milkdrop window and its webview. The webview is created when the window first shows.
pub struct MilkdropWindow {
    // Dropped before `win`, because the webview is a child of the window.
    view: Option<WebView>,
    win: Option<SkinnedWindow>,
    /// Where the page's messages go (the app's event loop).
    sink: Rc<dyn Fn(Msg)>,
    /// Whether the page has said it is ready. Until then it gets no calls.
    ready: bool,
    visible: bool,
    /// Whether the window has been placed beside the main window (on its first show).
    placed: bool,
    steps: (u32, u32),
    focused: bool,
    fullscreen: bool,
    /// Random cycling, as the page last reported it.
    random: bool,
    modifiers: ModifiersState,
    /// The frame piece pressed under a press not released yet.
    press: Option<GenHit>,
    resize: Option<Resize>,
    next_feed: Instant,
    /// The total sample count at the last feed. Unchanged means no new audio.
    last_written: u64,
    samples: Vec<f32>,
    bytes: Vec<u8>,
    /// `TUBEAMP_LOG` is set: the page's stats are printed.
    log: bool,
}

impl Default for MilkdropWindow {
    /// A window that does not exist yet. [`MilkdropWindow::create`] makes it real.
    fn default() -> Self {
        MilkdropWindow {
            view: None,
            win: None,
            sink: Rc::new(|_: Msg| {}),
            ready: false,
            visible: false,
            placed: false,
            steps: DEFAULT_STEPS,
            focused: false,
            fullscreen: false,
            random: false,
            modifiers: ModifiersState::empty(),
            press: None,
            resize: None,
            next_feed: Instant::now(),
            last_written: 0,
            samples: Vec::new(),
            bytes: Vec::new(),
            log: std::env::var_os("TUBEAMP_LOG").is_some(),
        }
    }
}

impl MilkdropWindow {
    /// Creates the hidden window at the default size. `sink` gets the page's messages, on the
    /// event loop's thread.
    pub fn create(
        event_loop: &ActiveEventLoop,
        zoom: u32,
        sink: impl Fn(Msg) + 'static,
    ) -> Result<MilkdropWindow, String> {
        let (w, h) = gen::window_size(DEFAULT_STEPS);
        let attributes = WindowAttributes::default()
            .with_title("TubeAmp Milkdrop")
            .with_decorations(false)
            // Not resizable by the OS: the corner grip resizes it in steps, as the library does.
            .with_resizable(false)
            .with_visible(false)
            .with_inner_size(LogicalSize::new(w * zoom, h * zoom));
        let win = SkinnedWindow::new(event_loop, attributes, (w, h), zoom)?;
        Ok(MilkdropWindow {
            win: Some(win),
            sink: Rc::new(sink),
            ..MilkdropWindow::default()
        })
    }

    pub fn window(&self) -> Option<&SkinnedWindow> {
        self.win.as_ref()
    }

    pub fn window_mut(&mut self) -> Option<&mut SkinnedWindow> {
        self.win.as_mut()
    }

    /// Whether `id` is this window's.
    pub fn owns(&self, id: WindowId) -> bool {
        self.win.as_ref().is_some_and(|w| w.id() == id)
    }

    pub fn visible(&self) -> bool {
        self.visible
    }

    /// The frame's size in resize steps.
    pub fn steps(&self) -> (u32, u32) {
        self.steps
    }

    /// Resizes the frame to `steps`. The webview follows.
    pub fn set_steps(&mut self, steps: (u32, u32)) {
        self.steps = steps;
        if let Some(win) = self.win.as_mut() {
            win.set_base(gen::window_size(steps));
        }
        self.update_bounds();
        self.request_redraw();
    }

    /// Shows the window, and starts the page's animation. The first time, it goes at `beside`
    /// (the physical position beside the main window, if the app knows it). The webview is made
    /// on the first show, while the window is visible.
    pub fn show(&mut self, beside: Option<(i32, i32)>) {
        let Some(win) = self.win.as_ref() else {
            return;
        };
        if !self.placed {
            if let Some((x, y)) = beside {
                win.window.set_outer_position(PhysicalPosition::new(x, y));
                self.placed = true;
            }
        }
        win.window.set_visible(true);
        self.visible = true;
        if self.view.is_none() {
            match self.make_view() {
                Ok(view) => self.view = Some(view),
                Err(e) => {
                    eprintln!("tubeamp: {e}");
                    return;
                }
            }
        }
        self.update_bounds();
        self.request_redraw();
        self.sync_page();
    }

    /// Hides the window. The page stops animating; the webview stays, so showing it again is
    /// quick. Fullscreen is left first, so a hidden window never stays fullscreen.
    pub fn hide(&mut self) {
        if self.fullscreen {
            self.set_fullscreen(false);
        }
        self.visible = false;
        self.press = None;
        self.resize = None;
        if let Some(win) = self.win.as_ref() {
            win.window.set_visible(false);
        }
        self.sync_page();
    }

    /// Runs a menu command or an O menu item.
    pub fn command(&mut self, command: Command) {
        match command {
            Command::Next => self.run("window.tubeamp&&window.tubeamp.next()"),
            Command::Previous => self.run("window.tubeamp&&window.tubeamp.prev()"),
            Command::Random => {
                self.random = !self.random;
                self.run(&format!(
                    "window.tubeamp&&window.tubeamp.setRandom({})",
                    self.random
                ));
            }
            Command::Fullscreen => self.toggle_fullscreen(),
            Command::Close => self.hide(),
        }
    }

    /// A message from the page. A `key` message is for the app, which runs it as a page key.
    pub fn on_msg(&mut self, msg: Msg) {
        match msg {
            Msg::Ready => {
                self.ready = true;
                self.sync_page();
            }
            Msg::Fullscreen => self.toggle_fullscreen(),
            Msg::Escape => {
                if self.fullscreen {
                    self.toggle_fullscreen();
                }
            }
            Msg::Random { on } => self.random = on,
            Msg::Menu { x, y } => {
                let (ox, oy) = self.view_origin();
                self.show_menu(ox + x, oy + y);
            }
            Msg::Key { .. } => {}
            Msg::Stats { fps, feeds, size } => {
                if self.log {
                    eprintln!("tubeamp: Milkdrop {fps} fps, {feeds} audio feeds/s; {size}");
                }
            }
            Msg::Error { msg } => eprintln!("tubeamp: Milkdrop: {msg}"),
        }
    }

    /// A window event of the Milkdrop window. Returns the move the app must carry out, if any.
    pub fn event(&mut self, event: WindowEvent) -> Option<Effect> {
        match event {
            // Closing the window hides it. The playback and the page's loop follow the window.
            WindowEvent::CloseRequested => self.hide(),
            WindowEvent::Resized(_) | WindowEvent::ScaleFactorChanged { .. } => {
                self.update_bounds();
                self.request_redraw();
            }
            WindowEvent::Focused(focused) => {
                self.focused = focused;
                self.request_redraw();
            }
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::CursorMoved { position, .. } => {
                if let Some(win) = self.win.as_mut() {
                    win.set_cursor(Some(position));
                }
                return if self.resize.is_some() {
                    self.resize_to_cursor()
                } else {
                    Some(Effect::DragMoved)
                };
            }
            WindowEvent::CursorLeft { .. } => {
                if let Some(win) = self.win.as_mut() {
                    win.set_cursor(None);
                }
            }
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => {
                return match state {
                    ElementState::Pressed => self.press_frame(),
                    ElementState::Released => self.release_frame(),
                };
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Right,
                ..
            } => self.frame_context_menu(),
            WindowEvent::KeyboardInput { event, .. } => self.native_key(&event),
            _ => {}
        }
        None
    }

    /// Draws the frame. The client area is left as it is: the webview covers it.
    pub fn redraw(&mut self, skin: &Skin) {
        let canvas = self.frame_canvas(skin);
        if let Some(win) = self.win.as_mut() {
            win.present(&canvas);
        }
    }

    /// Feeds the page the newest audio, about every 16 ms while the window shows. With no new
    /// audio (or no engine) the page gets silence.
    pub fn tick(&mut self, now: Instant, audio: Option<&AudioEngine>) {
        if !self.visible || !self.ready || self.view.is_none() || now < self.next_feed {
            return;
        }
        self.next_feed = now + FEED_INTERVAL;
        let fresh = match audio {
            Some(engine) => {
                let written = engine.copy_latest(&mut self.samples, FEED_SAMPLES);
                let fresh = written != self.last_written;
                self.last_written = written;
                fresh
            }
            None => false,
        };
        if !fresh {
            self.samples.clear();
        }
        time_bytes(&self.samples, &mut self.bytes);
        let script = format!(
            "window.tubeamp&&window.tubeamp.audio(\"{}\")",
            base64(&self.bytes)
        );
        self.run(&script);
    }

    /// When the next audio feed is due, if one is needed now.
    pub fn next_wake(&self) -> Option<Instant> {
        (self.visible && self.ready && self.view.is_some()).then_some(self.next_feed)
    }

    /// With `--screenshot-after`: the frame as drawn (`OUT-milkdrop.png`), and the real window
    /// with the webview (`OUT-milkdrop-live.png`, macOS, screen recording permission needed).
    pub fn save_screenshots(&self, skin: &Skin, path: &Path) {
        if !self.visible {
            return;
        }
        let frame = self.frame_canvas(skin);
        let out = suffixed_path(path, "milkdrop");
        save_one(&scaled(&frame, 2), frame.w * 2, frame.h * 2, &out);
        if let Some(number) = self
            .win
            .as_ref()
            .and_then(|w| skinned::window_number(&w.window))
        {
            let live = suffixed_path(path, "milkdrop-live");
            match skinned::capture_window(number, &live) {
                Ok(()) => eprintln!("tubeamp: wrote {}", live.display()),
                Err(e) => eprintln!("tubeamp: could not capture the Milkdrop window: {e}"),
            }
        }
    }

    // Internals.

    /// The frame as drawn at the current size.
    fn frame_canvas(&self, skin: &Skin) -> Canvas {
        let (w, h) = gen::window_size(self.steps);
        let mut canvas = Canvas::new(w, h);
        gen::render_frame(skin, self.steps, TITLE, self.focused, &mut canvas);
        canvas
    }

    fn request_redraw(&self) {
        if let Some(win) = self.win.as_ref() {
            win.request_redraw();
        }
    }

    /// The webview's rectangle in logical pixels of the window: the frame's client area, or the
    /// whole window in fullscreen.
    fn view_rect(&self, win: &SkinnedWindow) -> (f64, f64, f64, f64) {
        if self.fullscreen {
            let size = win.window.inner_size().to_logical::<f64>(win.window.scale_factor());
            return (0.0, 0.0, size.width, size.height);
        }
        let zoom = f64::from(win.zoom());
        let r = gen::client_rect(self.steps);
        (
            f64::from(r.x) * zoom,
            f64::from(r.y) * zoom,
            f64::from(r.w) * zoom,
            f64::from(r.h) * zoom,
        )
    }

    /// Where the webview's top-left corner is, in logical pixels of the window.
    fn view_origin(&self) -> (f64, f64) {
        match self.win.as_ref() {
            Some(win) => {
                let (x, y, _, _) = self.view_rect(win);
                (x, y)
            }
            None => (0.0, 0.0),
        }
    }

    fn update_bounds(&self) {
        let (Some(view), Some(win)) = (self.view.as_ref(), self.win.as_ref()) else {
            return;
        };
        let (x, y, w, h) = self.view_rect(win);
        let rect = Rect {
            position: wry::dpi::LogicalPosition::new(x, y).into(),
            size: wry::dpi::LogicalSize::new(w.max(0.0), h.max(0.0)).into(),
        };
        if let Err(e) = view.set_bounds(rect) {
            eprintln!("tubeamp: could not place the Milkdrop view: {e}");
        }
    }

    /// Builds the webview as a child of the window, showing the page.
    fn make_view(&self) -> Result<WebView, String> {
        let win = self
            .win
            .as_ref()
            .ok_or_else(|| "the Milkdrop window does not exist".to_string())?;
        let (x, y, w, h) = self.view_rect(win);
        let sink = Rc::clone(&self.sink);
        WebViewBuilder::new()
            .with_custom_protocol(SCHEME.to_string(), |_id, request| {
                serve(request.uri().path())
            })
            .with_url(ENTRY_URL)
            .with_background_color((0, 0, 0, 255))
            .with_ipc_handler(move |request: Request<String>| {
                match serde_json::from_str::<Msg>(request.body()) {
                    Ok(msg) => sink(msg),
                    Err(e) => eprintln!(
                        "tubeamp: unrecognised Milkdrop message ({e}): {}",
                        request.body()
                    ),
                }
            })
            .with_bounds(Rect {
                position: wry::dpi::LogicalPosition::new(x, y).into(),
                size: wry::dpi::LogicalSize::new(w.max(0.0), h.max(0.0)).into(),
            })
            .build_as_child(&*win.window)
            .map_err(|e| format!("could not create the Milkdrop view: {e}"))
    }

    /// Runs `script` in the page, if the webview exists.
    fn run(&self, script: &str) {
        let Some(view) = self.view.as_ref() else {
            return;
        };
        if let Err(e) = view.evaluate_script(script) {
            if self.log {
                eprintln!("tubeamp: could not run a Milkdrop script: {e}");
            }
        }
    }

    /// Tells the page whether it shows, and the random cycling state. Only once it is ready.
    fn sync_page(&self) {
        if !self.ready {
            return;
        }
        self.run(&format!(
            "window.tubeamp&&window.tubeamp.setActive({})",
            self.visible
        ));
        self.run(&format!(
            "window.tubeamp&&window.tubeamp.setRandom({})",
            self.random
        ));
    }

    fn set_fullscreen(&mut self, on: bool) {
        self.fullscreen = on;
        if let Some(win) = self.win.as_mut() {
            win.set_fullscreen(on);
        }
        self.update_bounds();
        self.request_redraw();
    }

    fn toggle_fullscreen(&mut self) {
        let on = !self.fullscreen;
        self.set_fullscreen(on);
    }

    /// The skin point under the pointer, as a frame hit.
    fn frame_hit(&self) -> Option<GenHit> {
        let (x, y) = self.win.as_ref()?.skin_pixel()?;
        gen::hit_test_gen(self.steps, x, y)
    }

    fn press_frame(&mut self) -> Option<Effect> {
        let hit = self.frame_hit()?;
        match hit {
            GenHit::TitleBar => {
                self.press = Some(hit);
                Some(Effect::BeginDrag)
            }
            GenHit::Close => {
                self.press = Some(hit);
                None
            }
            GenHit::Resize => {
                let cursor = self.win.as_ref()?.cursor_position()?;
                self.resize = Some(Resize {
                    start_steps: self.steps,
                    start_cursor: (cursor.x, cursor.y),
                });
                None
            }
            GenHit::Client | GenHit::Frame => None,
        }
    }

    fn release_frame(&mut self) -> Option<Effect> {
        let press = self.press.take();
        let hit = self.frame_hit();
        self.resize = None;
        if press == Some(GenHit::Close) && hit == Some(GenHit::Close) {
            self.hide();
        }
        (press == Some(GenHit::TitleBar)).then_some(Effect::EndDrag)
    }

    /// The size the pointer asks for while the grip is dragged. The app snaps it.
    fn resize_to_cursor(&self) -> Option<Effect> {
        let drag = self.resize?;
        let win = self.win.as_ref()?;
        let cursor = win.cursor_position()?;
        let factor = f64::from(win.factor());
        let (w0, h0) = gen::window_size(drag.start_steps);
        let w = f64::from(w0) + (cursor.x - drag.start_cursor.0) / factor;
        let h = f64::from(h0) + (cursor.y - drag.start_cursor.1) / factor;
        Some(Effect::Resize(w, h))
    }

    /// A right press on the frame (the page handles right presses in its own area).
    fn frame_context_menu(&self) {
        let Some(win) = self.win.as_ref() else {
            return;
        };
        let Some((x, y)) = win.skin_point() else {
            return;
        };
        let zoom = f64::from(win.zoom());
        self.show_menu(x * zoom, y * zoom);
    }

    /// Shows the right-click menu at `(x, y)` in logical pixels of the window.
    fn show_menu(&self, x: f64, y: f64) {
        let Some(win) = self.win.as_ref() else {
            return;
        };
        match build_menu(self.random) {
            Ok(menu) => {
                if let Err(e) = skinned::show_context_menu(&win.window, &menu, x, y) {
                    eprintln!("tubeamp: could not show the menu: {e}");
                }
            }
            Err(e) => eprintln!("tubeamp: could not build the menu: {e}"),
        }
    }

    /// A key press while the native window has the focus (not the page). It goes to the page, so
    /// the Milkdrop keys behave the same whichever part has the focus. Key repeats move only the
    /// arrows.
    fn native_key(&self, event: &KeyEvent) {
        if event.state != ElementState::Pressed {
            return;
        }
        let Some((key, code)) = web_key(event) else {
            return;
        };
        if event.repeat && !code.starts_with("Arrow") {
            return;
        }
        let script = format!(
            "window.tubeamp&&window.tubeamp.key({}, {}, {}, {}, {})",
            js_string(&key),
            js_string(&code),
            self.modifiers.control_key(),
            self.modifiers.alt_key(),
            self.modifiers.super_key(),
        );
        self.run(&script);
    }
}

/// The page's `KeyboardEvent.key` and `code` for a native key press, for the keys the page uses.
/// Letters and digits get a `code` (`KeyN`, `Digit5`); other keys get their name.
fn web_key(event: &KeyEvent) -> Option<(String, String)> {
    let named = |name: &str| Some((name.to_string(), name.to_string()));
    match event.key_without_modifiers() {
        Key::Named(NamedKey::Space) => Some((" ".to_string(), "Space".to_string())),
        Key::Named(NamedKey::Backspace) => named("Backspace"),
        Key::Named(NamedKey::Escape) => named("Escape"),
        Key::Named(NamedKey::ArrowLeft) => named("ArrowLeft"),
        Key::Named(NamedKey::ArrowRight) => named("ArrowRight"),
        Key::Named(NamedKey::ArrowUp) => named("ArrowUp"),
        Key::Named(NamedKey::ArrowDown) => named("ArrowDown"),
        Key::Character(c) => {
            let mut chars = c.chars();
            let (Some(ch), None) = (chars.next(), chars.next()) else {
                return None;
            };
            let code = match ch {
                ' ' => "Space".to_string(),
                ch if ch.is_ascii_alphabetic() => format!("Key{}", ch.to_ascii_uppercase()),
                ch if ch.is_ascii_digit() => format!("Digit{ch}"),
                _ => String::new(),
            };
            Some((c.to_string(), code))
        }
        _ => None,
    }
}

/// `s` as a JavaScript string literal (JSON quoting is valid JavaScript).
fn js_string(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

/// The page for `path`, or a 404. The bodies are compiled in, so they are never copied.
fn serve(path: &str) -> Response<Cow<'static, [u8]>> {
    match ASSETS.iter().find(|a| a.path == path) {
        Some(asset) => Response::builder()
            .status(200)
            .header(CONTENT_TYPE, asset.mime)
            .body(Cow::Borrowed(asset.bytes))
            .expect("a static response is valid"),
        None => Response::builder()
            .status(404)
            .header(CONTENT_TYPE, "text/plain; charset=utf-8")
            .body(Cow::Borrowed(&b"not found"[..]))
            .expect("a static response is valid"),
    }
}

/// The newest `FEED_SAMPLES` of `samples` as butterchurn's time-domain bytes: 128 is silence and
/// ±1.0 maps to 1 and 255. Missing samples at the start are silence, and the rest are right-aligned.
pub fn time_bytes(samples: &[f32], out: &mut Vec<u8>) {
    out.clear();
    out.resize(FEED_SAMPLES, 128);
    let n = samples.len().min(FEED_SAMPLES);
    let newest = &samples[samples.len() - n..];
    for (dst, &s) in out[FEED_SAMPLES - n..].iter_mut().zip(newest) {
        let s = if s.is_finite() { s.clamp(-1.0, 1.0) } else { 0.0 };
        *dst = (128.0 + 127.0 * s).round() as u8;
    }
}

/// Standard base64 (RFC 4648, with padding), for the audio feed.
pub fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = chunk[0];
        let b1 = chunk.get(1).copied().unwrap_or(0);
        let b2 = chunk.get(2).copied().unwrap_or(0);
        let n = (u32::from(b0) << 16) | (u32::from(b1) << 8) | u32::from(b2);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(ALPHABET[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

/// Saves a `0x00RRGGBB` image, printing the outcome as the other windows' saves do.
fn save_one(pixels: &[u32], width: u32, height: u32, path: &Path) {
    match crate::save_png(pixels, width, height, path) {
        Ok(()) => eprintln!("tubeamp: wrote {}", path.display()),
        Err(e) => eprintln!("tubeamp: could not write {}: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_rfc_vectors() {
        let cases: [(&[u8], &str); 7] = [
            (b"", ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
        ];
        for (input, want) in cases {
            assert_eq!(base64(input), want, "input {input:?}");
        }
    }

    #[test]
    fn silence_is_128_and_full_scale_maps_to_the_ends() {
        let mut out = Vec::new();
        time_bytes(&[0.0, 1.0, -1.0, 2.0, f32::NAN], &mut out);
        assert_eq!(out.len(), FEED_SAMPLES);
        // Right-aligned: the five samples are the last five bytes, and the rest is silence.
        assert!(out[..FEED_SAMPLES - 5].iter().all(|&b| b == 128));
        assert_eq!(&out[FEED_SAMPLES - 5..], &[128, 255, 1, 255, 128]);
    }

    #[test]
    fn the_newest_samples_are_kept_when_there_are_too_many() {
        let samples: Vec<f32> = (0..FEED_SAMPLES + 3).map(|k| if k < 3 { 1.0 } else { 0.0 }).collect();
        let mut out = Vec::new();
        time_bytes(&samples, &mut out);
        assert_eq!(out.len(), FEED_SAMPLES);
        // The three full-scale samples were the oldest, so they are gone.
        assert!(out.iter().all(|&b| b == 128));
    }

    #[test]
    fn an_empty_feed_is_all_silence() {
        let mut out = vec![7u8; 3];
        time_bytes(&[], &mut out);
        assert_eq!(out, vec![128u8; FEED_SAMPLES]);
    }

    #[test]
    fn menu_ids_map_back_to_their_commands() {
        assert_eq!(command_for_id(MENU_NEXT), Some(Command::Next));
        assert_eq!(command_for_id(MENU_PREVIOUS), Some(Command::Previous));
        assert_eq!(command_for_id(MENU_RANDOM), Some(Command::Random));
        assert_eq!(command_for_id(MENU_FULLSCREEN), Some(Command::Fullscreen));
        assert_eq!(command_for_id(MENU_CLOSE), Some(Command::Close));
        assert_eq!(command_for_id("lib.play"), None);
    }

    #[test]
    fn the_page_messages_parse() {
        let parse = |s: &str| serde_json::from_str::<Msg>(s).expect("message parses");
        assert_eq!(parse(r#"{"t":"ready","presets":300}"#), Msg::Ready);
        assert_eq!(parse(r#"{"t":"fullscreen"}"#), Msg::Fullscreen);
        assert_eq!(parse(r#"{"t":"escape"}"#), Msg::Escape);
        assert_eq!(parse(r#"{"t":"random","on":true}"#), Msg::Random { on: true });
        assert_eq!(
            parse(r#"{"t":"menu","x":12.5,"y":3}"#),
            Msg::Menu { x: 12.5, y: 3.0 }
        );
        assert_eq!(
            parse(r#"{"t":"key","key":"z","code":"KeyZ","ctrl":false,"alt":true,"meta":false,"shift":true}"#),
            Msg::Key {
                key: "z".into(),
                code: "KeyZ".into(),
                ctrl: false,
                alt: true,
                meta: false,
            }
        );
        assert_eq!(
            parse(r#"{"t":"stats","fps":60,"feeds":60}"#),
            Msg::Stats {
                fps: 60.0,
                feeds: 60.0,
                size: String::new(),
            }
        );
        assert_eq!(
            parse(r#"{"t":"stats","fps":59,"feeds":58,"size":"canvas 550x464 viewport 550x464 css 275x232 dpr 2"}"#),
            Msg::Stats {
                fps: 59.0,
                feeds: 58.0,
                size: "canvas 550x464 viewport 550x464 css 275x232 dpr 2".into(),
            }
        );
        assert_eq!(
            parse(r#"{"t":"error","msg":"boom"}"#),
            Msg::Error { msg: "boom".into() }
        );
        assert!(serde_json::from_str::<Msg>(r#"{"t":"nope"}"#).is_err());
    }

    #[test]
    fn the_page_and_its_scripts_are_served() {
        let page = serve("/milkdrop.html");
        assert_eq!(page.status(), 200);
        assert_eq!(page.headers()[CONTENT_TYPE], "text/html; charset=utf-8");
        assert_eq!(serve("/missing.js").status(), 404);
        // Every script the page loads is one of the served assets.
        let html = std::str::from_utf8(ASSETS[0].bytes).expect("the page is UTF-8");
        for line in html.lines().filter(|l| l.contains("<script src=")) {
            let src = line.split('"').nth(1).expect("a src attribute");
            assert!(
                ASSETS.iter().any(|a| a.path == format!("/{src}")),
                "{src} is not served"
            );
        }
    }

    #[test]
    fn js_strings_are_quoted_for_javascript() {
        assert_eq!(js_string(" "), "\" \"");
        assert_eq!(js_string("a\"b"), "\"a\\\"b\"");
    }
}
