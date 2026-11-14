//! OS media keys and Now Playing, through `souvlaki`: MPNowPlayingInfoCenter and the remote command
//! center on macOS, the System Media Transport Controls on Windows.
//!
//! The OS events come back as [`MediaAction`]s on the event loop, where they run the same code as
//! the buttons. What is shown (title, artist, length, cover, play or pause) is pushed only when it
//! changes. The pure parts here (what counts as a change, what a seek means) are tested on their
//! own. The OS calls are only compiled on macOS and Windows.

/// A request from the OS media keys or the Now Playing widget.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MediaAction {
    Play,
    Pause,
    Toggle,
    Next,
    Previous,
    Stop,
    /// Seek to this position, in seconds.
    Seek(f64),
    /// Seek by this many seconds (negative is backwards).
    SeekBy(f64),
}

/// The seek step for a "seek forward or backward" event that gives no amount: the arrow keys' 5 s.
pub const DEFAULT_SEEK_SECS: f64 = 5.0;

/// What Now Playing shows for the current track.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Track {
    pub title: String,
    pub artist: String,
    /// Whole seconds; 0 when unknown.
    pub duration_secs: u32,
    /// Cover art URL, when the page has one.
    pub cover_url: Option<String>,
}

/// Whether Now Playing shows the track as playing, paused or not at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum State {
    Stopped,
    Paused,
    Playing,
}

/// The last values pushed to the OS, so that only changes are pushed.
#[derive(Debug, Default)]
pub struct Sent {
    track: Option<Track>,
    playback: Option<(State, u32)>,
}

/// What [`Sent::update`] decided must be pushed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pushes {
    /// The new track, when it changed. Push it before the playback state, because setting the
    /// metadata clears the elapsed time on macOS.
    pub track: Option<Track>,
    /// The new state and whole elapsed seconds, when either changed.
    pub playback: Option<(State, u32)>,
}

impl Sent {
    /// Compares the current values with the last pushed ones and records the new ones. A track
    /// change also counts as a playback change, so the position is sent again after it.
    pub fn update(&mut self, track: &Track, state: State, secs: u32) -> Pushes {
        let track_changed = self.track.as_ref() != Some(track);
        let mut pushes = Pushes {
            track: None,
            playback: None,
        };
        if track_changed {
            self.track = Some(track.clone());
            pushes.track = Some(track.clone());
        }
        let playback = (state, secs);
        if track_changed || self.playback != Some(playback) {
            self.playback = Some(playback);
            pushes.playback = Some(playback);
        }
        pushes
    }
}

/// The seconds a seek event means: forward is positive.
pub fn seek_secs(forward: bool, amount: f64) -> f64 {
    if forward {
        amount
    } else {
        -amount
    }
}

/// The app's own media controls, one set per main window.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub struct NowPlaying {
    controls: souvlaki::MediaControls,
    sent: Sent,
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
impl NowPlaying {
    /// Registers the controls. `on_action` runs for each media key or Now Playing request, on the
    /// thread that runs the OS callbacks (the main thread on macOS). It should only forward the
    /// action to the event loop. `window` is the main window; Windows needs its handle.
    pub fn new(
        window: &winit::window::Window,
        on_action: impl Fn(MediaAction) + Send + 'static,
    ) -> Result<NowPlaying, String> {
        let hwnd = window_handle_for_os(window);
        let config = souvlaki::PlatformConfig {
            display_name: "TubeAmp",
            dbus_name: "app.tubeamp",
            hwnd,
        };
        let mut controls = souvlaki::MediaControls::new(config).map_err(|e| format!("{e:?}"))?;
        controls
            .attach(move |event| {
                if let Some(action) = map_event(event) {
                    on_action(action);
                }
            })
            .map_err(|e| format!("{e:?}"))?;
        Ok(NowPlaying {
            controls,
            sent: Sent::default(),
        })
    }

    /// Pushes what changed since the last call.
    pub fn update(&mut self, track: &Track, state: State, secs: u32) {
        let pushes = self.sent.update(track, state, secs);
        if let Some(track) = pushes.track {
            let metadata = souvlaki::MediaMetadata {
                title: non_empty(&track.title),
                album: None,
                artist: non_empty(&track.artist),
                cover_url: track.cover_url.as_deref(),
                duration: (track.duration_secs > 0)
                    .then(|| std::time::Duration::from_secs(u64::from(track.duration_secs))),
            };
            if let Err(e) = self.controls.set_metadata(metadata) {
                eprintln!("tubeamp: could not set Now Playing info: {e:?}");
            }
        }
        if let Some((state, secs)) = pushes.playback {
            let progress = Some(souvlaki::MediaPosition(std::time::Duration::from_secs(
                u64::from(secs),
            )));
            let playback = match state {
                State::Stopped => souvlaki::MediaPlayback::Stopped,
                State::Paused => souvlaki::MediaPlayback::Paused { progress },
                State::Playing => souvlaki::MediaPlayback::Playing { progress },
            };
            if let Err(e) = self.controls.set_playback(playback) {
                eprintln!("tubeamp: could not set the playback state: {e:?}");
            }
        }
    }
}

