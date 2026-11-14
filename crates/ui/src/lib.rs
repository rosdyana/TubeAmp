pub mod canvas;
pub mod labels;
pub mod main_window;
pub mod sliders;
pub mod text;
pub mod vis;

pub use canvas::{Canvas, Rect};
pub use main_window::{
    displayed_secs, hit_test_main, render_main, time_digits, MainButton, MainHit, MainState,
    Status, HEIGHT, WIDTH,
};
pub use text::{draw_text, marquee_frame, marquee_period_px};
pub use vis::{render_vis, VisMode, VIS_RECT};
