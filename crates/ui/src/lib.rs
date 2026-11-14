pub mod canvas;
pub mod eq_window;
pub mod labels;
pub mod main_window;
pub mod sliders;
pub mod text;
pub mod vis;

pub use canvas::{Canvas, Rect};
pub use eq_window::{
    hit_test_eq, press_grab, render_eq, slider_rect, slider_value_at, EqHit, EqSlider,
    EqWindowState, PRESETS_BTN,
};
pub use main_window::{
    displayed_secs, hit_test_main, render_main, time_digits, MainButton, MainHit, MainState,
    Status, HEIGHT, WIDTH,
};
pub use text::{draw_text, marquee_frame, marquee_period_px};
pub use vis::{render_vis, VisMode, VIS_RECT};