#[cfg(any(target_os = "macos", target_os = "windows"))]
fn non_empty(text: &str) -> Option<&str> {
    let trimmed = text.trim();
    (!trimmed.is_empty()).then_some(trimmed)
}

/// The window handle souvlaki needs on Windows (its `HWND`). macOS does not use one.
#[cfg(target_os = "windows")]
fn window_handle_for_os(window: &winit::window::Window) -> Option<*mut std::ffi::c_void> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    match window.window_handle().ok()?.as_raw() {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as *mut std::ffi::c_void),
        _ => None,
    }
}

#[cfg(target_os = "macos")]
fn window_handle_for_os(_window: &winit::window::Window) -> Option<*mut std::ffi::c_void> {
    None
}

/// Maps a souvlaki event to an action. Events the app does not use (volume, shuffle, open URI)
/// give `None`.
#[cfg(any(target_os = "macos", target_os = "windows"))]
fn map_event(event: souvlaki::MediaControlEvent) -> Option<MediaAction> {
    use souvlaki::{MediaControlEvent as E, SeekDirection};
    let forward = |dir: SeekDirection| matches!(dir, SeekDirection::Forward);
    match event {
        E::Play => Some(MediaAction::Play),
        E::Pause => Some(MediaAction::Pause),
        E::Toggle => Some(MediaAction::Toggle),
        E::Next => Some(MediaAction::Next),
        E::Previous => Some(MediaAction::Previous),
        E::Stop => Some(MediaAction::Stop),
        E::Seek(dir) => Some(MediaAction::SeekBy(seek_secs(
            forward(dir),
            DEFAULT_SEEK_SECS,
        ))),
        E::SeekBy(dir, amount) => Some(MediaAction::SeekBy(seek_secs(
            forward(dir),
            amount.as_secs_f64(),
        ))),
        E::SetPosition(position) => Some(MediaAction::Seek(position.0.as_secs_f64())),
        _ => None,
    }
}

/// Stand-in on other platforms: there are no media keys to register.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub struct NowPlaying;

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
impl NowPlaying {
    pub fn new(
        _window: &winit::window::Window,
        _on_action: impl Fn(MediaAction) + Send + 'static,
    ) -> Result<NowPlaying, String> {
        Err("media keys need macOS or Windows".to_string())
    }

    pub fn update(&mut self, _track: &Track, _state: State, _secs: u32) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(title: &str) -> Track {
        Track {
            title: title.to_string(),
            artist: "Artist".to_string(),
            duration_secs: 200,
            cover_url: None,
        }
    }

    #[test]
    fn the_first_update_pushes_everything() {
        let mut sent = Sent::default();
        let pushes = sent.update(&track("A"), State::Playing, 3);
        assert_eq!(pushes.track, Some(track("A")));
        assert_eq!(pushes.playback, Some((State::Playing, 3)));
    }

    #[test]
    fn an_unchanged_update_pushes_nothing() {
        let mut sent = Sent::default();
        sent.update(&track("A"), State::Playing, 3);
        let pushes = sent.update(&track("A"), State::Playing, 3);
        assert_eq!(pushes.track, None);
        assert_eq!(pushes.playback, None);
    }

    #[test]
    fn a_new_second_or_state_pushes_only_the_playback() {
        let mut sent = Sent::default();
        sent.update(&track("A"), State::Playing, 3);
        let tick = sent.update(&track("A"), State::Playing, 4);
        assert_eq!(tick.track, None);
        assert_eq!(tick.playback, Some((State::Playing, 4)));
        let paused = sent.update(&track("A"), State::Paused, 4);
        assert_eq!(paused.playback, Some((State::Paused, 4)));
    }

    #[test]
    fn a_new_track_pushes_the_metadata_and_the_position_again() {
        let mut sent = Sent::default();
        sent.update(&track("A"), State::Playing, 90);
        let next = sent.update(&track("B"), State::Playing, 90);
        assert_eq!(next.track, Some(track("B")));
        assert_eq!(next.playback, Some((State::Playing, 90)));
    }

    #[test]
    fn a_new_cover_counts_as_a_track_change() {
        let mut sent = Sent::default();
        sent.update(&track("A"), State::Playing, 0);
        let mut with_cover = track("A");
        with_cover.cover_url = Some("https://example.invalid/cover.jpg".to_string());
        assert_eq!(sent.update(&with_cover, State::Playing, 0).track, Some(with_cover));
    }

    #[test]
    fn seeks_forward_are_positive_and_backward_negative() {
        assert_eq!(seek_secs(true, 5.0), 5.0);
        assert_eq!(seek_secs(false, 5.0), -5.0);
        assert_eq!(seek_secs(false, 0.0), 0.0);
    }
}
