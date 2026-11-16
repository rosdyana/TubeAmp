//! Captures real YouTube Music InnerTube responses for the `tubeamp-ytm` parser fixtures.
//!
//! Opens a small visible window with the bridge webview on music.youtube.com, waits for
//! `Evt::Ready`, sends the requests built by `tubeamp-ytm`, writes each raw body to
//! `crates/ytm/tests/fixtures/<name>.json`, prints a summary, and exits.
//!
//! Run with `cargo run -p tubeamp-bridge --example ytm_dump`. Each request is answered by
//! YouTube's own page, so the results are the ones a visitor sees (signed in or not).

use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

use tubeamp_bridge::{Bridge, Cmd, Evt};
use tubeamp_ytm as ytm;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::window::{Window, WindowId};

/// How long to wait for the page to report `Ready` (sign-in state and ytcfg) before giving up.
const READY_TIMEOUT: Duration = Duration::from_secs(90);
/// How long the whole capture may take, from start to the last response.
const TOTAL_TIMEOUT: Duration = Duration::from_secs(240);

fn fixtures_dir() -> PathBuf {
    PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../ytm/tests/fixtures"))
}

/// A request to send, with the name its response is saved under.
type Job = (&'static str, ytm::Request);

struct Capture {
    window: Option<Window>,
    bridge: Option<Bridge>,
    proxy: winit::event_loop::EventLoopProxy<Evt>,
    started: Instant,
    signed_in: Option<bool>,
    /// Next InnerTube request id.
    next_req: u64,
    /// Requests in flight: request id to name.
    pending: BTreeMap<u64, &'static str>,
    /// Successful raw bodies, by name.
    raw: BTreeMap<&'static str, String>,
    /// Failed requests, for the report.
    failures: Vec<String>,
    /// Stage of the capture plan. Stage 0 is sent on `Ready`, later stages once the previous ones
    /// have answered and can supply the ids they need.
    stage: u8,
    /// Set once the capture has finished or timed out.
    done: bool,
    exit_code: u8,
}

impl Capture {
    fn new(proxy: winit::event_loop::EventLoopProxy<Evt>) -> Capture {
        Capture {
            window: None,
            bridge: None,
            proxy,
            started: Instant::now(),
            signed_in: None,
            next_req: 1,
            pending: BTreeMap::new(),
            raw: BTreeMap::new(),
            failures: Vec::new(),
            stage: 0,
            done: false,
            exit_code: 0,
        }
    }

    fn send(&mut self, name: &'static str, (endpoint, body): ytm::Request) {
        let req = self.next_req;
        self.next_req += 1;
        self.pending.insert(req, name);
        println!("request {req}: {name} -> {endpoint}");
        if let Some(bridge) = &self.bridge {
            let cmd = Cmd::Api {
                req,
                endpoint: endpoint.to_string(),
                body,
            };
            if let Err(e) = bridge.send(&cmd) {
                self.failures.push(format!("{name}: could not send ({e})"));
                self.pending.remove(&req);
            }
        }
    }

    /// The jobs for a stage. They can depend on the responses of earlier stages.
    fn jobs(&self, stage: u8) -> Vec<Job> {
        match stage {
            0 => vec![
                ("search_daft_punk", ytm::search("daft punk", None)),
                (
                    "search_songs",
                    ytm::search("daft punk", Some(ytm::SearchFilter::Songs)),
                ),
                (
                    "search_albums",
                    ytm::search("daft punk", Some(ytm::SearchFilter::Albums)),
                ),
                (
                    "search_playlists",
                    ytm::search("daft punk", Some(ytm::SearchFilter::Playlists)),
                ),
                ("home", ytm::home()),
                ("next", ytm::next("kJQP7kiw5Fk")),
                ("artist", ytm::artist("UC_kRDKYrUlrbtrSiyu5Tflg")),
            ],
            1 => {
                let mut jobs = Vec::new();
                if let Some(id) = self.public_playlist_id() {
                    println!("playlist id from captured results: {id}");
                    jobs.push(("playlist", ytm::playlist(&id)));
                }
                if let Some(id) = self.first_album_id() {
                    println!("album id from captured results: {id}");
                    jobs.push(("album", ytm::album(&id)));
                }
                if let Some(token) = self.raw.get("home").and_then(|raw| ytm::parse_home(raw).continuation) {
                    jobs.push(("home_continuation", ytm::continuation(&token)));
                }
                jobs
            }
            2 => self
                .raw
                .get("playlist")
                .and_then(|raw| ytm::parse_playlist(raw).continuation)
                .map(|token| vec![("playlist_continuation", ytm::continuation(&token))])
                .unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    /// Items from a captured search or home response, in page order.
    fn items_in(&self, name: &str) -> Vec<ytm::Item> {
        let Some(raw) = self.raw.get(name) else {
            return Vec::new();
        };
        if name.starts_with("search") {
            let results = ytm::parse_search(raw);
            results
                .top
                .into_iter()
                .chain(results.shelves.into_iter().flat_map(|s| s.items))
                .collect()
        } else {
            ytm::parse_home(raw)
                .shelves
                .into_iter()
                .flat_map(|s| s.items)
                .collect()
        }
    }

    /// A public playlist, preferring `PL…` ids over mixes.
    fn public_playlist_id(&self) -> Option<String> {
        let mut ids = Vec::new();
        for name in ["search_playlists", "search_daft_punk", "home"] {
            for item in self.items_in(name) {
                if let ytm::Item::Playlist(p) = item {
                    ids.push(p.browse_id);
                }
            }
        }
        ids.iter()
            .find(|id| id.starts_with("VLPL"))
            .or(ids.first())
            .map(|id| id.strip_prefix("VL").unwrap_or(id).to_string())
    }

    fn first_album_id(&self) -> Option<String> {
        for name in ["search_albums", "search_daft_punk", "home"] {
            for item in self.items_in(name) {
                if let ytm::Item::Album(a) = item {
                    return Some(a.browse_id);
                }
            }
        }
        None
    }

    /// Sends the next stage that has requests. Finishes when there are none left.
    fn advance(&mut self, event_loop: &ActiveEventLoop) {
        while self.pending.is_empty() && !self.done {
            let stage = self.stage;
            self.stage += 1;
            if stage > 2 {
                self.finish(event_loop);
                return;
            }
            for (name, request) in self.jobs(stage) {
                self.send(name, request);
            }
        }
    }

    fn finish(&mut self, event_loop: &ActiveEventLoop) {
        self.done = true;
        println!("\n== capture finished in {:.1}s ==", self.started.elapsed().as_secs_f64());
        println!("signed in: {}", match self.signed_in {
            Some(v) => v.to_string(),
            None => "unknown (no Ready event)".to_string(),
        });
        let dir = fixtures_dir();
        if let Err(e) = fs::create_dir_all(&dir) {
            println!("could not create {}: {e}", dir.display());
        }
        for (name, body) in &self.raw {
            let path = dir.join(format!("{name}.json"));
            match fs::write(&path, body) {
                Ok(()) => println!("wrote {} ({} bytes)", path.display(), body.len()),
                Err(e) => println!("could not write {}: {e}", path.display()),
            }
            summarize(name, body);
        }
        for failure in &self.failures {
            println!("FAILED {failure}");
        }
        if !self.pending.is_empty() {
            println!("unanswered: {:?}", self.pending.values().collect::<Vec<_>>());
        }
        let missing = ["search_daft_punk", "search_songs", "search_albums", "home", "next"]
            .iter()
            .filter(|n| !self.raw.contains_key(*n))
            .count();
        self.exit_code = if missing == 0 { 0 } else { 1 };
        event_loop.exit();
    }

    fn timed_out(&mut self, event_loop: &ActiveEventLoop) {
        println!("timed out");
        if self.signed_in.is_none() {
            println!("no Ready event: the page did not expose ytcfg");
        }
        for (req, name) in std::mem::take(&mut self.pending) {
            self.failures.push(format!("{name} (request {req}): no answer in time"));
        }
        self.exit_code = 2;
        self.finish(event_loop);
    }
}

/// Prints what the parser makes of a captured response, to spot gaps quickly.
fn summarize(name: &str, body: &str) {
    if name.starts_with("search") {
        let r = ytm::parse_search(body);
        let tracks: usize = r
            .shelves
            .iter()
            .map(|s| s.items.iter().filter(|i| matches!(i, ytm::Item::Track(_))).count())
            .sum();
        println!("  {name}: top={} shelves={} tracks={tracks}", r.top.is_some(), r.shelves.len());
        for s in &r.shelves {
            println!("    shelf {:?}: {} items", s.title, s.items.len());
        }
    } else if name == "home" || name == "home_continuation" {
        let r = ytm::parse_continuation(body);
        let home = ytm::parse_home(body);
        println!(
            "  {name}: shelves={} (continuation shelves={}) continuation={}",
            home.shelves.len(),
            r.shelves.len(),
            home.continuation.is_some()
        );
    } else if name.starts_with("playlist") || name == "album" {
        let p = if name.ends_with("continuation") {
            let c = ytm::parse_continuation(body);
            ytm::PlaylistPage {
                title: String::new(),
                author: None,
                tracks: c.tracks,
                continuation: c.continuation,
            }
        } else {
            ytm::parse_playlist(body)
        };
        println!(
            "  {name}: title={:?} author={:?} tracks={} continuation={}",
            p.title,
            p.author,
            p.tracks.len(),
            p.continuation.is_some()
        );
    } else if name == "next" {
        println!("  {name}: tracks={}", ytm::parse_next(body).len());
    }
}

impl ApplicationHandler<Evt> for Capture {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attributes = Window::default_attributes()
            .with_title("tubeamp ytm_dump")
            .with_inner_size(LogicalSize::new(480.0, 320.0))
            .with_visible(true);
        let window = match event_loop.create_window(attributes) {
            Ok(window) => window,
            Err(e) => {
                println!("could not create the window: {e}");
                self.exit_code = 1;
                event_loop.exit();
                return;
            }
        };
        let proxy = self.proxy.clone();
        match Bridge::new(&window, move |evt| {
            let _ = proxy.send_event(evt);
        }) {
            Ok(bridge) => self.bridge = Some(bridge),
            Err(e) => {
                println!("could not create the webview: {e}");
                self.exit_code = 1;
                event_loop.exit();
                return;
            }
        }
        self.window = Some(window);
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250)));
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        if let WindowEvent::CloseRequested = event {
            self.exit_code = 1;
            event_loop.exit();
        }
    }

    fn user_event(&mut self, event_loop: &ActiveEventLoop, evt: Evt) {
        if self.done {
            return;
        }
        match evt {
            Evt::Ready { signed_in } => {
                if self.signed_in.is_none() {
                    println!("ready: signed_in={signed_in}");
                    self.signed_in = Some(signed_in);
                    self.advance(event_loop);
                }
            }
            Evt::ApiResult { req, ok, status, body } => {
                let Some(name) = self.pending.remove(&req) else {
                    return;
                };
                if ok {
                    println!("response {req}: {name} ok ({status}, {} bytes)", body.len());
                    self.raw.insert(name, body);
                } else {
                    let head: String = body.chars().take(300).collect();
                    self.failures.push(format!("{name}: HTTP {status}: {head}"));
                    println!("response {req}: {name} failed ({status})");
                }
                self.advance(event_loop);
            }
            Evt::State { .. }
            | Evt::Ended
            | Evt::Escape
            | Evt::Key { .. }
            | Evt::Error { .. }
            | Evt::Media { .. }
            | Evt::Samples { .. }
            | Evt::AdSkip { .. } => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.done {
            return;
        }
        if self.started.elapsed() > TOTAL_TIMEOUT {
            self.timed_out(event_loop);
            return;
        }
        if self.signed_in.is_none() && self.started.elapsed() > READY_TIMEOUT {
            self.timed_out(event_loop);
            return;
        }
        event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + Duration::from_millis(250)));
    }
}

fn main() -> ExitCode {
    let event_loop = match EventLoop::<Evt>::with_user_event().build() {
        Ok(event_loop) => event_loop,
        Err(e) => {
            eprintln!("could not create the event loop: {e}");
            return ExitCode::FAILURE;
        }
    };
    let mut capture = Capture::new(event_loop.create_proxy());
    if let Err(e) = event_loop.run_app(&mut capture) {
        eprintln!("event loop error: {e}");
        return ExitCode::FAILURE;
    }
    ExitCode::from(capture.exit_code)
}
