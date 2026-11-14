//! Native frameless window: softbuffer presentation plus mouse and keyboard handling.

use std::num::NonZeroU32;
use std::rc::Rc;
use std::time::{Duration, Instant};

use softbuffer::{Context, Surface};
use tubeamp_skin::Skin;
use tubeamp_ui::{hit_test_main, MainButton, MainHit, MainState, HEIGHT, WIDTH};
use winit::application::ApplicationHandler;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event::{ElementState, KeyEvent, MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, ModifiersState};
use winit::platform::modifier_supplement::KeyEventExtModifierSupplement;
use winit::window::{Window, WindowAttributes, WindowId};

use crate::{demo, render_scaled};

/// Marquee step: one character cell, as in Webamp's `Marquee.tsx`.
const MARQUEE_STEP_PX: u32 = 5;
/// Marquee interval: Webamp steps the marquee every 220 ms.
const MARQUEE_INTERVAL: Duration = Duration::from_millis(220);
const CLOCK_INTERVAL: Duration = Duration::from_secs(1);

type SharedWindow = Rc<Window>;

struct App {
    skin: Skin,
    state: MainState,
    zoom: u32,
    window: Option<SharedWindow>,
    context: Option<Context<SharedWindow>>,
    surface: Option<Surface<SharedWindow, SharedWindow>>,
    modifiers: ModifiersState,
    cursor: Option<PhysicalPosition<f64>>,
    press_target: Option<MainHit>,
    next_marquee: Instant,
    next_clock: Instant,
}

/// Opens the window and runs the event loop until the window is closed.
pub fn run(skin: Skin, zoom: u32) -> Result<(), winit::error::EventLoopError> {
    let event_loop = EventLoop::new()?;
    let now = Instant::now();
    let mut app = App {
        skin,
        state: demo::demo_state(0, 0, true),
        zoom,
        window: None,
        context: None,
        surface: None,
        modifiers: ModifiersState::empty(),
        cursor: None,
        press_target: None,
        next_marquee: now + MARQUEE_INTERVAL,
        next_clock: now + CLOCK_INTERVAL,
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

    /// Maps the cursor to main-window pixel coordinates (None outside the image).
    fn cursor_main(&self) -> Option<(i32, i32)> {
        let pos = self.cursor?;
        let size = self.window.as_ref()?.inner_size();
        let f = self.factor();
        let (ox, cw) = fit(size.width, WIDTH * f);
        let (oy, ch) = fit(size.height, HEIGHT * f);
        let x = pos.x - f64::from(ox);
        let y = pos.y - f64::from(oy);
        if x < 0.0 || y < 0.0 || x >= f64::from(cw) || y >= f64::from(ch) {
            return None;
        }
        Some(((x as u32 / f) as i32, (y as u32 / f) as i32))
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
                self.request_redraw();
            }
            _ => {}
        }
    }

    fn on_release(&mut self, event_loop: &ActiveEventLoop) {
        let target = self.press_target.take();
        let hit = self.hit_under_cursor();
        self.state.pressed = None;
        match (target, hit) {
            (
                Some(MainHit::Button(MainButton::Close)),
                Some(MainHit::Button(MainButton::Close)),
            ) => {
                event_loop.exit();
            }
            (Some(MainHit::Time), Some(MainHit::Time)) => {
                self.state.remaining_mode = !self.state.remaining_mode;
            }
            _ => {}
        }
        self.request_redraw();
    }

    fn on_key(&mut self, event: &KeyEvent) {
        if event.state != ElementState::Pressed || event.repeat || !self.modifiers.control_key() {
            return;
        }
        match event.key_without_modifiers() {
            Key::Character(c) if c.eq_ignore_ascii_case("d") => self.toggle_double_size(),
            Key::Character(c) if c.eq_ignore_ascii_case("t") => {
                self.state.remaining_mode = !self.state.remaining_mode;
                self.request_redraw();
            }
            _ => {}
        }
    }

    fn toggle_double_size(&mut self) {
        self.zoom = if self.zoom == 1 { 2 } else { 1 };
        if let Some(window) = &self.window {
            let _ = window.request_inner_size(self.logical_size());
        }
        self.request_redraw();
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
        let frame = render_scaled(&self.skin, &self.state, f);
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

    /// Steps the marquee and the demo clock when due, then sleeps until the next deadline.
    fn tick(&mut self, event_loop: &ActiveEventLoop) {
        let now = Instant::now();
        if now >= self.next_marquee {
            let period = tubeamp_ui::marquee_period_px(&self.state.title);
            if period > 0 {
                self.state.marquee_offset = (self.state.marquee_offset + MARQUEE_STEP_PX) % period;
            }
            self.next_marquee = now + MARQUEE_INTERVAL;
            self.request_redraw();
        }
        if now >= self.next_clock {
            demo::advance_clock(&mut self.state);
            self.next_clock = now + CLOCK_INTERVAL;
            self.request_redraw();
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(
            self.next_marquee.min(self.next_clock),
        ));
    }
}

impl ApplicationHandler for App {
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
        self.window = Some(window);
        self.context = Some(context);
        self.surface = Some(surface);
        self.request_redraw();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window: WindowId,
        event: WindowEvent,
    ) {
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
            WindowEvent::CursorMoved { position, .. } => self.cursor = Some(position),
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
