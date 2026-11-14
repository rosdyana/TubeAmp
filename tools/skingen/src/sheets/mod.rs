//! Sheet renderers, one module per sheet (`NUMBERS` and `NUMS_EX` share `numbers`).

mod balance;
mod cbuttons;
mod eq_ex;
mod eqmain;
mod gen;
mod genex;
mod main;
mod monoster;
mod numbers;
mod playpaus;
mod pledit;
mod posbar;
mod shufrep;
mod text;
mod titlebar;
mod video;
mod volume;

use image::RgbImage;

/// Every bitmap sheet, as `(base name, art)`, in archive order. The file name is
/// `<base name>.BMP`.
pub fn render_all() -> Vec<(&'static str, RgbImage)> {
    vec![
        ("MAIN", main::render().into_rgb()),
        ("TITLEBAR", titlebar::render().into_rgb()),
        ("CBUTTONS", cbuttons::render().into_rgb()),
        ("NUMBERS", numbers::render_numbers().into_rgb()),
        ("NUMS_EX", numbers::render_nums_ex().into_rgb()),
        ("PLAYPAUS", playpaus::render().into_rgb()),
        ("MONOSTER", monoster::render().into_rgb()),
        ("POSBAR", posbar::render().into_rgb()),
        ("VOLUME", volume::render().into_rgb()),
        ("BALANCE", balance::render().into_rgb()),
        ("SHUFREP", shufrep::render().into_rgb()),
        ("TEXT", text::render().into_rgb()),
        ("PLEDIT", pledit::render().into_rgb()),
        ("EQMAIN", eqmain::render().into_rgb()),
        ("EQ_EX", eq_ex::render().into_rgb()),
        ("GEN", gen::render().into_rgb()),
        ("GENEX", genex::render().into_rgb()),
        ("VIDEO", video::render().into_rgb()),
    ]
}
