//! Audio for TubeAmp.
//!
//! - [`AudioEngine`] taps the audio of YouTube Music's WebKit process (macOS 14.2+, Core Audio
//!   process tap), runs the Winamp EQ in the audio callback and hands the post-EQ mono samples
//!   to the UI thread through a lock-free ring. Other platforms get a stub whose `start` fails
//!   with [`AudioError::Unsupported`] (Windows will use in-page Web Audio, later).
//! - [`params`] is the shared EQ state (preamp, bands, balance). It is a plain static, so the UI
//!   can change it before the engine starts.
//! - [`vis`] is the platform-independent spectrum/oscilloscope analyser for the main window.
//! - [`LatestSamples`] keeps the newest mono samples for a second reader (the Milkdrop window),
//!   fed by the same audio callback. [`AudioEngine::copy_latest`] reads it.

pub mod latest;
pub mod vis;

pub use latest::{LatestSamples, LATEST_CAPACITY};
pub use tubeamp_eq::EqParams;

/// Shared EQ parameters read by the audio callback.
static PARAMS: EqParams = EqParams::new();

/// The shared EQ parameters. Changes take effect on the next audio block.
pub fn params() -> &'static EqParams {
    &PARAMS
}

/// Why the audio engine could not start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AudioError {
    /// Not supported on this platform yet.
    Unsupported,
    /// No WebKit process owned by us has an audio object yet. Retry once playback has started.
    NoOwnedProcess,
    /// An engine is already running in this process.
    AlreadyRunning,
    /// A Core Audio call failed. `code` is the OSStatus or the internal step code.
    Core { stage: &'static str, code: i32 },
}

impl std::fmt::Display for AudioError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AudioError::Unsupported => write!(f, "audio tap is not supported on this platform yet"),
            AudioError::NoOwnedProcess => {
                write!(f, "no WebKit audio process yet (start playback first)")
            }
            AudioError::AlreadyRunning => write!(f, "audio engine is already running"),
            AudioError::Core { stage, code } => {
                write!(f, "audio tap failed at {stage} (code {code})")
            }
        }
    }
}

impl std::error::Error for AudioError {}

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::AudioEngine;

#[cfg(not(target_os = "macos"))]
mod stub;
#[cfg(not(target_os = "macos"))]
pub use stub::AudioEngine;
