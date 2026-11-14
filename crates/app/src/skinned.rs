//! A skinned window: a winit window with a softbuffer surface that shows a skin-pixel
//! [`Canvas`] at an integer zoom. The main and equalizer windows are both built on it.

use std::num::NonZeroU32;
use std::rc::Rc;

use softbuffer::{Context, Surface};
use tubeamp_ui::Canvas;
use winit::dpi::{LogicalSize, PhysicalPosition};
use winit::event_loop::ActiveEventLoop;
use winit::window::{Window, WindowAttributes, WindowId};

/// A window shared between winit, softbuffer and the app.
pub type SharedWindow = Rc<Window>;

/// Nearest-neighbour upscale of a canvas to `0x00RRGGBB` pixels at integer `factor`.
pub fn scaled(canvas: &Canvas, factor: u32) -> Vec<u32> {
    let mut out = vec![0u32; ((canvas.w * factor) * (canvas.h * factor)) as usize];
    canvas.scale_into(&mut out, factor);
    out
}

/// A winit window that presents skin-pixel canvases through softbuffer.
pub struct SkinnedWindow {
    pub window: SharedWindow,
    // Kept for the lifetime of the surface.
    _context: Context<SharedWindow>,
    surface: Surface<SharedWindow, SharedWindow>,
    /// The skin's size in skin pixels (1×).
    base: (u32, u32),
    zoom: u32,
    cursor: Option<PhysicalPosition<f64>>,
}

impl SkinnedWindow {
    /// Creates the window and its surface. `base` is the skin size in skin pixels.
    pub fn new(
        event_loop: &ActiveEventLoop,
        attributes: WindowAttributes,
        base: (u32, u32),
        zoom: u32,
    ) -> Result<Self, String> {
        let window: SharedWindow = Rc::new(
            event_loop
                .create_window(attributes)
                .map_err(|e| format!("could not create the window: {e}"))?,
        );
        let context = Context::new(window.clone())
            .map_err(|e| format!("could not create the softbuffer context: {e}"))?;
        let surface = Surface::new(&context, window.clone())
            .map_err(|e| format!("could not create the softbuffer surface: {e}"))?;
        Ok(SkinnedWindow {
            window,
            _context: context,
            surface,
            base,
            zoom,
            cursor: None,
        })
    }

    pub fn id(&self) -> WindowId {
        self.window.id()
    }

    pub fn zoom(&self) -> u32 {
        self.zoom
    }

    /// The window size in logical pixels at the current zoom.
    pub fn logical_size(&self) -> LogicalSize<u32> {
        LogicalSize::new(self.base.0 * self.zoom, self.base.1 * self.zoom)
    }

    /// Changes the zoom and asks the OS for the matching size.
    pub fn set_zoom(&mut self, zoom: u32) {
        self.zoom = zoom;
        let _ = self.window.request_inner_size(self.logical_size());
    }

    /// Changes the skin size (for example a resized playlist) and asks the OS for the matching
    /// window size at the current zoom.
    pub fn set_base(&mut self, base: (u32, u32)) {
        self.base = base;
        let _ = self.window.request_inner_size(self.logical_size());
    }

    /// The last pointer position over the window, in physical pixels.
    pub fn cursor_position(&self) -> Option<PhysicalPosition<f64>> {
        self.cursor
    }

    /// Integer scale from skin pixels to physical pixels: round(scale factor) × zoom.
    pub fn factor(&self) -> u32 {
        let scale = self.window.scale_factor();
        (scale.round().max(1.0) as u32) * self.zoom
    }

    pub fn request_redraw(&self) {
        self.window.request_redraw();
    }

    pub fn set_cursor(&mut self, cursor: Option<PhysicalPosition<f64>>) {
        self.cursor = cursor;
    }

    /// Pointer position in skin pixels, not clamped to the window. Drags use this so they keep
    /// tracking past the edge.
    pub fn skin_point(&self) -> Option<(f64, f64)> {
        let pos = self.cursor?;
        let size = self.window.inner_size();
        let f = self.factor();
        let (ox, _) = fit(size.width, self.base.0 * f);
        let (oy, _) = fit(size.height, self.base.1 * f);
        Some((
            (pos.x - f64::from(ox)) / f64::from(f),
            (pos.y - f64::from(oy)) / f64::from(f),
        ))
    }

    /// Pointer position in whole skin pixels, or `None` outside the skin.
    pub fn skin_pixel(&self) -> Option<(i32, i32)> {
        let (x, y) = self.skin_point()?;
        if x < 0.0 || y < 0.0 || x >= f64::from(self.base.0) || y >= f64::from(self.base.1) {
            return None;
        }
        Some((x as i32, y as i32))
    }

    /// Shows `canvas` (which must be the skin size) in the window.
    pub fn present(&mut self, canvas: &Canvas) {
        let size = self.window.inner_size();
        let (Some(pw), Some(ph)) = (NonZeroU32::new(size.width), NonZeroU32::new(size.height))
        else {
            return;
        };
        let f = self.factor();
        let pixels = scaled(canvas, f);
        if let Err(e) = self.surface.resize(pw, ph) {
            eprintln!("tubeamp: surface resize failed: {e}");
            return;
        }
        let mut buffer = match self.surface.buffer_mut() {
            Ok(buffer) => buffer,
            Err(e) => {
                eprintln!("tubeamp: could not map the window buffer: {e}");
                return;
            }
        };
        blit_centered(
            &mut buffer,
            size.width,
            size.height,
            &pixels,
            canvas.w * f,
            canvas.h * f,
        );
        if let Err(e) = buffer.present() {
            eprintln!("tubeamp: present failed: {e}");
        }
    }
}

