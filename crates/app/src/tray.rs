//! The tray icon (the menu bar item on macOS) and its menu. The icon is drawn in code: a tube with
//! a lightning bolt inside, as an alpha mask (a template image on macOS, so the menu bar tints it).
//!
//! Menu clicks come back through muda's menu events, with the ids below, and run in `session.rs`.

/// Side of the icon in pixels. macOS shows it at 22 points, so 44 is sharp on retina screens.
pub const ICON_PX: u32 = 44;

/// Menu ids.
pub const ID_NOW: &str = "tray.now";
pub const ID_PLAY_PAUSE: &str = "tray.playpause";
pub const ID_NEXT: &str = "tray.next";
pub const ID_PREVIOUS: &str = "tray.previous";
pub const ID_STOP: &str = "tray.stop";
pub const ID_SHOW_HIDE: &str = "tray.showhide";
pub const ID_LIBRARY: &str = "tray.library";
pub const ID_QUIT: &str = "tray.quit";

/// The label of the disabled first line when nothing is loaded.
pub const NOTHING_PLAYING: &str = "Nothing playing";

/// The first menu line for a track: `Artist - Title`, or the title alone.
pub fn now_line(title: &str, artist: &str) -> String {
    let title = title.trim();
    if title.is_empty() {
        return NOTHING_PLAYING.to_string();
    }
    let artist = artist.trim();
    if artist.is_empty() {
        title.to_string()
    } else {
        format!("{artist} - {title}")
    }
}

/// The icon's pixels as RGBA, `ICON_PX` square. Each pixel's alpha is the share of its 4×4
/// sub-samples that fall inside the drawing, so the edges are smooth. The colour is black on macOS
/// (a template image) and white elsewhere.
pub fn icon_rgba() -> Vec<u8> {
    let color: [u8; 3] = if cfg!(target_os = "macos") {
        [0, 0, 0]
    } else {
        [255, 255, 255]
    };
    let size = ICON_PX as usize;
    let mut out = vec![0u8; size * size * 4];
    const SUB: usize = 4;
    for py in 0..size {
        for px in 0..size {
            let mut hits = 0u32;
            for sy in 0..SUB {
                for sx in 0..SUB {
                    let x = (px as f64 + (sx as f64 + 0.5) / SUB as f64) / size as f64;
                    let y = (py as f64 + (sy as f64 + 0.5) / SUB as f64) / size as f64;
                    if inside_glyph(x, y) {
                        hits += 1;
                    }
                }
            }
            let alpha = (hits * 255 / (SUB * SUB) as u32) as u8;
            let i = (py * size + px) * 4;
            out[i..i + 3].copy_from_slice(&color);
            out[i + 3] = alpha;
        }
    }
    out
}

/// Whether the point (`x`, `y`), in icon units from 0 to 1, is on the glyph.
fn inside_glyph(x: f64, y: f64) -> bool {
    in_tube_outline(x, y) || in_bolt(x, y)
}

/// The tube: a vertical capsule outline, centred, with a stroke about a tenth of the icon wide.
fn in_tube_outline(x: f64, y: f64) -> bool {
    const CX: f64 = 0.5;
    const TOP: f64 = 0.05;
    const BOTTOM: f64 = 0.95;
    const RADIUS: f64 = 0.27;
    const STROKE: f64 = 0.09;
    // Distance to the segment that runs down the capsule's axis.
    let cy = y.clamp(TOP + RADIUS, BOTTOM - RADIUS);
    let dist = ((x - CX).powi(2) + (y - cy).powi(2)).sqrt();
    (dist - (RADIUS - STROKE / 2.0)).abs() <= STROKE / 2.0
}

/// The bolt: a zigzag polygon inside the tube, filled (even-odd rule).
fn in_bolt(x: f64, y: f64) -> bool {
    const BOLT: [(f64, f64); 7] = [
        (0.58, 0.14),
        (0.34, 0.52),
        (0.48, 0.52),
        (0.40, 0.86),
        (0.68, 0.44),
        (0.53, 0.44),
        (0.62, 0.14),
    ];
    let mut inside = false;
    for (i, &(xi, yi)) in BOLT.iter().enumerate() {
        // Each edge runs from the previous point to this one (the first edge closes the ring).
        let (xj, yj) = BOLT[(i + BOLT.len() - 1) % BOLT.len()];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            inside = !inside;
        }
    }
    inside
}

/// The text of the menu items that change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Labels {
    pub now: String,
    pub playing: bool,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
pub use native::Tray;

#[cfg(any(target_os = "macos", target_os = "windows"))]
mod native {
    use muda::{Menu, MenuItem, PredefinedMenuItem};
    use tray_icon::{Icon, TrayIcon, TrayIconBuilder};

    use super::*;

