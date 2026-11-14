//! Saved settings: `config.toml` (windows, main window, EQ, skin, last track) and `playlist.json`
//! (the playlist, in the same format as LIST ▸ SAVE) in the config folder.
//!
//! Loading never fails the app: a missing file means defaults, and an unreadable one is moved
//! aside as `*.bad` and replaced by defaults, with a warning. Writes go to a temporary file that is
//! then renamed over the old one, so a crash never leaves a half-written file.
//!
//! This module knows nothing about the windows. `window/session.rs` copies the app's state in and
//! out of [`Config`].

use std::collections::BTreeMap;
use std::fs;
use std::io::{self, ErrorKind, Write};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tubeamp_playlist::Playlist;

/// The settings file, in the config folder.
pub const CONFIG_FILE: &str = "config.toml";
/// The playlist file, in the config folder.
pub const PLAYLIST_FILE: &str = "playlist.json";
/// The folder of skins (`*.wsz`), in the config folder.
pub const SKINS_DIR: &str = "skins";
/// The skin value meaning the built-in TubeAmp Classic skin.
pub const SKIN_DEFAULT: &str = "default";
/// The smallest share of a window (in pixels, each way) that must lie on a monitor for its saved
/// position to be kept.
const MIN_VISIBLE_PX: i32 = 64;

/// Where the settings are kept, and whether they are used at all (`--config-dir`, `--no-config`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Setup {
    pub dir: PathBuf,
    /// False with `--no-config`: nothing is read or written.
    pub enabled: bool,
}

/// The per-user config folder: `ProjectDirs("app", "TubeAmp", "TubeAmp")`. `None` when the OS has
/// no home folder.
pub fn default_dir() -> Option<PathBuf> {
    directories::ProjectDirs::from("app", "TubeAmp", "TubeAmp")
        .map(|dirs| dirs.config_dir().to_path_buf())
}

/// Everything TubeAmp remembers between runs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Window placement, by window name: `main`, `eq`, `playlist`, `library`, `video`, and any name
    /// written by a later version (kept as it is).
    pub windows: BTreeMap<String, WindowPrefs>,
    pub main: MainPrefs,
    pub eq: EqPrefs,
    /// `"default"`, or the absolute path of the chosen `.wsz`.
    pub skin: String,
    /// Whether the video window shows the whole YouTube Music page (the browser view).
    pub video_browser: bool,
    pub playlist: PlaylistPrefs,
    /// The track that was loaded when TubeAmp last quit. It is restored paused, not started.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_played: Option<LastPlayed>,
    /// Whether the first-run question about the classic skin has been answered.
    pub classic_skin_answered: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            windows: BTreeMap::new(),
            main: MainPrefs::default(),
            eq: EqPrefs::default(),
            skin: SKIN_DEFAULT.to_string(),
            video_browser: false,
            playlist: PlaylistPrefs::default(),
            last_played: None,
            classic_skin_answered: false,
        }
    }
}

/// One window's placement and state. Every field has a default, so older files still load.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct WindowPrefs {
    /// Top-left corner in physical pixels.
    pub x: i32,
    pub y: i32,
    pub visible: bool,
    /// The windowshade (main, equalizer and playlist).
    pub shade: bool,
    /// Playlist and library: the resize steps (see `playlist_window::DEFAULT_STEPS`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub steps: Option<[u32; 2]>,
    /// Video window: the frame size in skin pixels.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<[u32; 2]>,
}

/// The main window's controls and the double-size flag, which applies to every window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MainPrefs {
    pub volume: u8,
    pub balance: i8,
    pub shuffle: bool,
    pub repeat: bool,
    /// Show the time left instead of the time elapsed.
    pub remaining_mode: bool,
    /// `"spectrum"`, `"oscilloscope"` or `"off"`.
    pub vis_mode: String,
    pub doubled: bool,
}

