mod proto;

pub use proto::{Cmd, Evt};

use std::cell::Cell;

use winit::window::Window;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{Rect, WebView, WebViewBuilder};

const BRIDGE_JS: &str = include_str!("bridge.js");
/// Runs before `bridge.js`. On macOS it makes the page's media session TubeAmp's, so that WebKit's
/// Now Playing entry is the single one (see `takeMediaSession` in `bridge.js`). Elsewhere the page's
/// session is left as it was.
const NATIVE_MEDIA_PRELUDE: &str = if cfg!(target_os = "macos") {
    "window.__tubeamp_native_media = true;"
} else {
    ""
};
const YTM_URL: &str = "https://music.youtube.com/";
/// A desktop Safari user agent, which YouTube Music serves with the full player.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Safari/605.1.15";
/// The widest CSS viewport the video view lays the page out at. A wider webview is zoomed in
/// instead (see [`video_page_zoom`]): YouTube Music switches to a different layout above about
/// 615 CSS px, and the player's video then ends up with no size.
pub const VIDEO_CSS_WIDTH: f64 = 600.0;

/// The YouTube Music webview and its command channel.
pub struct Bridge {
    webview: WebView,
    /// Whether WebKit's window-occlusion detection was switched off for this view (macOS only).
    occlusion_detection_off: bool,
    /// The page zoom last set on the webview, so that unchanged zooms are not set again.
    page_zoom: Cell<f64>,
}

impl Bridge {
    /// Creates the webview as a child of `host` and starts loading YouTube Music. `sink` gets
    /// every event posted by `bridge.js`, on the thread that runs the event loop.
    pub fn new(host: &Window, sink: impl Fn(Evt) + 'static) -> Result<Bridge, wry::Error> {
        let size = host.inner_size().to_logical::<f64>(host.scale_factor());
        let webview = WebViewBuilder::new()
            .with_url(YTM_URL)
            .with_user_agent(USER_AGENT)
            .with_autoplay(true)
            .with_initialization_script(format!("{NATIVE_MEDIA_PRELUDE}\n{BRIDGE_JS}"))
            .with_ipc_handler(move |req| match serde_json::from_str::<Evt>(req.body()) {
                Ok(evt) => sink(evt),
                Err(e) => eprintln!("tubeamp: unrecognised bridge message ({e}): {}", req.body()),
            })
            .with_bounds(bounds(0.0, 0.0, size.width, size.height))
            .build_as_child(host)?;
        let occlusion_detection_off = disable_window_occlusion(&webview);
        Ok(Bridge {
            webview,
            occlusion_detection_off,
            page_zoom: Cell::new(1.0),
        })
    }

    /// Sends a command to the page. Ignored by the page until `bridge.js` has run.
    pub fn send(&self, cmd: &Cmd) -> Result<(), wry::Error> {
        let json = serde_json::to_string(cmd).expect("Cmd always serializes");
        self.webview
            .evaluate_script(&format!("window.__tubeamp&&window.__tubeamp.cmd({json})"))
    }

    /// Moves and resizes the webview inside its host window (logical pixels, origin at the top
    /// left of the host's content view).
    pub fn set_bounds(&self, x: f64, y: f64, width: f64, height: f64) -> Result<(), wry::Error> {
        self.webview.set_bounds(bounds(x, y, width, height))
    }

    /// Sets the page's zoom (1.0 is the normal size). The page lays out at its CSS size divided by
    /// this, so a larger zoom gives the page a narrower viewport. Unchanged zooms are not set again.
    pub fn set_page_zoom(&self, zoom: f64) -> Result<(), wry::Error> {
        if (self.page_zoom.get() - zoom).abs() < f64::EPSILON {
            return Ok(());
        }
        self.webview.zoom(zoom)?;
        self.page_zoom.set(zoom);
        Ok(())
    }

    /// Whether WebKit's window-occlusion detection was switched off for this view. Always false
    /// off macOS, or when the WebKit version has no such private method.
    pub fn occlusion_detection_off(&self) -> bool {
        self.occlusion_detection_off
    }
}

/// The page zoom for a webview `width` logical px wide in the video view: 1.0 up to
/// [`VIDEO_CSS_WIDTH`] px, and `width / VIDEO_CSS_WIDTH` above it, so the page's CSS viewport stays
/// at most `VIDEO_CSS_WIDTH` wide.
pub fn video_page_zoom(width: f64) -> f64 {
    if width.is_finite() && width > VIDEO_CSS_WIDTH {
        width / VIDEO_CSS_WIDTH
    } else {
        1.0
    }
}

fn bounds(x: f64, y: f64, width: f64, height: f64) -> Rect {
    Rect {
        position: LogicalPosition::new(x, y).into(),
        size: LogicalSize::new(width.max(0.0), height.max(0.0)).into(),
    }
}

/// Stops WebKit from treating the page as occluded or hidden when its window is hidden or covered.
/// Otherwise its timers and video run throttled, and the state updates slow down. This uses the
/// private `-[WKWebView _setWindowOcclusionDetectionEnabled:]`, which WebKit has had for years;
/// the call is guarded with `respondsToSelector:`, so a WebKit without it just keeps the default.
#[cfg(target_os = "macos")]
fn disable_window_occlusion(webview: &WebView) -> bool {
    use objc2::runtime::{AnyObject, Bool};
    use objc2::{msg_send, sel};
    use wry::WebViewExtMacOS;

    let wk = webview.webview();
    let view: &AnyObject = &wk;
    let selector = sel!(_setWindowOcclusionDetectionEnabled:);
    // SAFETY: every NSObject answers `respondsToSelector:`, and the view is one.
    let responds: Bool = unsafe { msg_send![view, respondsToSelector: selector] };
    if !responds.as_bool() {
        return false;
    }
    // SAFETY: the selector was checked just above, and the method takes one BOOL argument.
    unsafe {
        let _: () = msg_send![view, _setWindowOcclusionDetectionEnabled: Bool::NO];
    }
    true
}

#[cfg(not(target_os = "macos"))]
fn disable_window_occlusion(_webview: &WebView) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_page_is_not_zoomed_up_to_the_css_width() {
        assert_eq!(video_page_zoom(0.0), 1.0);
        assert_eq!(video_page_zoom(256.0), 1.0);
        assert_eq!(video_page_zoom(VIDEO_CSS_WIDTH), 1.0);
    }

    #[test]
    fn a_wider_view_is_zoomed_so_the_css_width_stays_at_the_limit() {
        // The 2x button's content is 640 px wide: zoom 640 / 600.
        let zoom = video_page_zoom(640.0);
        assert!((zoom - 640.0 / 600.0).abs() < 1e-12);
        assert!((640.0 / zoom - VIDEO_CSS_WIDTH).abs() < 1e-9);
        // A large or fullscreen view keeps the CSS width at the limit too.
        for width in [900.0, 1512.0, 2560.0] {
            let css = width / video_page_zoom(width);
            assert!((css - VIDEO_CSS_WIDTH).abs() < 1e-9, "{width}: {css}");
        }
    }

    #[test]
    fn a_bad_width_keeps_the_page_at_its_normal_zoom() {
        assert_eq!(video_page_zoom(f64::NAN), 1.0);
        assert_eq!(video_page_zoom(f64::INFINITY), 1.0);
        assert_eq!(video_page_zoom(-5.0), 1.0);
    }
}
