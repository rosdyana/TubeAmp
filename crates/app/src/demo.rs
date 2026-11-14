//! Demo content for the headless `--render-main` and `--render-eq` renders.

use tubeamp_eq::{EqSettings, Preset};
use tubeamp_playlist::{Entry, Playlist};
use tubeamp_ui::playlist_window::PlaylistWindowState;
use tubeamp_ui::{EqWindowState, MainState, Status};

/// The equalizer state for `--render-eq`: flat, or the given built-in preset, focused.
pub fn eq_demo_state(preset: Option<&Preset>) -> EqWindowState {
    let mut settings = EqSettings::default();
    if let Some(p) = preset {
        settings.preamp = p.preamp;
        settings.bands = p.bands;
    }
    EqWindowState {
        settings,
        focused: true,
        ..EqWindowState::default()
    }
}

pub const DEMO_TITLE: &str = "TubeAmp - YouTube Music in a classic skin";
pub const DEMO_DURATION_SECS: u32 = 245;
const DEMO_KBPS: u32 = 128;
const DEMO_KHZ: u32 = 44;
const DEMO_VOLUME: u8 = 75;

/// The demo state at `time_secs` with the marquee at `marquee_offset` pixels.
pub fn demo_state(time_secs: u32, marquee_offset: u32, focused: bool) -> MainState {
    let mut st = MainState {
        title: DEMO_TITLE.to_string(),
        duration_secs: DEMO_DURATION_SECS,
        status: Status::Play,
        kbps: Some(DEMO_KBPS),
        khz: Some(DEMO_KHZ),
        stereo: true,
        volume: DEMO_VOLUME,
        balance: 0,
        focused,
        marquee_offset,
        ..MainState::default()
    };
    set_time(&mut st, time_secs);
    st
}

/// Sets the elapsed time and the matching position (time / duration).
fn set_time(st: &mut MainState, time_secs: u32) {
    st.time_secs = time_secs;
    st.position = if st.duration_secs == 0 {
        0.0
    } else {
        time_secs as f32 / st.duration_secs as f32
    };
}

/// Invented tracks for `--render-pl`: `(artist, title, seconds)`.
const PL_DEMO_TRACKS: [(&str, &str, u32); 12] = [
    ("Paper Lanterns", "Harbour Lights", 214),
    ("The Quiet Hours", "Glass Staircase", 187),
    ("Mira Solano", "Night Bus to Ayer", 243),
    ("Kaito Ren", "Slow Tide", 198),
    ("Velvet Engine", "Chromatic Rain", 256),
    ("Lantern & Wren", "Tuesday Again", 221),
    ("Ober Field", "Ninety Degrees", 175),
    ("Saltwater Choir", "Low Sun", 302),
    ("Tomo Pilar", "Street Light Hymn", 190),
    ("The Fernwood Set", "Morning Freight", 236),
    ("Ilse Varga", "Paper Moons", 208),
    ("Dune Harbour", "Last Ferry", 269),
];

/// The playlist for `--render-pl`: the 3rd track is current, and the 5th and 6th are selected.
pub fn pl_demo_playlist() -> Playlist {
    let mut pl = Playlist::from_entries(
        PL_DEMO_TRACKS
            .iter()
            .enumerate()
            .map(|(i, (artist, title, secs))| Entry {
                video_id: format!("demo{i:07}"),
                title: (*title).to_string(),
                artist: (*artist).to_string(),
                duration_secs: Some(*secs),
            })
            .collect(),
    );
    pl.set_current(Some(2));
    pl.click(4);
    pl.ctrl_click(5);
    pl
}

/// The playlist window state for `--render-pl`: mini time at 01:23, at the given size.
pub fn pl_demo_state(size_steps: (u32, u32)) -> PlaylistWindowState {
    PlaylistWindowState {
        size_steps,
        focused: true,
        mini_time_secs: Some(83),
        ..PlaylistWindowState::default()
    }
}
