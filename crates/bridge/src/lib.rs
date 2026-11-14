mod proto;

pub use proto::{Cmd, Evt};

use winit::window::Window;
use wry::dpi::{LogicalPosition, LogicalSize};
use wry::{Rect, WebView, WebViewBuilder};

const BRIDGE_JS: &str = include_str!("bridge.js");
const YTM_URL: &str = "https://music.youtube.com/";
/// A desktop Safari user agent, which YouTube Music serves with the full player.
const USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.5 Safari/605.1.15";

/// The YouTube Music webview and its command channel.
pub struct Bridge {
    webview: WebView,
    /// Whether WebKit's window-occlusion detection was switched off for this view (macOS only).
    occlusion_detection_off: bool,
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
            .with_initialization_script(BRIDGE_JS)
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

    /// Whether WebKit's window-occlusion detection was switched off for this view. Always false
    /// off macOS, or when the WebKit version has no such private method.
    pub fn occlusion_detection_off(&self) -> bool {
        self.occlusion_detection_off
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
