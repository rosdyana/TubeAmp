//! What the bridge reports about the player, and how it maps onto the main window.

use std::fmt;
use std::time::{Duration, Instant};

use tubeamp_bridge::Evt;
use tubeamp_ui::{labels, Status};

/// How often the state-update gaps are reported to the log.
const RATE_REPORT_EVERY: Duration = Duration::from_secs(5);

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
    /// The position from the last state update, in seconds (not rounded).
    cur_secs: f64,
    /// When the last state update arrived.
    updated_at: Option<Instant>,
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
            cur_secs: 0.0,
            updated_at: None,
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
    /// Applies a bridge event that arrived at `now`. Only `state` events change the playback.
    pub fn apply(&mut self, evt: &Evt, now: Instant) {
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
            self.cur_secs = match cur {
                Some(c) if c.is_finite() && *c > 0.0 => *c,
                _ => 0.0,
            };
            self.updated_at = Some(now);
        }
    }

    /// The whole seconds to show at `now`. While playing, this is the last update's position
    /// moved on by the time since it arrived, and clamped to the duration. So the clock ticks
    /// even when the bridge's updates come slowly. Otherwise it is the last update's time.
    pub fn time_at(&self, now: Instant) -> u32 {
        let playing = self.status == Status::Play;
        match self.updated_at {
            Some(at) if playing => {
                let mut secs = self.cur_secs + now.saturating_duration_since(at).as_secs_f64();
                if self.duration_secs > 0 {
                    secs = secs.min(f64::from(self.duration_secs));
                }
                secs.floor() as u32
            }
            _ => self.time_secs,
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

/// Measures the gaps between bridge state updates, for the log. Every few seconds it gives a
/// report of how many arrived and how far apart they were.
#[derive(Debug, Default)]
pub struct RateMeter {
    window_start: Option<Instant>,
    last: Option<Instant>,
    updates: u32,
    gaps: u32,
    gap_sum: Duration,
    gap_max: Duration,
}

/// What [`RateMeter`] measured over one report window.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RateReport {
    pub updates: u32,
    pub span_secs: f64,
    pub mean_gap_ms: f64,
    pub max_gap_ms: f64,
}

impl fmt::Display for RateReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} state updates in {:.1} s, gap mean {:.0} ms, max {:.0} ms",
            self.updates, self.span_secs, self.mean_gap_ms, self.max_gap_ms
        )
    }
}

impl RateMeter {
    /// Records one update at `now`. Returns a report once the window has run for five seconds.
    pub fn record(&mut self, now: Instant) -> Option<RateReport> {
        let start = *self.window_start.get_or_insert(now);
        if let Some(last) = self.last {
            let gap = now.saturating_duration_since(last);
            self.gaps += 1;
            self.gap_sum += gap;
            self.gap_max = self.gap_max.max(gap);
        }
        self.last = Some(now);
        self.updates += 1;

        let span = now.saturating_duration_since(start);
        if span < RATE_REPORT_EVERY {
            return None;
        }
        let mean_gap_ms = if self.gaps == 0 {
            0.0
        } else {
            self.gap_sum.as_secs_f64() * 1000.0 / f64::from(self.gaps)
        };
        let report = RateReport {
            updates: self.updates,
            span_secs: span.as_secs_f64(),
            mean_gap_ms,
            max_gap_ms: self.gap_max.as_secs_f64() * 1000.0,
        };
        self.window_start = Some(now);
        self.updates = 0;
        self.gaps = 0;
        self.gap_sum = Duration::ZERO;
        self.gap_max = Duration::ZERO;
        Some(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Any fixed instant: the tests only ever compare offsets from it.
    fn t0() -> Instant {
        Instant::now()
    }

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
        p.apply(&state(Some(1)), t0());
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
        p.apply(&state(Some(1)), t0());
        let before = p.clone();
        p.apply(&Evt::Ended, t0());
        p.apply(
            &Evt::Error {
                at: "x".to_string(),
                msg: "y".to_string(),
            },
            t0(),
        );
        assert_eq!(p, before);
    }

    #[test]
    fn marquee_uses_the_given_playlist_number() {
        let mut p = Playback::default();
        p.apply(&state(Some(1)), t0());
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
        p.apply(&state(Some(1)), t0());
        assert_eq!(p.kbps_for_display(), Some(128));
        p.apply(&state(Some(2)), t0());
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
        p.apply(&reported, t0());
        assert_eq!(p.kbps_for_display(), Some(160));
        assert_eq!(p.codec.as_deref(), Some("opus"));
    }

    #[test]
    fn time_ticks_on_between_updates_while_playing() {
        let mut p = Playback::default();
        let at = t0();
        p.apply(&state(Some(1)), at); // cur 83.7, dur 229.4
        assert_eq!(p.time_at(at), 83);
        assert_eq!(p.time_at(at + Duration::from_millis(1_400)), 85);
        assert_eq!(p.time_at(at + Duration::from_millis(2_600)), 86);
    }

    #[test]
    fn time_is_clamped_to_the_duration_and_frozen_when_paused() {
        let mut p = Playback::default();
        let at = t0();
        p.apply(&state(Some(1)), at);
        assert_eq!(p.time_at(at + Duration::from_secs(1_000)), 229);
        p.apply(&state(Some(2)), at);
        assert_eq!(p.time_at(at + Duration::from_secs(10)), 83);
    }

    #[test]
    fn time_without_a_duration_is_not_clamped() {
        let mut p = Playback::default();
        let at = t0();
        let evt = Evt::State {
            video_id: None,
            title: None,
            author: None,
            cur: Some(10.0),
            dur: None,
            ps: Some(1),
            kbps: None,
            codec: None,
        };
        p.apply(&evt, at);
        assert_eq!(p.time_at(at + Duration::from_secs(20)), 30);
        assert_eq!(Playback::default().time_at(at), 0);
    }

    /// Feeds `n` updates `gap` apart from `at`. Returns the instant after the last one and the last
    /// report, if any came.
    fn feed(
        meter: &mut RateMeter,
        mut at: Instant,
        gap: Duration,
        n: u32,
    ) -> (Instant, Option<RateReport>) {
        let mut last = None;
        for _ in 0..n {
            if let Some(report) = meter.record(at) {
                last = Some(report);
            }
            at += gap;
        }
        (at, last)
    }

    #[test]
    fn rate_meter_reports_each_five_seconds() {
        let mut meter = RateMeter::default();
        // 250 ms updates from t = 0: the report comes at t = 5 s, on the 21st update.
        let (_, report) = feed(&mut meter, t0(), Duration::from_millis(250), 21);
        let report = report.expect("report after 5 s");
        assert_eq!(report.updates, 21);
        assert!(
            (report.span_secs - 5.0).abs() < 1e-9,
            "span {}",
            report.span_secs
        );
        assert!((report.mean_gap_ms - 250.0).abs() < 1e-6);
        assert!((report.max_gap_ms - 250.0).abs() < 1e-6);
        assert!(report.to_string().contains("state updates"));
    }

    #[test]
    fn rate_meter_keeps_the_worst_gap() {
        let mut meter = RateMeter::default();
        let start = t0();
        assert_eq!(meter.record(start), None);
        assert_eq!(meter.record(start + Duration::from_millis(250)), None);
        assert_eq!(meter.record(start + Duration::from_millis(1_250)), None);
        let report = meter
            .record(start + Duration::from_secs(5))
            .expect("report");
        assert!((report.max_gap_ms - 3_750.0).abs() < 1e-6);
        assert!((report.mean_gap_ms - (250.0 + 1_000.0 + 3_750.0) / 3.0).abs() < 1e-6);
    }
}
