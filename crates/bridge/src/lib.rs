//! YouTube Music host: a wry webview that loads music.youtube.com as a child of a winit window,
//! with `bridge.js` injected to read the player and run commands.

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
            .with_bounds(bounds(size.width, size.height))
            .build_as_child(host)?;
        Ok(Bridge { webview })
    }

    /// Sends a command to the page. Ignored by the page until `bridge.js` has run.
    pub fn send(&self, cmd: &Cmd) -> Result<(), wry::Error> {
        let json = serde_json::to_string(cmd).expect("Cmd always serializes");
        self.webview
            .evaluate_script(&format!("window.__tubeamp&&window.__tubeamp.cmd({json})"))
    }

    /// Resizes the webview to fill the host window (logical pixels, origin at the top left).
    pub fn set_bounds(&self, width: f64, height: f64) -> Result<(), wry::Error> {
        self.webview.set_bounds(bounds(width, height))
    }
}

fn bounds(width: f64, height: f64) -> Rect {
    Rect {
        position: LogicalPosition::new(0.0, 0.0).into(),
        size: LogicalSize::new(width, height).into(),
    }
}