impl Default for MainPrefs {
    fn default() -> Self {
        MainPrefs {
            // YouTube Music starts at full volume, as the app does without a config.
            volume: 100,
            balance: 0,
            shuffle: false,
            repeat: false,
            remaining_mode: false,
            vis_mode: "spectrum".to_string(),
            doubled: false,
        }
    }
}

/// The equalizer's curve and switches, as `.eqf` and the sliders hold them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct EqPrefs {
    pub on: bool,
    pub auto: bool,
    pub preamp: u8,
    pub bands: [u8; 10],
}

impl Default for EqPrefs {
    fn default() -> Self {
        let flat = tubeamp_eq::EqSettings::default();
        EqPrefs {
            on: flat.on,
            auto: flat.auto,
            preamp: flat.preamp,
            bands: flat.bands,
        }
    }
}

/// The playlist's current entry and scroll position. The entries are in `playlist.json`.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaylistPrefs {
    pub current: Option<usize>,
    pub scroll: usize,
}

/// The video that was loaded, and the position in seconds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LastPlayed {
    pub video_id: String,
    pub secs: f64,
}

/// What [`load`] found.
#[derive(Debug, Clone, PartialEq)]
pub struct Loaded {
    pub config: Config,
    /// Whether `config.toml` existed (false on the first run).
    pub existed: bool,
    pub playlist: Playlist,
    /// Problems to show the user, such as a corrupt file that was moved aside.
    pub warnings: Vec<String>,
}

/// Reads the settings. Never fails: problems become warnings and defaults.
pub fn load(setup: &Setup) -> Loaded {
    let mut warnings = Vec::new();
    if !setup.enabled {
        return Loaded {
            config: Config::default(),
            existed: false,
            playlist: Playlist::default(),
            warnings,
        };
    }
    let (config, existed) = match read_text(&setup.dir.join(CONFIG_FILE)) {
        Ok(None) => (Config::default(), false),
        Ok(Some(text)) => match parse_config(&text) {
            Ok(config) => (config, true),
            Err(e) => {
                set_aside(&setup.dir.join(CONFIG_FILE), &mut warnings);
                warnings.push(format!("config.toml could not be read ({e}); using defaults"));
                (Config::default(), true)
            }
        },
        Err(e) => {
            warnings.push(format!("config.toml could not be read ({e}); using defaults"));
            (Config::default(), true)
        }
    };
    let playlist = match read_text(&setup.dir.join(PLAYLIST_FILE)) {
        Ok(None) => Playlist::default(),
        Ok(Some(text)) => match Playlist::from_json(&text) {
            Ok(list) => list,
            Err(e) => {
                set_aside(&setup.dir.join(PLAYLIST_FILE), &mut warnings);
                warnings.push(format!("playlist.json could not be read ({e}); starting empty"));
                Playlist::default()
            }
        },
        Err(e) => {
            warnings.push(format!("playlist.json could not be read ({e}); starting empty"));
            Playlist::default()
        }
    };
    Loaded {
        config,
        existed,
        playlist,
        warnings,
    }
}

/// Parses `config.toml`, and keeps the values in their valid ranges.
pub fn parse_config(text: &str) -> Result<Config, toml::de::Error> {
    let mut config: Config = toml::from_str(text)?;
    config.eq.bands = config.eq.bands.map(|v| v.clamp(1, 64));
    config.eq.preamp = config.eq.preamp.clamp(1, 64);
    config.main.volume = config.main.volume.min(100);
    config.main.balance = config.main.balance.clamp(-100, 100);
    Ok(config)
}

/// Writes `config.toml` and `playlist.json` into the config folder, creating it if needed. Does
/// nothing with `--no-config`.
pub fn save(setup: &Setup, config: &Config, playlist: &Playlist) -> io::Result<()> {
    if !setup.enabled {
        return Ok(());
    }
    fs::create_dir_all(&setup.dir)?;
    let text = toml::to_string_pretty(config)
        .map_err(|e| io::Error::new(ErrorKind::InvalidData, e.to_string()))?;
    write_atomic(&setup.dir.join(CONFIG_FILE), text.as_bytes())?;
    write_atomic(&setup.dir.join(PLAYLIST_FILE), playlist.to_json().as_bytes())?;
    Ok(())
}

