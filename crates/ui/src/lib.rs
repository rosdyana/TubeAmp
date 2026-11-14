//! Winamp-style main window rendering for TubeAmp.
//!
//! - [`canvas`]: the 1× pixel buffer and sprite blitting.
//! - [`text`]: TEXT.BMP font rendering and the marquee.
//! - [`main_window`]: the 275×116 main window (rendering and hit testing).

pub mod canvas;
pub mod main_window;
pub mod text;

pub use canvas::{Canvas, Rect};
pub use main_window::{
    displayed_secs, hit_test_main, render_main, time_digits, MainButton, MainHit, MainState,
    Status, HEIGHT, WIDTH,
};
pub use text::{draw_text, marquee_frame, marquee_period_px};
