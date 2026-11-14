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
    /// `cueVideoById(video_id, secs)`: load a video at `secs` without starting it (restores the
    /// last track paused, at its position).
    Cue { video_id: String, secs: f64 },
    /// Switches the page between the video view and the full YouTube Music page. `video: true`
    /// shows only the player's video, filling the view. `false` shows the whole page (browser mode).
    SetViewMode { video: bool },
    /// Calls YouTube Music's InnerTube `endpoint` (e.g. `"search"`, `"browse"`, `"next"`) from
    /// inside the page with `body` merged over the page's `INNERTUBE_CONTEXT`. Waits until the page
    /// is ready. The answer comes back as `Evt::ApiResult` with the same `req`.
    Api {
        req: u64,
        endpoint: String,
        body: serde_json::Value,
    },
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
        /// Cover art URL from the page's media session artwork, when the page has one.
        #[serde(default)]
        thumbnail: Option<String>,
    },
    /// The current video reached its end (player state 0).
    Ended,
    /// Escape was pressed while the page had focus. The video window uses it to leave fullscreen,
    /// since the webview takes the key events once it has focus.
    Escape,
    /// A key pressed while the page shows the video (not the full page) and the target is not a
    /// text field. The app runs its own shortcut for it, as if the key had come to its window.
    /// `key` is `KeyboardEvent.key`, and `code` is `KeyboardEvent.code` (for letters under Alt,
    /// where the key is a symbol on macOS).
    Key {
        key: String,
        code: String,
        #[serde(default)]
        ctrl: bool,
        #[serde(default)]
        alt: bool,
        #[serde(default)]
        shift: bool,
        #[serde(default)]
        meta: bool,
        /// The key is auto-repeating (`KeyboardEvent.repeat`). Repeats never run a press-only
        /// shortcut (play, pause, stop, the letters), as they do not for native keys.
        #[serde(default)]
        repeat: bool,
    },
    /// Something failed in the page. `at` names the place, `msg` is the message.
    Error {
        #[serde(rename = "where")]
        at: String,
        msg: String,
    },
    /// The page's `ytcfg` is ready, so InnerTube requests can run. Posted once per page load.
    Ready { signed_in: bool },
    /// The answer to `Cmd::Api` with the same `req`. `body` is the raw response text when `ok`,
    /// otherwise an error message. `status` is the HTTP status, or 0 when the request never got one.
    ApiResult {
        req: u64,
        ok: bool,
        status: u16,
        body: String,
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
        round_trip_cmd(Cmd::SetViewMode { video: true });
        round_trip_cmd(Cmd::SetViewMode { video: false });
        round_trip_cmd(Cmd::Load {
            video_id: "kJQP7kiw5Fk".to_string(),
        });
        round_trip_cmd(Cmd::Cue {
            video_id: "kJQP7kiw5Fk".to_string(),
            secs: 42.5,
        });
        round_trip_cmd(Cmd::Api {
            req: 7,
            endpoint: "search".to_string(),
            body: serde_json::json!({"query": "daft punk", "params": "EgWKAQIIAWoKEAMQBBAJEAoQBQ=="}),
        });
    }

    #[test]
    fn api_cmd_wire_format_matches_bridge_js() {
        let json = serde_json::to_string(&Cmd::Api {
            req: 7,
            endpoint: "browse".to_string(),
            body: serde_json::json!({"browseId": "FEmusic_home"}),
        })
        .unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "t": "api",
                "req": 7,
                "endpoint": "browse",
                "body": {"browseId": "FEmusic_home"}
            })
        );
    }

    #[test]
    fn cue_wire_format_matches_bridge_js() {
        assert_eq!(
            serde_json::to_string(&Cmd::Cue {
                video_id: "abc".to_string(),
                secs: 12.5
            })
            .unwrap(),
            r#"{"t":"cue","video_id":"abc","secs":12.5}"#
        );
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
    fn set_view_mode_wire_format_matches_bridge_js() {
        assert_eq!(
            serde_json::to_string(&Cmd::SetViewMode { video: true }).unwrap(),
            r#"{"t":"setviewmode","video":true}"#
        );
        assert_eq!(
            serde_json::from_str::<Cmd>(r#"{"t":"setviewmode","video":false}"#).unwrap(),
            Cmd::SetViewMode { video: false }
        );
        assert!(serde_json::from_str::<Cmd>(r#"{"t":"setviewmode"}"#).is_err());
        assert_eq!(
            serde_json::from_str::<Evt>(r#"{"t":"escape"}"#).unwrap(),
            Evt::Escape
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
            thumbnail: Some("https://lh3.googleusercontent.com/cover=w226".to_string()),
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
            thumbnail: None,
        });
        round_trip_evt(Evt::Ended);
        round_trip_evt(Evt::Escape);
        round_trip_evt(Evt::Error {
            at: "cmd:seek".to_string(),
            msg: "boom".to_string(),
        });
        round_trip_evt(Evt::Ready { signed_in: true });
        round_trip_evt(Evt::Ready { signed_in: false });
        round_trip_evt(Evt::ApiResult {
            req: 7,
            ok: true,
            status: 200,
            body: r#"{"responseContext":{}}"#.to_string(),
        });
        round_trip_evt(Evt::ApiResult {
            req: 8,
            ok: false,
            status: 0,
            body: "TypeError: Failed to fetch".to_string(),
        });
    }

    #[test]
    fn ready_and_api_result_parse_from_bridge_js_shape() {
        assert_eq!(
            serde_json::from_str::<Evt>(r#"{"t":"ready","signed_in":true}"#).unwrap(),
            Evt::Ready { signed_in: true }
        );
        assert_eq!(
            serde_json::from_str::<Evt>(
                r#"{"t":"apiresult","req":3,"ok":false,"status":403,"body":"forbidden"}"#
            )
            .unwrap(),
            Evt::ApiResult {
                req: 3,
                ok: false,
                status: 403,
                body: "forbidden".to_string()
            }
        );
        assert!(serde_json::from_str::<Evt>(r#"{"t":"ready"}"#).is_err());
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
    fn key_evt_round_trips_with_its_modifiers() {
        round_trip_evt(Evt::Key {
            key: "z".to_string(),
            code: "KeyZ".to_string(),
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
            repeat: false,
        });
        round_trip_evt(Evt::Key {
            key: "√".to_string(),
            code: "KeyV".to_string(),
            ctrl: false,
            alt: true,
            shift: true,
            meta: true,
            repeat: true,
        });
    }

    #[test]
    fn key_evt_wire_format_matches_bridge_js() {
        assert_eq!(
            serde_json::from_str::<Evt>(
                r#"{"t":"key","key":"ArrowLeft","code":"ArrowLeft","ctrl":false,"alt":false,"shift":true,"meta":false}"#
            )
            .unwrap(),
            Evt::Key {
                key: "ArrowLeft".to_string(),
                code: "ArrowLeft".to_string(),
                ctrl: false,
                alt: false,
                shift: true,
                meta: false,
                repeat: false,
            }
        );
        // The modifier flags and the repeat flag default to false when the page leaves them out.
        assert_eq!(
            serde_json::from_str::<Evt>(r#"{"t":"key","key":"x","code":"KeyX"}"#).unwrap(),
            Evt::Key {
                key: "x".to_string(),
                code: "KeyX".to_string(),
                ctrl: false,
                alt: false,
                shift: false,
                meta: false,
                repeat: false,
            }
        );
        assert_eq!(
            serde_json::from_str::<Evt>(
                r#"{"t":"key","key":"x","code":"KeyX","repeat":true}"#
            )
            .unwrap(),
            Evt::Key {
                key: "x".to_string(),
                code: "KeyX".to_string(),
                ctrl: false,
                alt: false,
                shift: false,
                meta: false,
                repeat: true,
            }
        );
        assert!(serde_json::from_str::<Evt>(r#"{"t":"key","key":"x"}"#).is_err());
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