/// Writes `bytes` to `path` through a temporary file in the same folder, then renames it over
/// `path`. A crash leaves either the old file or the new one.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let mut tmp_name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    tmp_name.push(".tmp");
    let tmp = path.with_file_name(tmp_name);
    {
        let mut file = fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    fs::rename(&tmp, path)
}

/// The file's text, or `None` when it does not exist.
fn read_text(path: &Path) -> io::Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}

/// Moves an unreadable file to `<name>.bad`, so the defaults do not overwrite it.
fn set_aside(path: &Path, warnings: &mut Vec<String>) {
    let mut bad = path.as_os_str().to_os_string();
    bad.push(".bad");
    let bad = PathBuf::from(bad);
    match fs::rename(path, &bad) {
        Ok(()) => warnings.push(format!("the unreadable file was kept as {}", bad.display())),
        Err(e) => warnings.push(format!("could not move {} aside: {e}", path.display())),
    }
}

/// A rectangle in physical pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

/// Where a window of `size` should go so that it is on a monitor. The saved `origin` is kept when
/// enough of the window lies on some monitor. Otherwise the window moves onto the monitor it
/// overlaps most (or the first one), and fits inside it when it can. With no monitors known, the
/// origin is kept.
pub fn clamp_origin(origin: (i32, i32), size: (i32, i32), monitors: &[Rect]) -> (i32, i32) {
    let (x, y) = origin;
    let (w, h) = size;
    if monitors.is_empty() {
        return origin;
    }
    let need_w = MIN_VISIBLE_PX.min(w.max(1));
    let need_h = MIN_VISIBLE_PX.min(h.max(1));
    let overlap = |m: &Rect| -> (i32, i32) {
        let ox = (x + w).min(m.x + m.w) - x.max(m.x);
        let oy = (y + h).min(m.y + m.h) - y.max(m.y);
        (ox.max(0), oy.max(0))
    };
    if monitors.iter().any(|m| {
        let (ox, oy) = overlap(m);
        ox >= need_w && oy >= need_h
    }) {
        return origin;
    }
    // The monitor with the largest overlap. On a tie (none overlaps, say) the first one wins.
    let mut target = monitors[0];
    let mut best = -1i64;
    for m in monitors {
        let (ox, oy) = overlap(m);
        let area = i64::from(ox) * i64::from(oy);
        if area > best {
            best = area;
            target = *m;
        }
    }
    let nx = if w >= target.w {
        target.x
    } else {
        x.clamp(target.x, target.x + target.w - w)
    };
    let ny = if h >= target.h {
        target.y
    } else {
        y.clamp(target.y, target.y + target.h - h)
    };
    (nx, ny)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fresh folder under the system's temp folder, for one test.
    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tubeamp-config-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn setup_in(dir: &Path) -> Setup {
        Setup {
            dir: dir.to_path_buf(),
            enabled: true,
        }
    }

    fn sample_config() -> Config {
        let mut config = Config::default();
        config.windows.insert(
            "main".to_string(),
            WindowPrefs {
                x: 120,
                y: -40,
                visible: true,
                shade: true,
                steps: None,
                size: None,
            },
        );
        config.windows.insert(
            "playlist".to_string(),
            WindowPrefs {
                x: 120,
                y: 300,
                visible: true,
                shade: false,
                steps: Some([2, 4]),
                size: None,
            },
        );
        config.windows.insert(
            "video".to_string(),
            WindowPrefs {
                x: 500,
                y: 80,
                visible: false,
                shade: false,
                steps: None,
                size: Some([320, 240]),
            },
        );
        config.main.volume = 64;
        config.main.balance = -30;
        config.main.shuffle = true;
        config.main.repeat = true;
        config.main.remaining_mode = true;
        config.main.vis_mode = "oscilloscope".to_string();
        config.main.doubled = true;
        config.eq.on = false;
        config.eq.auto = true;
        config.eq.preamp = 40;
        config.eq.bands = [1, 2, 3, 33, 33, 33, 33, 33, 60, 64];
        config.skin = "/Users/someone/Library/TubeAmp/skins/Classic.wsz".to_string();
        config.video_browser = true;
        config.playlist = PlaylistPrefs {
            current: Some(2),
            scroll: 1,
        };
        config.last_played = Some(LastPlayed {
            video_id: "kJQP7kiw5Fk".to_string(),
            secs: 83.25,
        });
        config.classic_skin_answered = true;
        config
    }

    #[test]
    fn a_full_config_round_trips_through_toml() {
        let config = sample_config();
        let text = toml::to_string_pretty(&config).expect("serialize");
        let back = parse_config(&text).expect("parse");
        assert_eq!(back, config, "via:\n{text}");
    }

    #[test]
    fn the_default_config_round_trips_and_omits_empty_fields() {
        let config = Config::default();
        let text = toml::to_string_pretty(&config).expect("serialize");
        assert!(!text.contains("last_played"), "{text}");
        assert_eq!(parse_config(&text).expect("parse"), config);
    }

    #[test]
    fn unknown_windows_and_fields_are_kept_by_name() {
        let text = r#"
            skin = "default"
            [windows.milkdrop]
            x = 10
            y = 20
            visible = true
            shade = false
        "#;
        let config = parse_config(text).expect("parse");
        assert_eq!(
            config.windows.get("milkdrop"),
            Some(&WindowPrefs {
                x: 10,
                y: 20,
                visible: true,
                shade: false,
                steps: None,
                size: None,
            })
        );
        let again = toml::to_string_pretty(&config).expect("serialize");
        assert!(again.contains("[windows.milkdrop]"), "{again}");
    }

    #[test]
    fn out_of_range_values_are_clamped_on_load() {
        let text = "[main]\nvolume = 250\nbalance = -120\n[eq]\npreamp = 0\nbands = [0, 1, 2, 3, 4, 5, 6, 7, 8, 99]\n";
        let config = parse_config(text).expect("parse");
        assert_eq!(config.main.volume, 100);
        assert_eq!(config.main.balance, -100);
        assert_eq!(config.eq.preamp, 1);
        assert_eq!(config.eq.bands[0], 1);
        assert_eq!(config.eq.bands[9], 64);
    }

    #[test]
    fn a_missing_folder_loads_defaults_and_is_a_first_run() {
        let dir = test_dir("missing");
        let loaded = load(&setup_in(&dir));
        assert!(!loaded.existed);
        assert_eq!(loaded.config, Config::default());
        assert!(loaded.playlist.is_empty());
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn save_then_load_restores_the_config_and_the_playlist() {
        let dir = test_dir("roundtrip");
        let setup = setup_in(&dir);
        let mut playlist = Playlist::default();
        playlist.add(tubeamp_playlist::Entry {
            video_id: "kJQP7kiw5Fk".to_string(),
            title: "Despacito".to_string(),
            artist: "Luis Fonsi".to_string(),
            duration_secs: Some(229),
        });
        playlist.add(tubeamp_playlist::Entry::from_id("dQw4w9WgXcQ"));
        let config = sample_config();
        save(&setup, &config, &playlist).expect("save");

        let loaded = load(&setup);
        assert!(loaded.existed);
        assert!(loaded.warnings.is_empty(), "{:?}", loaded.warnings);
        assert_eq!(loaded.config, config);
        assert_eq!(loaded.playlist, playlist);
        // No temporary files are left behind.
        assert!(!dir.join("config.toml.tmp").exists());
        assert!(!dir.join("playlist.json.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_config_becomes_defaults_and_is_kept_aside() {
        let dir = test_dir("corrupt-config");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(CONFIG_FILE), "volume = [[[ not toml").unwrap();
        let loaded = load(&setup_in(&dir));
        assert!(loaded.existed, "a corrupt file still counts as a previous run");
        assert_eq!(loaded.config, Config::default());
        assert_eq!(loaded.warnings.len(), 2, "{:?}", loaded.warnings);
        assert!(dir.join("config.toml.bad").exists());
        assert!(!dir.join(CONFIG_FILE).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_corrupt_playlist_starts_empty_and_keeps_the_config() {
        let dir = test_dir("corrupt-playlist");
        let setup = setup_in(&dir);
        let config = sample_config();
        save(&setup, &config, &Playlist::default()).unwrap();
        fs::write(dir.join(PLAYLIST_FILE), "{ this is not a playlist").unwrap();
        let loaded = load(&setup);
        assert_eq!(loaded.config, config);
        assert!(loaded.playlist.is_empty());
        assert!(dir.join("playlist.json.bad").exists());
        assert_eq!(loaded.warnings.len(), 2, "{:?}", loaded.warnings);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn no_config_reads_and_writes_nothing() {
        let dir = test_dir("disabled");
        let setup = Setup {
            dir: dir.clone(),
            enabled: false,
        };
        save(&setup, &sample_config(), &Playlist::default()).expect("save is a no-op");
        assert!(!dir.exists());
        let loaded = load(&setup);
        assert!(!loaded.existed);
        assert_eq!(loaded.config, Config::default());
    }

    #[test]
    fn write_atomic_replaces_the_old_file() {
        let dir = test_dir("atomic");
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("thing.txt");
        fs::write(&path, "old").unwrap();
        write_atomic(&path, b"new").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "new");
        assert!(!dir.join("thing.txt.tmp").exists());
        let _ = fs::remove_dir_all(&dir);
    }

    fn screen(x: i32, y: i32, w: i32, h: i32) -> Rect {
        Rect { x, y, w, h }
    }

    #[test]
    fn a_position_on_a_monitor_is_kept() {
        let monitors = [screen(0, 0, 1920, 1080)];
        assert_eq!(clamp_origin((100, 200), (275, 116), &monitors), (100, 200));
        // Mostly on the screen is enough.
        assert_eq!(clamp_origin((-200, 0), (275, 116), &monitors), (-200, 0));
    }

    #[test]
    fn a_position_off_every_monitor_moves_onto_the_one_it_overlaps_most() {
        let monitors = [screen(0, 0, 1920, 1080), screen(1920, 0, 2560, 1440)];
        // Off every screen with no overlap at all: the first screen, pulled in from the right.
        let (x, y) = clamp_origin((9000, 200), (275, 116), &monitors);
        assert_eq!(x, 1920 - 275);
        assert_eq!(y, 200);
        // Below the screens: pulled up so the window is inside the first screen.
        let (x, y) = clamp_origin((50, 5000), (275, 116), &monitors);
        assert_eq!(x, 50);
        assert_eq!(y, 1080 - 116);
        // Mostly above the second screen's top edge: the second screen has the larger overlap,
        // so the window comes down onto it.
        let (x, y) = clamp_origin((2000, -100), (275, 116), &monitors);
        assert_eq!((x, y), (2000, 0));
    }

    #[test]
    fn a_window_larger_than_the_monitor_goes_to_its_corner() {
        let monitors = [screen(100, 50, 800, 600)];
        assert_eq!(clamp_origin((-5000, -5000), (1000, 700), &monitors), (100, 50));
    }

    #[test]
    fn no_monitors_means_no_change() {
        assert_eq!(clamp_origin((-9000, 9000), (275, 116), &[]), (-9000, 9000));
    }
}
