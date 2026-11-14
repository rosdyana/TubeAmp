//! The global shortcuts, as one table. Native key events (winit) and the keys the YouTube Music
//! page forwards in the video view (`Evt::Key`) are both turned into a [`Stroke`], and
//! [`shortcut_for`] decides what it does, so the two paths cannot drift apart.
//!
//! Winamp's bindings: Z, X, C, V, B for previous, play, pause, stop and next; the arrow keys seek
//! and change the volume; Ctrl+D doubles, Ctrl+T toggles the time, Alt+V and L toggle the video,
//! Alt+L the media library, and Ctrl+M the Milkdrop window (a [`Stroke`] does not carry Shift, so
//! it is the plain Ctrl letter). Webamp has no key for windowshade, so none is bound here.
//! Modifiers other than Ctrl and Alt (Cmd or the Windows key) are never shortcuts: on macOS Cmd+V
//! is paste, not Stop.

/// The key of a stroke: a letter in lower case, an arrow, or Escape. Other keys are not shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyName {
    Char(char),
    Left,
    Right,
    Up,
    Down,
    Escape,
}

/// One key press with the modifiers that matter for shortcuts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stroke {
    pub key: KeyName,
    pub ctrl: bool,
    pub alt: bool,
    /// Cmd on macOS, the Windows key elsewhere. Any such press is not a shortcut.
    pub meta: bool,
}

/// What a shortcut does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Shortcut {
    ToggleDouble,
    ToggleRemaining,
    ToggleLibrary,
    ToggleVideo,
    /// Shows or hides the Milkdrop window.
    ToggleMilkdrop,
    SeekBack,
    SeekForward,
    VolumeUp,
    VolumeDown,
    Previous,
    Play,
    Pause,
    Stop,
    Next,
    /// Leaves the video window's fullscreen. Does nothing (and is not handled) otherwise.
    Escape,
}

/// Maps a key to its shortcut, given whether the key is auto-repeating. Ctrl and Alt combinations
/// are checked first, as Winamp does: Ctrl+anything is a Ctrl shortcut or nothing; Alt+L and Alt+V
/// are Alt shortcuts, and other Alt presses fall through to the plain keys. Ctrl+Alt is nothing,
/// as is anything with Cmd or the Windows key.
pub fn shortcut_for(stroke: &Stroke, repeat: bool) -> Option<Shortcut> {
    if stroke.meta || (stroke.ctrl && stroke.alt) {
        return None;
    }
    if stroke.ctrl {
        if repeat {
            return None;
        }
        return match stroke.key {
            KeyName::Char('d') => Some(Shortcut::ToggleDouble),
            KeyName::Char('t') => Some(Shortcut::ToggleRemaining),
            KeyName::Char('l') => Some(Shortcut::ToggleLibrary),
            KeyName::Char('m') => Some(Shortcut::ToggleMilkdrop),
            _ => None,
        };
    }
    if stroke.alt && !repeat {
        match stroke.key {
            KeyName::Char('l') => return Some(Shortcut::ToggleLibrary),
            KeyName::Char('v') => return Some(Shortcut::ToggleVideo),
            _ => {}
        }
    }
    // Arrows and Escape repeat; letters do not.
    match stroke.key {
        KeyName::Left => Some(Shortcut::SeekBack),
        KeyName::Right => Some(Shortcut::SeekForward),
        KeyName::Up => Some(Shortcut::VolumeUp),
        KeyName::Down => Some(Shortcut::VolumeDown),
        KeyName::Escape => Some(Shortcut::Escape),
        _ if repeat => None,
        KeyName::Char('z') => Some(Shortcut::Previous),
        KeyName::Char('x') => Some(Shortcut::Play),
        KeyName::Char('c') => Some(Shortcut::Pause),
        KeyName::Char('v') => Some(Shortcut::Stop),
        KeyName::Char('b') => Some(Shortcut::Next),
        KeyName::Char('l') => Some(Shortcut::ToggleVideo),
        KeyName::Char(_) => None,
    }
}

/// The stroke for a key the page forwarded. `key` is `KeyboardEvent.key` and `code` is
/// `KeyboardEvent.code`. Letters come from `code` when it has one (`KeyV`), because under Alt on
/// macOS `key` is a symbol. Keys that are not shortcut candidates give `None`.
pub fn stroke_from_page(
    key: &str,
    code: &str,
    ctrl: bool,
    alt: bool,
    meta: bool,
) -> Option<Stroke> {
    let letter = code
        .strip_prefix("Key")
        .filter(|rest| rest.chars().count() == 1)
        .and_then(|rest| rest.chars().next());
    let name = match (letter, key) {
        (Some(c), _) => KeyName::Char(c.to_ascii_lowercase()),
        (None, "ArrowLeft") => KeyName::Left,
        (None, "ArrowRight") => KeyName::Right,
        (None, "ArrowUp") => KeyName::Up,
        (None, "ArrowDown") => KeyName::Down,
        (None, "Escape") => KeyName::Escape,
        (None, k) if k.chars().count() == 1 => {
            KeyName::Char(k.chars().next()?.to_ascii_lowercase())
        }
        _ => return None,
    };
    Some(Stroke {
        key: name,
        ctrl,
        alt,
        meta,
    })
}

