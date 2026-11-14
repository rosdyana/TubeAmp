//! M0 risk spike. Two winit windows:
//! - "host": a wry WebView embedded as a child, loading music.youtube.com
//! - "control": plain winit window that receives keys
//!   Space play/pause · N next · H hide/show host · M minimize host
//!   E next EQ preset · W previous EQ preset (cursor 0 = Flat, then the 17 Winamp presets)
//!   O EQ on/off · = / - preamp +/-1 · [ / ] balance -/+25 · Q quit
//! The bridge prints state lines once per second to stdout.

use tubeamp_eq::{EqSettings, BUILTIN};
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::{ElementState, KeyEvent, WindowEvent};
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};
use wry::{Rect, WebView, WebViewBuilder};

#[cfg(target_os = "macos")]
mod tap;

const BRIDGE: &str = include_str!("bridge.js");
const UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Safari/605.1.15";
/// Preset cursor size: index 0 = "Flat", then each built-in preset.
const EQ_COUNT: usize = 1 + BUILTIN.len();

/// Name and settings for preset cursor position `i` (0 = Flat).
fn eq_preset(i: usize) -> (&'static str, EqSettings) {
    if i == 0 {
        ("Flat", EqSettings::default())
    } else {
        (BUILTIN[i - 1].name, EqSettings::from_preset(&BUILTIN[i - 1]))
    }
}

/// Scripted steps for hands-free runs (M0_SCRIPT=<videoId>).
#[derive(Debug)]
enum Step { Load(String), Eq(usize), Hide, Show, Minimize, Quit, Playing }

struct App {
    proxy: Option<EventLoopProxy<Step>>,
    host: Option<Window>,
    control: Option<Window>,
    webview: Option<WebView>,
    host_visible: bool,
    /// Preset cursor (0 = Flat).
    eq: usize,
    /// Current EQ state (on, preamp, bands).
    settings: EqSettings,
    /// Balance -100..=100 (tap mode only).
    balance: i8,
    tap: bool,
    tap_started: bool,
}

impl App {
    /// Pushes the current EQ state to the active backend.
    fn apply_eq(&self) {
        if self.tap {
            tap::PARAMS.set(&self.settings);
            tap::PARAMS.set_balance(self.balance);
        } else {
            let s = self.settings;
            let gains: [f32; 10] = std::array::from_fn(|i| if s.on { s.band_db(i) } else { 0.0 });
            self.send(serde_json::json!({"t": "eq", "gains": gains}));
        }
    }

    /// Moves the preset cursor to `i` (wrapping) and loads that preset's preamp and bands.
    /// The on/off toggle and balance are kept across preset changes.
    fn select(&mut self, i: usize) {
        self.eq = i % EQ_COUNT;
        let (name, preset) = eq_preset(self.eq);
        self.settings = EqSettings { on: self.settings.on, ..preset };
        let mode = if self.tap { "native tap" } else { "web audio" };
        println!("[spike] EQ preset -> {name} ({mode})");
        let bands: Vec<String> = (0..10).map(|i| format!("{:+.0}", self.settings.band_db(i))).collect();
        println!("[spike]   bands dB (60 170 310 600 1k 3k 6k 12k 14k 16k): {}", bands.join(" "));
        self.apply_eq();
    }

    fn send(&self, js_cmd: serde_json::Value) {
        if let Some(wv) = &self.webview {
            let _ = wv.evaluate_script(&format!("window.__tubeamp && window.__tubeamp.cmd({js_cmd})"));
        }
    }
}

impl ApplicationHandler<Step> for App {
    fn user_event(&mut self, el: &ActiveEventLoop, step: Step) {
        println!("[script] {step:?}");
        match step {
            Step::Load(id) => self.send(serde_json::json!({"t": "load", "id": id})),
            Step::Eq(i) => self.select(i),
            Step::Playing => {
                #[cfg(target_os = "macos")]
                if self.tap && !self.tap_started {
                    let sr = tap::start();
                    println!("[spike] tap start -> {sr}");
                    self.tap_started = sr > 0;
                }
            }
            Step::Hide => { if let Some(h) = &self.host { h.set_visible(false); } }
            Step::Show => { if let Some(h) = &self.host { h.set_visible(true); } }
            Step::Minimize => { if let Some(h) = &self.host { h.set_minimized(true); } }
            Step::Quit => el.exit(),
        }
    }

    fn resumed(&mut self, el: &ActiveEventLoop) {
        if self.host.is_some() {
            return;
        }
        let host = el
            .create_window(Window::default_attributes().with_title("m0 host (video)").with_inner_size(LogicalSize::new(640.0, 480.0)))
            .unwrap();
        let control = el
            .create_window(Window::default_attributes().with_title("m0 control — Space N H M E Q").with_inner_size(LogicalSize::new(360.0, 80.0)))
            .unwrap();
        let size = host.inner_size().to_logical::<f64>(host.scale_factor());
        let webview = WebViewBuilder::new()
            .with_url("https://music.youtube.com/")
            .with_user_agent(UA)
            .with_autoplay(true)
            .with_initialization_script(if std::env::var("M0_NOGRAPH").is_ok() { "window.__M0_NOGRAPH=1;" } else { "" })
            .with_initialization_script(if std::env::var("M0_NOMSE").is_ok() { "try{delete window.MediaSource;delete window.ManagedMediaSource;delete window.WebKitMediaSource;Object.defineProperty(window,'MediaSource',{value:undefined});Object.defineProperty(window,'ManagedMediaSource',{value:undefined});const d=Object.getOwnPropertyDescriptor(HTMLMediaElement.prototype,'src');Object.defineProperty(HTMLMediaElement.prototype,'src',{get(){return d.get.call(this)},set(v){this.crossOrigin='anonymous';d.set.call(this,v)},configurable:true});}catch(e){}" } else { "" })
            .with_initialization_script(BRIDGE)
            .with_ipc_handler({
                let proxy = self.proxy.clone().unwrap();
                move |req| {
                    let body = req.body();
                    if body.contains("\"ps\":1,") {
                        let _ = proxy.send_event(Step::Playing);
                    }
                    println!("[bridge] {body}");
                }
            })
            .with_bounds(Rect {
                position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
                size: wry::dpi::LogicalSize::new(size.width, size.height).into(),
            })
            .build_as_child(&host)
            .expect("webview");
        self.host = Some(host);
        self.control = Some(control);
        self.webview = Some(webview);
        self.host_visible = true;
    }

