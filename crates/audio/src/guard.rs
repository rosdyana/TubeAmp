//! Denial guard: notices when the audio tap is silent while YouTube Music plays.
//!
//! The macOS tap mutes YouTube Music's own output (`CATapMutedWhenTapped`). Without the
//! System Audio Recording permission the tap delivers silence, so the user would hear nothing.
//! The app asks [`denial_suspected`] on every tick. When it returns true, the app drops the
//! engine (the tap is torn down and the original output plays again) and waits for an explicit
//! retry.
//!
//! Everything here is plain logic with no Core Audio calls, so it is unit-tested with synthetic
//! buffers and instants.

use std::time::{Duration, Instant};

/// How long the tap may be silent while the page plays before we assume it has no permission.
/// Digital silence in a real song (a quiet intro, a gap) is normally shorter than this.
pub const DENIAL_WINDOW: Duration = Duration::from_secs(4);

/// True when every sample is exactly `0.0` (digital silence). NaN counts as sound. An empty
/// block counts as silent; the callback never passes one.
pub fn is_silent(samples: &[f32]) -> bool {
    samples.iter().all(|&s| s == 0.0)
}

/// Whether the tap looks denied (or dead) at `now`.
///
/// - `last_sound`: the last time the tap delivered a non-zero sample, or when the tap started if
///   it never has. A tap whose IOProc never runs has `last_sound` at its start.
/// - `playing_since`: when the page began reporting "playing" (`ps == 1`), or `None` if it is not
///   playing now.
/// - `output_wanted`: the app's volume is above zero. At zero the silence is the user's choice.
///
/// It is suspected when the tap has been silent *and* the page has been playing for at least
/// `window`, counted from the later of the two moments. So a track that starts after a long
/// silence is measured from when it started playing, and a silent gap shorter than `window` never
/// counts.
pub fn denial_suspected(
    now: Instant,
    last_sound: Instant,
    playing_since: Option<Instant>,
    output_wanted: bool,
    window: Duration,
) -> bool {
    let Some(playing_since) = playing_since else {
        return false;
    };
    if !output_wanted {
        return false;
    }
    let from = last_sound.max(playing_since);
    now.saturating_duration_since(from) >= window
}

#[cfg(test)]
mod tests {
    use super::*;

    const WINDOW: Duration = Duration::from_secs(4);

    #[test]
    fn exact_zeros_are_silent_and_anything_else_is_sound() {
        assert!(is_silent(&[0.0; 512]));
        assert!(is_silent(&[]));
        let mut block = [0.0f32; 512];
        block[300] = 1e-9;
        assert!(!is_silent(&block), "a tiny sample is still sound");
        block[300] = -0.0;
        assert!(is_silent(&block), "negative zero is zero");
        block[0] = f32::NAN;
        assert!(!is_silent(&[f32::NAN]), "NaN is not silence");
    }

    #[test]
    fn silence_for_the_whole_window_while_playing_is_a_denial() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_secs(10);
        assert!(denial_suspected(now, t0, Some(t0), true, WINDOW));
    }

    #[test]
    fn exactly_the_window_counts() {
        let t0 = Instant::now();
        assert!(!denial_suspected(
            t0 + Duration::from_millis(3999),
            t0,
            Some(t0),
            true,
            WINDOW
        ));
        assert!(denial_suspected(
            t0 + WINDOW,
            t0,
            Some(t0),
            true,
            WINDOW
        ));
    }

    #[test]
    fn a_recent_sound_resets_the_clock() {
        let t0 = Instant::now();
        // Playing since t0, but a sample arrived 3 s ago: only 1 s of silence so far.
        let now = t0 + Duration::from_secs(10);
        let last_sound = t0 + Duration::from_secs(7);
        assert!(!denial_suspected(now, last_sound, Some(t0), true, WINDOW));
    }

    #[test]
    fn the_clock_starts_when_the_page_starts_playing() {
        let t0 = Instant::now();
        // Silent since the tap started long ago, but the page only started playing 2 s ago.
        let now = t0 + Duration::from_secs(20);
        let playing_since = t0 + Duration::from_secs(18);
        assert!(!denial_suspected(now, t0, Some(playing_since), true, WINDOW));
        let later = t0 + Duration::from_secs(22);
        assert!(denial_suspected(later, t0, Some(playing_since), true, WINDOW));
    }

    #[test]
    fn a_page_that_is_not_playing_never_counts() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_secs(60);
        assert!(!denial_suspected(now, t0, None, true, WINDOW));
    }

    #[test]
    fn zero_volume_is_the_users_choice_not_a_denial() {
        let t0 = Instant::now();
        let now = t0 + Duration::from_secs(60);
        assert!(!denial_suspected(now, t0, Some(t0), false, WINDOW));
    }

    #[test]
    fn a_tap_that_never_ran_is_silent_from_its_start() {
        // No callback has ever run: last_sound stays at the tap's start time.
        let start = Instant::now();
        let playing = start + Duration::from_millis(500);
        let now = start + Duration::from_secs(5);
        assert!(denial_suspected(now, start, Some(playing), true, WINDOW));
    }
}