    /// The tray icon, with the items whose text changes.
    pub struct Tray {
        // Kept alive for as long as the icon is shown.
        _icon: TrayIcon,
        now: MenuItem,
        play_pause: MenuItem,
        shown: Option<Labels>,
    }

    impl Tray {
        /// Shows the icon with its menu. Must run on the main thread, after the event loop has
        /// started (the menu bar item needs the application).
        pub fn new() -> Result<Tray, String> {
            let now = MenuItem::with_id(ID_NOW, NOTHING_PLAYING, false, None);
            let play_pause = MenuItem::with_id(ID_PLAY_PAUSE, "Play", true, None);
            let menu = Menu::new();
            let build = || -> Result<(), muda::Error> {
                menu.append(&now)?;
                menu.append(&PredefinedMenuItem::separator())?;
                menu.append(&play_pause)?;
                menu.append(&MenuItem::with_id(ID_NEXT, "Next", true, None))?;
                menu.append(&MenuItem::with_id(ID_PREVIOUS, "Previous", true, None))?;
                menu.append(&MenuItem::with_id(ID_STOP, "Stop", true, None))?;
                menu.append(&PredefinedMenuItem::separator())?;
                menu.append(&MenuItem::with_id(
                    ID_SHOW_HIDE,
                    "Show/Hide TubeAmp windows",
                    true,
                    None,
                ))?;
                menu.append(&MenuItem::with_id(ID_LIBRARY, "Media Library", true, None))?;
                menu.append(&PredefinedMenuItem::separator())?;
                menu.append(&MenuItem::with_id(ID_QUIT, "Quit TubeAmp", true, None))?;
                Ok(())
            };
            build().map_err(|e| e.to_string())?;
            let icon = Icon::from_rgba(icon_rgba(), ICON_PX, ICON_PX).map_err(|e| e.to_string())?;
            let builder = TrayIconBuilder::new()
                .with_menu(Box::new(menu))
                .with_tooltip("TubeAmp");
            // On macOS the icon is a template: the menu bar tints it.
            #[cfg(target_os = "macos")]
            let builder = builder.with_icon_templated(icon);
            #[cfg(not(target_os = "macos"))]
            let builder = builder.with_icon(icon);
            let tray = builder.build().map_err(|e| e.to_string())?;
            Ok(Tray {
                _icon: tray,
                now,
                play_pause,
                shown: None,
            })
        }

        /// Updates the track line, the play/pause label and the tooltip when they changed.
        pub fn sync(&mut self, labels: Labels) {
            if self.shown.as_ref() == Some(&labels) {
                return;
            }
            self.now.set_text(crate::menu_label::escape(&labels.now));
            self.play_pause
                .set_text(if labels.playing { "Pause" } else { "Play" });
            let _ = self
                ._icon
                .set_tooltip(Some(format!("TubeAmp - {}", labels.now)));
            self.shown = Some(labels);
        }
    }
}

/// Stand-in on other platforms: the tray is macOS and Windows only.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub struct Tray;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl Tray {
    pub fn new() -> Result<Tray, String> {
        Err("the tray icon needs macOS or Windows".to_string())
    }

    pub fn sync(&mut self, _labels: Labels) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_line_names_the_track_or_says_nothing_is_playing() {
        assert_eq!(now_line("", ""), NOTHING_PLAYING);
        assert_eq!(now_line("  ", "Artist"), NOTHING_PLAYING);
        assert_eq!(now_line("Despacito", ""), "Despacito");
        assert_eq!(now_line(" Despacito ", "Luis Fonsi"), "Luis Fonsi - Despacito");
    }

    #[test]
    fn the_icon_is_square_with_a_visible_glyph_and_empty_corners() {
        let rgba = icon_rgba();
        let size = ICON_PX as usize;
        assert_eq!(rgba.len(), size * size * 4);
        let alphas: Vec<u8> = rgba.chunks_exact(4).map(|p| p[3]).collect();
        let drawn = alphas.iter().filter(|&&a| a > 0).count();
        let solid = alphas.iter().filter(|&&a| a == 255).count();
        assert!(drawn > size * size / 10, "too little drawn: {drawn}");
        assert!(solid > 0, "no fully opaque pixels");
        assert!(drawn < size * size / 2, "too much drawn: {drawn}");
        // The corners are outside the tube.
        assert_eq!(alphas[0], 0);
        assert_eq!(alphas[size - 1], 0);
        assert_eq!(alphas[size * size - 1], 0);
    }

    #[test]
    fn the_bolt_is_inside_the_tube_and_the_tube_wall_is_drawn() {
        assert!(in_bolt(0.5, 0.5));
        assert!(!in_bolt(0.05, 0.05));
        // The tube's left wall at mid height, on the stroke's centre line.
        assert!(in_tube_outline(0.5 - 0.25, 0.5));
        // The middle of the tube is hollow except for the bolt.
        assert!(!in_tube_outline(0.5, 0.2));
    }
}