/// Shows `menu` as a context menu with its top-left at `(x, y)` in logical pixels of `window`'s
/// content view. The call blocks until the menu closes, so it must run on the main thread. Item
/// clicks arrive later as menu events.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn show_context_menu(window: &Window, menu: &muda::Menu, x: f64, y: f64) -> Result<(), String> {
    use muda::dpi::{LogicalPosition, Position};
    use muda::ContextMenu;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle().map_err(|e| e.to_string())?;
    let position = Some(Position::Logical(LogicalPosition::new(x, y)));
    match handle.as_raw() {
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(appkit) => {
            // SAFETY: the view belongs to `window`, which is alive for the whole call, and this
            // runs on the main thread that owns the AppKit view.
            unsafe { menu.show_context_menu_for_nsview(appkit.ns_view.as_ptr(), position) };
            Ok(())
        }
        #[cfg(target_os = "windows")]
        RawWindowHandle::Win32(win32) => {
            // SAFETY: the HWND belongs to `window`, which is alive for the whole call.
            unsafe { menu.show_context_menu_for_hwnd(win32.hwnd.get(), position) };
            Ok(())
        }
        _ => Err("the window has no native view for a menu".to_string()),
    }
}

/// Other platforms have no context menus yet (macOS and Windows only).
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn show_context_menu(
    _window: &Window,
    _menu: &muda::Menu,
    _x: f64,
    _y: f64,
) -> Result<(), String> {
    Err("context menus need macOS or Windows".to_string())
}

/// The window's number as `screencapture -l` takes it (the CGWindowID), on macOS.
#[cfg(target_os = "macos")]
pub fn window_number(window: &Window) -> Option<isize> {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};

    let handle = window.window_handle().ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else {
        return None;
    };
    let view = appkit.ns_view.as_ptr().cast::<AnyObject>();
    // SAFETY: the NSView belongs to `window`, which is alive here, and this runs on the main
    // thread that owns the view. Both messages are plain getters.
    unsafe {
        let view: &AnyObject = &*view;
        let ns_window: *mut AnyObject = msg_send![view, window];
        if ns_window.is_null() {
            return None;
        }
        let number: isize = msg_send![&*ns_window, windowNumber];
        Some(number)
    }
}

/// Other platforms have no window numbers for `screencapture`.
#[cfg(not(target_os = "macos"))]
pub fn window_number(_window: &Window) -> Option<isize> {
    None
}

/// Saves one window to `path` with macOS's `screencapture`, which includes the webview. The
/// capture needs the screen recording permission for the terminal or app that runs TubeAmp.
#[cfg(target_os = "macos")]
pub fn capture_window(number: isize, path: &std::path::Path) -> Result<(), String> {
    let status = std::process::Command::new("screencapture")
        .arg("-x")
        .arg("-o")
        .arg("-l")
        .arg(number.to_string())
        .arg(path)
        .status()
        .map_err(|e| format!("could not run screencapture: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("screencapture exited with {status}"))
    }
}

/// Window capture is macOS only.
#[cfg(not(target_os = "macos"))]
pub fn capture_window(_number: isize, _path: &std::path::Path) -> Result<(), String> {
    Err("window capture needs macOS".to_string())
}

/// Centres `size` pixels within `total`, cropping if it does not fit.
/// Returns `(offset, visible)`. Shared by presentation and cursor mapping.
fn fit(total: u32, size: u32) -> (u32, u32) {
    let visible = size.min(total);
    ((total - visible) / 2, visible)
}

/// Copies a `sw` × `sh` frame into the centre of a `pw` × `ph` buffer, black elsewhere.
fn blit_centered(dst: &mut [u32], pw: u32, ph: u32, src: &[u32], sw: u32, sh: u32) {
    dst.fill(0);
    let (ox, cw) = fit(pw, sw);
    let (oy, ch) = fit(ph, sh);
    let stride = pw as usize;
    for (row, src_row) in src.chunks_exact(sw as usize).take(ch as usize).enumerate() {
        let start = (oy as usize + row) * stride + ox as usize;
        dst[start..start + cw as usize].copy_from_slice(&src_row[..cw as usize]);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_centres_and_crops() {
        assert_eq!(fit(10, 4), (3, 4));
        assert_eq!(fit(4, 10), (0, 4));
        assert_eq!(fit(7, 7), (0, 7));
    }

    #[test]
    fn blit_centered_pads_with_black() {
        let src = vec![1u32, 2, 3, 4];
        let mut dst = vec![9u32; 4 * 3];
        blit_centered(&mut dst, 4, 3, &src, 2, 2);
        // A 2×2 frame in a 4×3 buffer sits at x = 1, y = 0.
        assert_eq!(dst, vec![0, 1, 2, 0, 0, 3, 4, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn scaled_repeats_pixels() {
        let canvas = Canvas {
            w: 1,
            h: 1,
            px: vec![0xFF00_0042],
        };
        assert_eq!(scaled(&canvas, 2), vec![0x42; 4]);
    }
}
