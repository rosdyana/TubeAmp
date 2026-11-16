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
    /// Tells the page whether to click YouTube's own "Skip" button on ads as soon as it is offered
    /// (`on: true`). Unskippable ads still play in full. The page starts with it off.
    SkipAds { on: bool },
    /// Calls YouTube Music's InnerTube `endpoint` (e.g. `"search"`, `"browse"`, `"next"`) from
    /// inside the page with `body` merged over the page's `INNERTUBE_CONTEXT`. Waits until the page
    /// is ready. The answer comes back as `Evt::ApiResult` with the same `req`.
    Api {
        req: u64,
        endpoint: String,
        body: serde_json::Value,
    },
    /// Sets the Now Playing entry to TubeAmp's track: the page's `navigator.mediaSession.metadata`
    /// (macOS only; the page ignores it elsewhere). An empty `title` clears it. `artwork` is a
    /// cover URL, when there is one.
    NowPlaying {
        title: String,
        artist: String,
        album: String,
        artwork: Option<String>,
    },
    /// The EQ for the page's Web Audio graph (Windows only; the page ignores it on macOS, where the
    /// tap does the EQ in Rust). `preamp_db` and `bands_db` are the settings in dB (see
    /// `tubeamp_eq::value_to_db`), with `bands_db` in `BANDS_HZ` order. `balance` is -100..=100 with
    /// the sign of `EqParams::set_balance`.
    Eq {
        on: bool,
        preamp_db: f32,
        bands_db: [f32; 10],
        balance: i8,
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
        /// True while YouTube shows an ad in the player.
        #[serde(default)]
        ad: bool,
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
    /// A media-session action from the OS (a media key, Control Center, the Now Playing menu), as
    /// the bridge's handler got it. `action` is the MediaSession action name: `play`, `pause`,
    /// `stop`, `previoustrack`, `nexttrack`, `seekto`, `seekbackward` or `seekforward`. `seek_time`
    /// is the position for `seekto`, and the offset for `seekbackward` and `seekforward`, when the
    /// event gives one.
    Media {
        action: String,
        #[serde(default)]
        seek_time: Option<f64>,
    },
    /// Post-EQ mono samples from the page's Web Audio graph (Windows only; macOS never gets these).
    /// `rate` is the graph's sample rate in Hz, and `data` is a block of samples in -1..=1.
    Samples { rate: u32, data: Vec<f32> },
    /// YouTube offered the Skip button on an ad, and the page is skipping it (`on` on). `x` and `y`
    /// are the centre of the button in CSS pixels of the page's viewport. Rust answers with a trusted
    /// click there (Windows only), since YouTube ignores a script click.
    AdSkip { x: f64, y: f64 },
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
        round_trip_cmd(Cmd::SkipAds { on: true });
        round_trip_cmd(Cmd::SkipAds { on: false });
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
        round_trip_cmd(Cmd::NowPlaying {
            title: "Despacito".to_string(),
            artist: "Luis Fonsi".to_string(),
            album: String::new(),
            artwork: Some("https://lh3.googleusercontent.com/cover=w226".to_string()),
        });
        round_trip_cmd(Cmd::NowPlaying {
            title: String::new(),
            artist: String::new(),
            album: String::new(),
            artwork: None,
        });
        round_trip_cmd(Cmd::Eq {
            on: true,
            preamp_db: -3.5,
            bands_db: [0.0, 1.5, -2.0, 0.0, 6.0, 0.0, 0.0, 0.0, 0.0, -12.0],
            balance: -40,
        });
    }

    #[test]
    fn eq_wire_format_matches_bridge_js() {
        let mut bands_db = [0.0f32; 10];
        bands_db[4] = 6.0;
        assert_eq!(
            serde_json::to_string(&Cmd::Eq {
                on: true,
                preamp_db: -3.5,
                bands_db,
                balance: -40,
            })
            .unwrap(),
            r#"{"t":"eq","on":true,"preamp_db":-3.5,"bands_db":[0.0,0.0,0.0,0.0,6.0,0.0,0.0,0.0,0.0,0.0],"balance":-40}"#
        );
        assert!(serde_json::from_str::<Cmd>(r#"{"t":"eq","on":true}"#).is_err());
    }

    #[test]
    fn samples_wire_format_matches_bridge_js() {
        assert_eq!(
            serde_json::from_str::<Evt>(r#"{"t":"samples","rate":48000,"data":[0.5,-0.25]}"#)
                .unwrap(),
            Evt::Samples {
                rate: 48000,
                data: vec![0.5, -0.25],
            }
        );
        assert!(serde_json::from_str::<Evt>(r#"{"t":"samples","rate":48000}"#).is_err());
    }

    #[test]
    fn now_playing_wire_format_matches_bridge_js() {
        assert_eq!(
            serde_json::to_string(&Cmd::NowPlaying {
                title: "T".to_string(),
                artist: "A".to_string(),
                album: "".to_string(),
                artwork: Some("https://example.invalid/c.jpg".to_string()),
            })
            .unwrap(),
            r#"{"t":"nowplaying","title":"T","artist":"A","album":"","artwork":"https://example.invalid/c.jpg"}"#
        );
        assert_eq!(
            serde_json::to_string(&Cmd::NowPlaying {
                title: "T".to_string(),
                artist: "A".to_string(),
                album: "".to_string(),
                artwork: None,
            })
            .unwrap(),
            r#"{"t":"nowplaying","title":"T","artist":"A","album":"","artwork":null}"#
        );
        assert!(serde_json::from_str::<Cmd>(r#"{"t":"nowplaying","title":"T"}"#).is_err());
    }

    #[test]
    fn media_evt_round_trips_with_and_without_a_seek_time() {
        round_trip_evt(Evt::Media {
            action: "play".to_string(),
            seek_time: None,
        });
        round_trip_evt(Evt::Media {
            action: "seekto".to_string(),
            seek_time: Some(83.5),
        });
    }

    #[test]
    fn media_evt_wire_format_matches_bridge_js() {
        assert_eq!(
            serde_json::from_str::<Evt>(r#"{"t":"media","action":"nexttrack"}"#).unwrap(),
            Evt::Media {
                action: "nexttrack".to_string(),
                seek_time: None,
            }
        );
        assert_eq!(
            serde_json::from_str::<Evt>(r#"{"t":"media","action":"seekto","seek_time":12.5}"#)
                .unwrap(),
            Evt::Media {
                action: "seekto".to_string(),
                seek_time: Some(12.5),
            }
        );
        assert_eq!(
            serde_json::from_str::<Evt>(r#"{"t":"media","action":"seekforward","seek_time":null}"#)
                .unwrap(),
            Evt::Media {
                action: "seekforward".to_string(),
                seek_time: None,
            }
        );
        assert!(serde_json::from_str::<Evt>(r#"{"t":"media"}"#).is_err());
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
    fn skip_ads_wire_format_matches_bridge_js() {
        assert_eq!(
            serde_json::to_string(&Cmd::SkipAds { on: true }).unwrap(),
            r#"{"t":"skipads","on":true}"#
        );
        assert_eq!(
            serde_json::from_str::<Cmd>(r#"{"t":"skipads","on":false}"#).unwrap(),
            Cmd::SkipAds { on: false }
        );
        assert!(serde_json::from_str::<Cmd>(r#"{"t":"skipads"}"#).is_err());
    }

    #[test]
    fn state_ad_flag_defaults_to_false_and_parses_true() {
        match serde_json::from_str::<Evt>(r#"{"t":"state","ps":1}"#).expect("parse") {
            Evt::State { ad, .. } => assert!(!ad),
            other => panic!("unexpected {other:?}"),
        }
        match serde_json::from_str::<Evt>(r#"{"t":"state","ps":-1,"ad":true}"#).expect("parse") {
            Evt::State { ad, ps, .. } => {
                assert!(ad);
                assert_eq!(ps, Some(-1));
            }
            other => panic!("unexpected {other:?}"),
        }
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
            ad: false,
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
            ad: false,
        });
        round_trip_evt(Evt::State {
            video_id: None,
            title: None,
            author: None,
            cur: None,
            dur: None,
            ps: Some(-1),
            kbps: None,
            codec: None,
            thumbnail: None,
            ad: true,
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
        round_trip_evt(Evt::Media {
            action: "stop".to_string(),
            seek_time: None,
        });
        round_trip_evt(Evt::Samples {
            rate: 44100,
            data: vec![0.0, 0.5, -1.0],
        });
        round_trip_evt(Evt::AdSkip { x: 312.5, y: 48.0 });
    }

    #[test]
    fn adskip_wire_format_matches_bridge_js() {
        assert_eq!(
            serde_json::from_str::<Evt>(r#"{"t":"adskip","x":312.5,"y":48}"#).unwrap(),
            Evt::AdSkip { x: 312.5, y: 48.0 }
        );
        assert!(serde_json::from_str::<Evt>(r#"{"t":"adskip","x":1}"#).is_err());
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