/// The shortcut for a key the page forwarded, or `None` when the key runs nothing.
///
/// A page key only counts while the window that forwarded it is shown (`source_shown`): a page
/// can only have the keyboard while its window is up, so a key that arrives after the window was
/// hidden is stale, and it must not start or stop playback. `repeat` is `KeyboardEvent.repeat`, so
/// an auto-repeating key does what the same key does natively.
pub fn page_shortcut(
    key: &str,
    code: &str,
    ctrl: bool,
    alt: bool,
    meta: bool,
    repeat: bool,
    source_shown: bool,
) -> Option<Shortcut> {
    if !source_shown {
        return None;
    }
    let stroke = stroke_from_page(key, code, ctrl, alt, meta)?;
    shortcut_for(&stroke, repeat)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Keys the page handles in the video view, as `bridge.js` names them (`PLAIN_KEYS`): the
    /// arrow keys by `KeyboardEvent.key`, letters by their `code`. Checked against the script by a
    /// test below.
    const PAGE_PLAIN_KEYS: &[&str] = &[
        "arrowleft",
        "arrowright",
        "arrowup",
        "arrowdown",
        "z",
        "x",
        "c",
        "v",
        "b",
        "l",
    ];
    /// Letters with Ctrl (`CTRL_KEYS` in `bridge.js`).
    const PAGE_CTRL_KEYS: &[&str] = &["d", "t", "l", "m"];
    /// Letters with Alt (`ALT_KEYS` in `bridge.js`).
    const PAGE_ALT_KEYS: &[&str] = &["l", "v"];

    fn plain(key: KeyName) -> Stroke {
        Stroke {
            key,
            ctrl: false,
            alt: false,
            meta: false,
        }
    }

    fn with(key: KeyName, ctrl: bool, alt: bool, meta: bool) -> Stroke {
        Stroke {
            key,
            ctrl,
            alt,
            meta,
        }
    }

    #[test]
    fn winamp_letters_and_arrows_map_to_transport_and_seek() {
        assert_eq!(
            shortcut_for(&plain(KeyName::Char('z')), false),
            Some(Shortcut::Previous)
        );
        assert_eq!(
            shortcut_for(&plain(KeyName::Char('x')), false),
            Some(Shortcut::Play)
        );
        assert_eq!(
            shortcut_for(&plain(KeyName::Char('c')), false),
            Some(Shortcut::Pause)
        );
        assert_eq!(
            shortcut_for(&plain(KeyName::Char('v')), false),
            Some(Shortcut::Stop)
        );
        assert_eq!(
            shortcut_for(&plain(KeyName::Char('b')), false),
            Some(Shortcut::Next)
        );
        assert_eq!(
            shortcut_for(&plain(KeyName::Char('l')), false),
            Some(Shortcut::ToggleVideo)
        );
        assert_eq!(
            shortcut_for(&plain(KeyName::Left), false),
            Some(Shortcut::SeekBack)
        );
        assert_eq!(
            shortcut_for(&plain(KeyName::Right), false),
            Some(Shortcut::SeekForward)
        );
        assert_eq!(
            shortcut_for(&plain(KeyName::Up), false),
            Some(Shortcut::VolumeUp)
        );
        assert_eq!(
            shortcut_for(&plain(KeyName::Down), false),
            Some(Shortcut::VolumeDown)
        );
        assert_eq!(shortcut_for(&plain(KeyName::Char('q')), false), None);
    }

    #[test]
    fn ctrl_and_alt_keys_map_to_their_own_shortcuts() {
        assert_eq!(
            shortcut_for(&with(KeyName::Char('d'), true, false, false), false),
            Some(Shortcut::ToggleDouble)
        );
        assert_eq!(
            shortcut_for(&with(KeyName::Char('t'), true, false, false), false),
            Some(Shortcut::ToggleRemaining)
        );
        assert_eq!(
            shortcut_for(&with(KeyName::Char('l'), true, false, false), false),
            Some(Shortcut::ToggleLibrary)
        );
        assert_eq!(
            shortcut_for(&with(KeyName::Char('m'), true, false, false), false),
            Some(Shortcut::ToggleMilkdrop)
        );
        // Plain M is nothing, and so is Alt+M.
        assert_eq!(shortcut_for(&plain(KeyName::Char('m')), false), None);
        assert_eq!(
            shortcut_for(&with(KeyName::Char('m'), false, true, false), false),
            None
        );
        assert_eq!(
            shortcut_for(&with(KeyName::Char('l'), false, true, false), false),
            Some(Shortcut::ToggleLibrary)
        );
        assert_eq!(
            shortcut_for(&with(KeyName::Char('v'), false, true, false), false),
            Some(Shortcut::ToggleVideo)
        );
        // Ctrl+Z is not Previous, and Ctrl+Q is nothing.
        assert_eq!(
            shortcut_for(&with(KeyName::Char('z'), true, false, false), false),
            None
        );
        assert_eq!(
            shortcut_for(&with(KeyName::Char('q'), true, false, false), false),
            None
        );
    }

    #[test]
    fn other_alt_keys_fall_through_to_the_plain_keys_as_before() {
        assert_eq!(
            shortcut_for(&with(KeyName::Char('z'), false, true, false), false),
            Some(Shortcut::Previous)
        );
    }

    #[test]
    fn ctrl_plus_alt_and_cmd_are_never_shortcuts() {
        assert_eq!(
            shortcut_for(&with(KeyName::Char('d'), true, true, false), false),
            None
        );
        assert_eq!(
            shortcut_for(&with(KeyName::Char('z'), false, false, true), false),
            None
        );
        assert_eq!(
            shortcut_for(&with(KeyName::Char('v'), false, false, true), false),
            None
        );
    }

    #[test]
    fn repeats_only_move_the_arrows_and_leave_escape_alone() {
        assert_eq!(
            shortcut_for(&plain(KeyName::Left), true),
            Some(Shortcut::SeekBack)
        );
        assert_eq!(
            shortcut_for(&plain(KeyName::Up), true),
            Some(Shortcut::VolumeUp)
        );
        assert_eq!(shortcut_for(&plain(KeyName::Char('z')), true), None);
        assert_eq!(
            shortcut_for(&with(KeyName::Char('d'), true, false, false), true),
            None
        );
        assert_eq!(
            shortcut_for(&plain(KeyName::Escape), false),
            Some(Shortcut::Escape)
        );
    }

    #[test]
    fn page_letters_come_from_the_code_under_alt() {
        // macOS: Option+V gives key "√" but code "KeyV".
        assert_eq!(
            stroke_from_page("√", "KeyV", false, true, false),
            Some(Stroke {
                key: KeyName::Char('v'),
                ctrl: false,
                alt: true,
                meta: false,
            })
        );
        // Shift+Z gives key "Z" with code "KeyZ": the same shortcut as z.
        assert_eq!(
            stroke_from_page("Z", "KeyZ", false, false, false).map(|s| s.key),
            Some(KeyName::Char('z'))
        );
    }

    #[test]
    fn page_arrows_and_symbols_map_by_key_name() {
        assert_eq!(
            stroke_from_page("ArrowLeft", "ArrowLeft", false, false, false).map(|s| s.key),
            Some(KeyName::Left)
        );
        assert_eq!(
            stroke_from_page("Escape", "Escape", false, false, false).map(|s| s.key),
            Some(KeyName::Escape)
        );
        assert_eq!(
            stroke_from_page("5", "Digit5", false, false, false).map(|s| s.key),
            Some(KeyName::Char('5'))
        );
        // Keys with no single character and no shortcut name are not strokes.
        assert_eq!(
            stroke_from_page("Shift", "ShiftLeft", false, false, false),
            None
        );
        assert_eq!(stroke_from_page("", "", false, false, false), None);
    }

    #[test]
    fn a_page_key_runs_its_shortcut_only_while_the_video_window_is_shown() {
        assert_eq!(
            page_shortcut("x", "KeyX", false, false, false, false, true),
            Some(Shortcut::Play)
        );
        assert_eq!(
            page_shortcut("x", "KeyX", false, false, false, false, false),
            None,
            "a key from a hidden video window is stale"
        );
        assert_eq!(
            page_shortcut("ArrowLeft", "ArrowLeft", false, false, false, false, false),
            None
        );
    }

    #[test]
    fn a_repeated_page_key_does_not_repeat_a_press_only_shortcut() {
        assert_eq!(
            page_shortcut("x", "KeyX", false, false, false, true, true),
            None,
            "holding x must not fire Play again and again"
        );
        assert_eq!(
            page_shortcut("ArrowRight", "ArrowRight", false, false, false, true, true),
            Some(Shortcut::SeekForward),
            "held arrows keep seeking, as they do natively"
        );
    }

    #[test]
    fn page_keys_match_the_script() {
        // The lists TubeAmp runs must be the lists the page prevents, so they are checked here
        // against the text of bridge.js.
        let script = include_str!("../../bridge/src/bridge.js");
        let quoted = |list: &[&str]| {
            list.iter()
                .map(|k| format!("\"{k}\""))
                .collect::<Vec<_>>()
                .join(", ")
        };
        for (name, list) in [
            ("PLAIN_KEYS", PAGE_PLAIN_KEYS),
            ("CTRL_KEYS", PAGE_CTRL_KEYS),
            ("ALT_KEYS", PAGE_ALT_KEYS),
        ] {
            let line = format!("const {name} = [{}];", quoted(list));
            assert!(script.contains(&line), "bridge.js lacks `{line}`");
        }
    }
}
