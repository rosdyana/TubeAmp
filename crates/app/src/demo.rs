//! Demo content for the headless `--render-main` render.

use tubeamp_ui::{MainState, Status};

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
