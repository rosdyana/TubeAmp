//! Non-macOS audio: the post-EQ samples come from the page.
//!
//! The EQ runs in the page's Web Audio graph (`crates/bridge/src/bridge.js`), which also plays the
//! audio. The page posts blocks of its post-EQ mono samples (`Evt::Samples`), and the app passes
//! them to [`feed_page_samples`]. [`AudioEngine`] is the reader: the analyser drains the pending
//! samples, and the Milkdrop window copies the latest ones. Nothing is opened here, so `start`
//! always succeeds and the page decides when samples flow.

use std::sync::{LazyLock, Mutex, MutexGuard};
use std::time::Instant;

use crate::guard;
use crate::latest::LatestSamples;
use crate::AudioError;

/// The sample rate until the page reports one.
const DEFAULT_RATE: u32 = 48_000;

/// What the page has sent and the analyser has not taken yet.
struct Feed {
    /// Mono samples not drained yet, oldest first. Capped at one second.
    pending: Vec<f32>,
    /// The page's sample rate in Hz.
    rate: u32,
    /// When a non-silent sample last arrived, or `None` if none has.
    last_sound: Option<Instant>,
}

static FEED: Mutex<Feed> = Mutex::new(Feed {
    pending: Vec::new(),
    rate: DEFAULT_RATE,
    last_sound: None,
});

/// The newest samples, for the Milkdrop window. Written by [`feed_page_samples`].
static LATEST: LazyLock<LatestSamples> = LazyLock::new(LatestSamples::new);

/// The feed, even if a panic left the mutex poisoned: the UI must not panic on it.
fn feed() -> MutexGuard<'static, Feed> {
    FEED.lock().unwrap_or_else(|e| e.into_inner())
}

/// Hands the page's post-EQ mono samples to the readers. Called on the UI thread for every
/// `Evt::Samples`. `rate` is the page's sample rate in Hz; zero keeps the rate already set.
pub fn feed_page_samples(rate: u32, samples: &[f32]) {
    let mut f = feed();
    if rate != 0 {
        f.rate = rate;
    }
    for &s in samples {
        LATEST.push(s);
    }
    f.pending.extend_from_slice(samples);
    // The analyser reads every frame, so this only trims a backlog: keep the newest second.
    let cap = f.rate as usize;
    if f.pending.len() > cap {
        let excess = f.pending.len() - cap;
        f.pending.drain(..excess);
    }
    if !guard::is_silent(samples) {
        f.last_sound = Some(Instant::now());
    }
}

/// The audio engine on non-macOS platforms. It reads the samples the page feeds in.
pub struct AudioEngine {
    /// When the engine started. Stands in for the last sound until the page makes one.
    started: Instant,
}

impl AudioEngine {
    /// Nothing to open, so this always succeeds.
    pub fn start() -> Result<AudioEngine, AudioError> {
        Ok(AudioEngine {
            started: Instant::now(),
        })
    }

    /// The page's sample rate in Hz.
    pub fn sample_rate(&self) -> u32 {
        feed().rate
    }

    /// When the page last sent a non-silent sample, or when the engine started if it never has.
    pub fn last_sound_at(&self) -> Instant {
        feed().last_sound.unwrap_or(self.started)
    }

    /// Replaces `out` with the samples fed since the last call.
    pub fn drain_samples(&mut self, out: &mut Vec<f32>) {
        let mut f = feed();
        out.clear();
        out.extend_from_slice(&f.pending);
        f.pending.clear();
    }

    /// Replaces `out` with the newest `n` mono samples (at most `LATEST_CAPACITY`), oldest first.
    /// Returns the total number of samples written so far, as `AudioEngine` on macOS does.
    pub fn copy_latest(&self, out: &mut Vec<f32>, n: usize) -> u64 {
        LATEST.copy_latest(out, n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    // The feed and the latest-samples ring are process-wide, so the checks run in order in one test.
    #[test]
    fn the_page_feed_drains_in_order_caps_the_backlog_and_tracks_sound() {
        let mut engine = AudioEngine::start().expect("starting never fails");
        let mut out = Vec::new();

        // Nothing has been fed yet, so the last sound is the engine's start.
        assert_eq!(engine.last_sound_at(), engine.started);

        // Samples come out in the order they went in, and only once.
        feed_page_samples(48_000, &[0.25, -0.5, 0.0, 0.75]);
        engine.drain_samples(&mut out);
        assert_eq!(out, vec![0.25, -0.5, 0.0, 0.75]);
        engine.drain_samples(&mut out);
        assert!(out.is_empty(), "a second drain is empty");

        // The rate follows the page, and zero keeps the rate already set.
        assert_eq!(engine.sample_rate(), 48_000);
        feed_page_samples(0, &[]);
        assert_eq!(engine.sample_rate(), 48_000);
        feed_page_samples(8_000, &[]);
        assert_eq!(engine.sample_rate(), 8_000);

        // The backlog is capped at one second at the page's rate, and the newest samples are kept.
        // The newest-samples ring is read on the same feed: its newest three are the ramp's end.
        let before = engine.copy_latest(&mut out, 0);
        let ramp: Vec<f32> = (0..10_000u32).map(|k| k as f32 / 10_000.0).collect();
        feed_page_samples(8_000, &ramp);
        engine.drain_samples(&mut out);
        assert_eq!(out.as_slice(), &ramp[2_000..]);
        let after = engine.copy_latest(&mut out, 3);
        assert_eq!(after, before + 10_000);
        assert_eq!(out.as_slice(), &ramp[9_997..]);

        // A silent feed does not move last_sound_at. A non-zero one does.
        let last = engine.last_sound_at();
        std::thread::sleep(Duration::from_millis(5));
        feed_page_samples(8_000, &[0.0; 256]);
        assert_eq!(engine.last_sound_at(), last, "silence is not sound");
        std::thread::sleep(Duration::from_millis(5));
        feed_page_samples(8_000, &[0.0, 0.001]);
        assert!(engine.last_sound_at() > last, "a non-zero sample is sound");
    }
}
