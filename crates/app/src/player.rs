//! What the bridge reports about the player, and how it maps onto the main window.

use tubeamp_bridge::Evt;
use tubeamp_ui::{labels, Status};

/// Bitrate shown while playing when the bridge cannot read one (Winamp shows 128 for MP3s).
pub const FALLBACK_KBPS: u32 = 128;
/// Sample rate shown before the audio tap is running (Winamp's 44 kHz).
pub const FALLBACK_KHZ: u32 = 44;

/// The player as last reported by the bridge.
#[derive(Debug, Clone, PartialEq)]
pub struct Playback {
    pub video_id: Option<String>,
    pub title: String,
    pub author: String,
    /// Whole seconds elapsed.
    pub time_secs: u32,
    /// Whole seconds in total (0 when unknown).
    pub duration_secs: u32,
    pub status: Status,
    /// Raw YouTube player state, kept for the log.
    pub ps: Option<i32>,
    pub kbps: Option<u32>,
    pub codec: Option<String>,
}

impl Default for Playback {
    fn default() -> Self {
        Playback {
            video_id: None,
            title: String::new(),
            author: String::new(),
            time_secs: 0,
            duration_secs: 0,
            status: Status::Stop,
            ps: None,
            kbps: None,
            codec: None,
        }
    }
}

/// Maps a YouTube player state to the main window's status. Buffering counts as playing, as in
/// Winamp. Unstarted (-1), ended (0), cued (5) and anything unknown count as stopped.
pub fn status_from_ps(ps: Option<i32>) -> Status {
    match ps {
        Some(1) | Some(3) => Status::Play,
        Some(2) => Status::Pause,
        _ => Status::Stop,
    }
}

/// Whole seconds from a bridge time value. Missing, negative and non-finite values are 0.
pub fn whole_secs(value: Option<f64>) -> u32 {
    match value {
        Some(v) if v.is_finite() && v > 0.0 => v.floor() as u32,
        _ => 0,
    }
}

impl Playback {
    /// Applies a bridge event. Only `state` events change the playback.
    pub fn apply(&mut self, evt: &Evt) {
        if let Evt::State {
            video_id,
            title,
            author,
            cur,
            dur,
            ps,
            kbps,
            codec,
        } = evt
        {
            self.video_id = video_id.clone();
            self.title = title.clone().unwrap_or_default();
            self.author = author.clone().unwrap_or_default();
            self.time_secs = whole_secs(*cur);
            self.duration_secs = whole_secs(*dur);
            self.ps = *ps;
            self.status = status_from_ps(*ps);
            self.kbps = kbps
                .filter(|k| k.is_finite() && *k > 0.0)
                .map(|k| k.round() as u32);
            self.codec = codec.clone();
        }
    }

    /// The bitrate to show: the bridge's figure, or 128 while playing when there is none.
    /// `None` keeps the caller's previous value (the window hides it while stopped anyway).
    pub fn kbps_for_display(&self) -> Option<u32> {
        self.kbps.or(match self.status {
            Status::Play => Some(FALLBACK_KBPS),
            _ => None,
        })
    }

    /// The marquee text with the track's playlist number (1 for a track that is not in the
    /// playlist).
    pub fn marquee_text_numbered(&self, number: u32) -> String {
        labels::now_playing_numbered(number, &self.author, &self.title, self.duration_secs)
            .unwrap_or_else(|| labels::IDLE_TEXT.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(ps: Option<i32>) -> Evt {
        Evt::State {
            video_id: Some("kJQP7kiw5Fk".to_string()),
            title: Some("Despacito".to_string()),
            author: Some("Luis Fonsi & Daddy Yankee".to_string()),
            cur: Some(83.7),
            dur: Some(229.4),
            ps,
            kbps: None,
            codec: None,
        }
    }

    #[test]
    fn player_states_map_to_the_window_status() {
        assert_eq!(status_from_ps(Some(1)), Status::Play);
        assert_eq!(status_from_ps(Some(3)), Status::Play);
        assert_eq!(status_from_ps(Some(2)), Status::Pause);
        assert_eq!(status_from_ps(Some(0)), Status::Stop);
        assert_eq!(status_from_ps(Some(-1)), Status::Stop);
        assert_eq!(status_from_ps(Some(5)), Status::Stop);
        assert_eq!(status_from_ps(None), Status::Stop);
    }

    #[test]
    fn times_are_whole_seconds_and_ignore_bad_values() {
        assert_eq!(whole_secs(Some(83.7)), 83);
        assert_eq!(whole_secs(Some(-1.0)), 0);
        assert_eq!(whole_secs(Some(f64::NAN)), 0);
        assert_eq!(whole_secs(Some(f64::INFINITY)), 0);
        assert_eq!(whole_secs(None), 0);
    }

    #[test]
    fn apply_copies_the_state_event() {
        let mut p = Playback::default();
        p.apply(&state(Some(1)));
        assert_eq!(p.video_id.as_deref(), Some("kJQP7kiw5Fk"));
        assert_eq!(p.time_secs, 83);
        assert_eq!(p.duration_secs, 229);
        assert_eq!(p.status, Status::Play);
        assert_eq!(
            p.marquee_text_numbered(labels::TRACK_NUMBER),
            "1. Luis Fonsi & Daddy Yankee - Despacito (3:49)"
        );
    }

    #[test]
    fn apply_ignores_other_events() {
        let mut p = Playback::default();
        p.apply(&state(Some(1)));
        let before = p.clone();
        p.apply(&Evt::Ended);
        p.apply(&Evt::Error {
            at: "x".to_string(),
            msg: "y".to_string(),
        });
        assert_eq!(p, before);
    }

    #[test]
    fn marquee_uses_the_given_playlist_number() {
        let mut p = Playback::default();
        p.apply(&state(Some(1)));
        assert_eq!(
            p.marquee_text_numbered(7),
            "7. Luis Fonsi & Daddy Yankee - Despacito (3:49)"
        );
    }

    #[test]
    fn idle_player_shows_the_app_name() {
        assert_eq!(
            Playback::default().marquee_text_numbered(labels::TRACK_NUMBER),
            "TubeAmp"
        );
    }

    #[test]
    fn kbps_falls_back_to_128_only_while_playing() {
        let mut p = Playback::default();
        p.apply(&state(Some(1)));
        assert_eq!(p.kbps_for_display(), Some(128));
        p.apply(&state(Some(2)));
        assert_eq!(p.kbps_for_display(), None);
        let reported = Evt::State {
            video_id: None,
            title: None,
            author: None,
            cur: None,
            dur: None,
            ps: Some(1),
            kbps: Some(160.4),
            codec: Some("opus".to_string()),
        };
        p.apply(&reported);
        assert_eq!(p.kbps_for_display(), Some(160));
        assert_eq!(p.codec.as_deref(), Some("opus"));
    }
}
