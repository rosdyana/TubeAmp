//! Messages between Rust and `bridge.js`: JSON objects tagged by `"t"`.
//!
//! Keep this in sync with `bridge.js`. The JSON keys are the Rust field names.

use serde::{Deserialize, Serialize};

/// Rust → page, sent with `window.__tubeamp.cmd(...)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Cmd {
    /// `playVideo()`.
    Play,
    /// `pauseVideo()`.
    Pause,
    /// Play when paused, pause when playing.
    Toggle,
    /// Pause and rewind to 0.
    Stop,
    /// `nextVideo()`.
    Next,
    /// Rewind if more than 3 s in (Winamp's behaviour), otherwise `previousVideo()`.
    Prev,
    /// `seekTo(secs)`, in seconds from the start.
    Seek { secs: f64 },
    /// `setVolume(level)`, `level` in `0..=100`.
    Volume { level: u8 },
    /// `loadVideoById(video_id)`: load and start a video.
    Load { video_id: String },
}

/// Page → Rust, posted with `window.ipc.postMessage(JSON)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "lowercase")]
pub enum Evt {
    /// Player snapshot: every 250 ms while playing, every 1000 ms otherwise.
    State {
        #[serde(default)]
        video_id: Option<String>,
        #[serde(default)]
        title: Option<String>,
        #[serde(default)]
        author: Option<String>,
        /// Current position in seconds.
        #[serde(default)]
        cur: Option<f64>,
        /// Duration in seconds.
        #[serde(default)]
        dur: Option<f64>,
        /// YouTube player state: -1 unstarted, 0 ended, 1 playing, 2 paused, 3 buffering, 5 cued.
        #[serde(default)]
        ps: Option<i32>,
        /// Audio bitrate from the player's stats, when it can be read.
        #[serde(default)]
        kbps: Option<f64>,
        /// Audio codec from the player's stats, when it can be read.
        #[serde(default)]
        codec: Option<String>,
    },
    /// The current video reached its end (player state 0).
    Ended,
    /// Something failed in the page. `at` names the place, `msg` is the message.
    Error {
        #[serde(rename = "where")]
        at: String,
        msg: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn round_trip_cmd(cmd: Cmd) {
        let json = serde_json::to_string(&cmd).expect("serialize");
        let back: Cmd = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, cmd, "via {json}");
    }

    fn round_trip_evt(evt: Evt) {
        let json = serde_json::to_string(&evt).expect("serialize");
        let back: Evt = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, evt, "via {json}");
    }

    #[test]
    fn every_cmd_round_trips() {
        round_trip_cmd(Cmd::Play);
        round_trip_cmd(Cmd::Pause);
        round_trip_cmd(Cmd::Toggle);
        round_trip_cmd(Cmd::Stop);
        round_trip_cmd(Cmd::Next);
        round_trip_cmd(Cmd::Prev);
        round_trip_cmd(Cmd::Seek { secs: 83.25 });
        round_trip_cmd(Cmd::Volume { level: 75 });
        round_trip_cmd(Cmd::Load {
            video_id: "kJQP7kiw5Fk".to_string(),
        });
    }

    #[test]
    fn cmd_wire_format_matches_bridge_js() {
        assert_eq!(
            serde_json::to_string(&Cmd::Play).unwrap(),
            r#"{"t":"play"}"#
        );
        assert_eq!(
            serde_json::to_string(&Cmd::Seek { secs: 12.5 }).unwrap(),
            r#"{"t":"seek","secs":12.5}"#
        );
        assert_eq!(
            serde_json::to_string(&Cmd::Volume { level: 40 }).unwrap(),
            r#"{"t":"volume","level":40}"#
        );
        assert_eq!(
            serde_json::to_string(&Cmd::Load {
                video_id: "abc".to_string()
            })
            .unwrap(),
            r#"{"t":"load","video_id":"abc"}"#
        );
    }

    #[test]
    fn every_evt_round_trips() {
        round_trip_evt(Evt::State {
            video_id: Some("kJQP7kiw5Fk".to_string()),
            title: Some("Despacito".to_string()),
            author: Some("Luis Fonsi".to_string()),
            cur: Some(12.25),
            dur: Some(229.0),
            ps: Some(1),
            kbps: Some(128.0),
            codec: Some("opus".to_string()),
        });
        round_trip_evt(Evt::State {
            video_id: None,
            title: None,
            author: None,
            cur: None,
            dur: None,
            ps: None,
            kbps: None,
            codec: None,
        });
        round_trip_evt(Evt::Ended);
        round_trip_evt(Evt::Error {
            at: "cmd:seek".to_string(),
            msg: "boom".to_string(),
        });
    }

    #[test]
    fn state_from_the_page_parses_with_nulls_and_missing_keys() {
        let json = r#"{"t":"state","video_id":"x","title":"T","author":null,"cur":1.5,"dur":200,"ps":1,"kbps":128,"codec":"mp4a.40.2"}"#;
        match serde_json::from_str::<Evt>(json).expect("parse") {
            Evt::State {
                video_id,
                author,
                dur,
                ps,
                kbps,
                ..
            } => {
                assert_eq!(video_id.as_deref(), Some("x"));
                assert_eq!(author, None);
                assert_eq!(dur, Some(200.0));
                assert_eq!(ps, Some(1));
                assert_eq!(kbps, Some(128.0));
            }
            other => panic!("unexpected {other:?}"),
        }
        let minimal = serde_json::from_str::<Evt>(r#"{"t":"state"}"#).expect("minimal");
        assert!(matches!(
            minimal,
            Evt::State {
                video_id: None,
                ps: None,
                ..
            }
        ));
    }

    #[test]
    fn error_uses_the_where_key() {
        let evt = serde_json::from_str::<Evt>(r#"{"t":"error","where":"video","msg":"x"}"#)
            .expect("parse");
        assert_eq!(
            evt,
            Evt::Error {
                at: "video".to_string(),
                msg: "x".to_string()
            }
        );
    }

    #[test]
    fn unknown_tags_are_rejected() {
        assert!(serde_json::from_str::<Evt>(r#"{"t":"nope"}"#).is_err());
        assert!(serde_json::from_str::<Cmd>(r#"{"t":"seek"}"#).is_err());
    }
}