    fn window_event(&mut self, el: &ActiveEventLoop, id: WindowId, event: WindowEvent) {
        let is_host = self.host.as_ref().map(|w| w.id()) == Some(id);
        match event {
            WindowEvent::CloseRequested => el.exit(),
            WindowEvent::Resized(sz) if is_host => {
                if let (Some(wv), Some(h)) = (&self.webview, &self.host) {
                    let l = sz.to_logical::<f64>(h.scale_factor());
                    let _ = wv.set_bounds(Rect {
                        position: wry::dpi::LogicalPosition::new(0.0, 0.0).into(),
                        size: wry::dpi::LogicalSize::new(l.width, l.height).into(),
                    });
                }
            }
            WindowEvent::KeyboardInput { event: KeyEvent { logical_key, state: ElementState::Pressed, repeat: false, .. }, .. } if !is_host => {
                match logical_key.as_ref() {
                    Key::Named(NamedKey::Space) => self.send(serde_json::json!({"t": "toggle"})),
                    Key::Character("n") => self.send(serde_json::json!({"t": "next"})),
                    Key::Character("h") => {
                        self.host_visible = !self.host_visible;
                        if let Some(h) = &self.host {
                            h.set_visible(self.host_visible);
                        }
                        println!("[spike] host visible = {}", self.host_visible);
                    }
                    Key::Character("m") => {
                        if let Some(h) = &self.host {
                            h.set_minimized(true);
                        }
                        println!("[spike] host minimized");
                    }
                    Key::Character("e") => self.select(self.eq + 1),
                    Key::Character("w") => self.select(self.eq + EQ_COUNT - 1),
                    Key::Character("o") => {
                        self.settings.on = !self.settings.on;
                        println!("[spike] EQ on = {}", self.settings.on);
                        self.apply_eq();
                    }
                    Key::Character("=") => {
                        self.settings.preamp = self.settings.preamp.saturating_add(1).min(64);
                        println!("[spike] preamp = {} ({:+.1} dB)", self.settings.preamp, self.settings.preamp_db());
                        self.apply_eq();
                    }
                    Key::Character("-") => {
                        self.settings.preamp = self.settings.preamp.saturating_sub(1).max(1);
                        println!("[spike] preamp = {} ({:+.1} dB)", self.settings.preamp, self.settings.preamp_db());
                        self.apply_eq();
                    }
                    Key::Character("[") => {
                        self.balance = self.balance.saturating_sub(25).max(-100);
                        println!("[spike] balance = {} (negative = left, positive = right)", self.balance);
                        self.apply_eq();
                    }
                    Key::Character("]") => {
                        self.balance = self.balance.saturating_add(25).min(100);
                        println!("[spike] balance = {} (negative = left, positive = right)", self.balance);
                        self.apply_eq();
                    }
                    Key::Character("q") => el.exit(),
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn main() {
    let el = EventLoop::<Step>::with_user_event().build().unwrap();
    let proxy = el.create_proxy();
    if let Ok(id) = std::env::var("M0_SCRIPT") {
        let p = proxy.clone();
        std::thread::spawn(move || {
            let plan = [
                (8, Step::Load(id)), (20, Step::Eq(12)), (28, Step::Eq(14)), (36, Step::Eq(0)), (42, Step::Quit),
            ];
            let mut t = 0;
            for (at, step) in plan {
                std::thread::sleep(std::time::Duration::from_secs(at - t));
                t = at;
                let _ = p.send_event(step);
            }
        });
    }
    let tap = std::env::var("M0_TAP").is_ok();
    if tap {
        std::thread::spawn(|| loop {
            std::thread::sleep(std::time::Duration::from_secs(2));
            let db = |a: &std::sync::atomic::AtomicU32| 10.0 * f32::from_bits(a.load(std::sync::atomic::Ordering::Relaxed)).max(1e-12).log10();
            let s = &tap::PARAMS;
            println!("[dsp] in={:.1}dB out={:.1}dB low={:.1}dB mid={:.1}dB", db(&s.in_sq), db(&s.out_sq), db(&s.low_sq), db(&s.mid_sq));
        });
    }
    let mut app = App {
        proxy: Some(proxy),
        host: None,
        control: None,
        webview: None,
        host_visible: false,
        eq: 0,
        settings: EqSettings::default(),
        balance: 0,
        tap,
        tap_started: false,
    };
    el.run_app(&mut app).unwrap();
}
