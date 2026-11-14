//! The session: what TubeAmp keeps between runs (`config.rs`), and the features that sit on the
//! app's state. Saving and restoring the settings. Media keys and Now Playing (`nowplaying.rs`).
//! The tray icon and its menu (`tray.rs`). The skin browser, skin loading and dropped files
//! (`skins.rs`). The classic skin download and the first-run question (`firstrun.rs`).
//!
//! The methods are `App` methods, so they can reach the windows and the playback. `window.rs`
//! calls them from its event handling and keeps only the hook lines.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use tubeamp_audio::params as eq_params;
use tubeamp_bridge::Cmd;
use tubeamp_eq::EqSettings;
use tubeamp_playlist::Playlist;
use tubeamp_skin::Skin;
use tubeamp_ui::playlist_window as plw;
use tubeamp_ui::video_window as vw;
use tubeamp_ui::{gen_window as gen, Status, VisMode};
use winit::event_loop::ActiveEventLoop;
use winit::window::WindowId;

use super::{App, UserEvent, ALL_WINDOWS};
use crate::config::{
    self, Config, EqPrefs, LastPlayed, Loaded, MainPrefs, PlaylistPrefs, Rect, Setup, WindowPrefs,
};
use crate::eq;
use crate::firstrun;
use crate::layout::Win;
use crate::library;
#[cfg(target_os = "windows")]
use crate::nowplaying::NowPlaying;
use crate::nowplaying::{MediaAction, Sent, State, Track};
use crate::skinned;
use crate::skins::{self, Choice};
use crate::tray::{self, Labels, Tray};

/// How often the settings are compared with what was last written.
const CHECK_EVERY: Duration = Duration::from_millis(500);
/// The most often the settings are written while they keep changing.
const WRITE_EVERY: Duration = Duration::from_secs(2);
/// How long a skin error stays in the marquee.
const ERROR_SHOWN: Duration = Duration::from_secs(6);
/// How long a short note stays in the marquee.
const NOTE_SHOWN: Duration = Duration::from_secs(4);
/// How long the startup warnings stay in the marquee.
const STARTUP_SHOWN: Duration = Duration::from_secs(8);
/// The YouTube player state of a cued video (loaded, not started).
const PLAYER_CUED: i32 = 5;
/// Shown while `--no-config` is on, for the actions that need the config folder.
const NO_CONFIG_NOTE: &str = "The skins folder is off with --no-config";

/// The windows the config keys by name, with their kinds.
const NAMED_WINDOWS: [(&str, Win); 6] = [
    ("main", Win::Main),
    ("eq", Win::Eq),
    ("playlist", Win::Playlist),
    ("library", Win::Library),
    ("video", Win::Video),
    ("milkdrop", Win::Milkdrop),
];

/// What the session keeps besides the app's own state: the saved settings and what was last
/// written, the skin in use, the marquee message, the media controls and the tray.
pub struct Session {
    setup: Setup,
    /// The settings as loaded, updated on the first-run answer. Its unknown entries are kept.
    base: Config,
    /// The playlist read at startup, taken once by `restore_session`.
    pending_playlist: Option<Playlist>,
    /// The settings and playlist as last written (or loaded), to tell when they changed.
    last_written: Option<Config>,
    last_written_playlist: Option<Playlist>,
    /// Whether writes are allowed (config not disabled with `--no-config`).
    writable: bool,
    last_write_at: Option<Instant>,
    next_check: Instant,
    /// The skin to remember: `"default"` or the chosen file's path.
    skin_pref: String,
    /// The track to load paused at its position, once the player exists.
    cue: Option<LastPlayed>,
    /// The video that was cued at startup. It shows paused while the player stays cued on it.
    cued: Option<String>,
    /// The last track, saved on quit.
    last_played: Option<LastPlayed>,
    /// A message for the main marquee, and until when it shows (`None`: until replaced).
    message: Option<(String, Option<Instant>)>,
    /// A classic skin download is running.
    downloading: bool,
    /// Whether Show/Hide from the tray hid the windows.
    hidden: bool,
    /// Whether the first-run question is due (no config yet).
    first_run: bool,
    /// Until when the tray's playback items are ignored. Set when the first-run dialog closes: the
    /// dialog is modal, so events it leaves behind must not start or stop playback.
    tray_quiet_until: Option<Instant>,
    /// The track last sent to the page's media session (see `sync_system`).
    now_playing: Sent,
    /// Windows' SMTC controls (souvlaki). macOS and the others use the page's media session.
    #[cfg(target_os = "windows")]
    media: Option<NowPlaying>,
    tray: Option<Tray>,
}

impl Session {
    /// Takes what was loaded. Warnings are printed here; `startup_notice` shows in the marquee.
    pub(super) fn new(
        setup: Setup,
        loaded: Loaded,
        startup_notice: Option<String>,
        now: Instant,
    ) -> Session {
        for warning in &loaded.warnings {
            eprintln!("tubeamp: {warning}");
        }
        // Settings are always saved; the classic-skin question is asked until it gets an
        // explicit Yes or No (a dismissed dialog leaves `classic_skin_answered` false).
        let writable = setup.enabled;
        let first_run = setup.enabled && !loaded.config.classic_skin_answered;
        let skin_pref = loaded.config.skin.clone();
        let last_played = loaded.config.last_played.clone();
        Session {
            setup,
            base: loaded.config,
            pending_playlist: Some(loaded.playlist),
            last_written: None,
            last_written_playlist: None,
            writable,
            last_write_at: None,
            next_check: now,
            skin_pref,
            cue: last_played.clone(),
            cued: None,
            last_played,
            message: startup_notice.map(|text| (text, Some(now + STARTUP_SHOWN))),
            downloading: false,
            hidden: false,
            first_run,
            tray_quiet_until: None,
            now_playing: Sent::default(),
            #[cfg(target_os = "windows")]
            media: None,
            tray: None,
        }
    }
}

/// How long the tray's playback items ignore clicks after the first-run dialog closes.
const TRAY_QUIET_AFTER_DIALOG: Duration = Duration::from_secs(1);

/// The resize steps as the config stores them.
fn steps_array((sx, sy): (u32, u32)) -> [u32; 2] {
    [sx, sy]
}

/// The config's name for a vis mode.
pub(super) fn vis_name(mode: VisMode) -> &'static str {
    match mode {
        VisMode::Spectrum => "spectrum",
        VisMode::Oscilloscope => "oscilloscope",
        VisMode::Off => "off",
    }
}

/// The vis mode a config name stands for. Unknown names give the spectrum.
pub(super) fn vis_from_name(name: &str) -> VisMode {
    match name {
        "oscilloscope" => VisMode::Oscilloscope,
        "off" => VisMode::Off,
        _ => VisMode::Spectrum,
    }
}

/// The position a restored track starts at. A track that was at its very end starts at the
/// beginning, so that it is not restored finished.
pub(super) fn resume_secs(secs: f64, duration_secs: f64) -> f64 {
    if duration_secs > 0.0 && secs + 1.0 >= duration_secs {
        0.0
    } else {
        secs
    }
}

/// The media state for a main-window status.
fn media_state(status: Status) -> State {
    match status {
        Status::Play => State::Playing,
        Status::Pause => State::Paused,
        Status::Stop => State::Stopped,
    }
}

impl App {
    // Settings.

    /// Puts the saved settings into the app. Runs once the windows exist and before any is shown.
    /// `--shade` and the other flags have already been applied, and they win where both set a
    /// thing (the shade is kept on when either says so).
    pub(super) fn restore_session(&mut self, el: &ActiveEventLoop) {
        let cfg = self.session.base.clone();
        let m = cfg.main.clone();
        self.state.volume = m.volume;
        self.state.balance = m.balance;
        eq_params().set_balance(m.balance);
        self.state.shuffle = m.shuffle;
        self.state.repeat = m.repeat;
        self.state.remaining_mode = m.remaining_mode;
        self.state.vis_mode = vis_from_name(&m.vis_mode);
        self.eq_state.settings = EqSettings {
            on: cfg.eq.on,
            auto: cfg.eq.auto,
            preamp: cfg.eq.preamp,
            bands: cfg.eq.bands,
        };
        if let Some(list) = self.session.pending_playlist.take() {
            self.playlist = list;
        }
        self.playlist.set_current(cfg.playlist.current);
        self.pl_state.scroll = cfg.playlist.scroll;
        self.video_browser = cfg.video_browser;
        self.session.cue = cfg.last_played.clone();
        self.log(format!(
            "settings restored: skin {}, volume {}, playlist {} entries",
            self.session.skin_pref,
            self.state.volume,
            self.playlist.len()
        ));

        for (name, win) in NAMED_WINDOWS {
            let Some(p) = cfg.windows.get(name).copied() else {
                continue;
            };
            match win {
                Win::Main => self.state.shade |= p.shade,
                Win::Eq => self.eq_state.shade |= p.shade,
                Win::Playlist => {
                    self.pl_state.shade |= p.shade;
                    if let Some([sx, sy]) = p.steps {
                        self.pl_state.size_steps = (sx.min(plw::MAX_STEPS), sy.min(plw::MAX_STEPS));
                    }
                }
                Win::Library => {
                    if let Some([sx, sy]) = p.steps {
                        self.library.state.size_steps = (sx.min(gen::MAX_STEPS), sy.min(gen::MAX_STEPS));
                        self.library.state.clamp_scroll();
                    }
                }
                Win::Video => {
                    if let Some([w, h]) = p.size {
                        let (w, h) = vw::clamp_size(w as i32, h as i32);
                        self.video_state.w = w;
                        self.video_state.h = h;
                    }
                }
                Win::Milkdrop => {
                    if let Some([sx, sy]) = p.steps {
                        let steps = (sx.min(gen::MAX_STEPS), sy.min(gen::MAX_STEPS));
                        self.milkdrop.set_steps(steps);
                    }
                }
            }
        }
        self.apply_sizes();
        self.pl_changed();

        // Positions: each saved origin, moved onto a monitor if it is off every one now.
        let monitors: Vec<Rect> = el
            .available_monitors()
            .map(|m| {
                let p = m.position();
                let s = m.size();
                Rect {
                    x: p.x,
                    y: p.y,
                    w: s.width as i32,
                    h: s.height as i32,
                }
            })
            .collect();
        for (name, win) in NAMED_WINDOWS {
            let Some(p) = cfg.windows.get(name).copied() else {
                continue;
            };
            if self.window(win).is_none() {
                continue;
            }
            let (w, h) = self.window_px_size(win);
            let (x, y) = config::clamp_origin(
                (p.x, p.y),
                (w.round() as i32, h.round() as i32),
                &monitors,
            );
            self.set_window_origin(win, (f64::from(x), f64::from(y)));
        }
        // Windows that were open come back open, where they were (no re-placing).
        for (name, win) in NAMED_WINDOWS {
            let visible = cfg.windows.get(name).is_some_and(|p| p.visible);
            if visible {
                self.reveal(win);
            }
        }
        self.update_video_bounds();
        self.request_redraw();
        if self.session.first_run {
            let _ = self.proxy.send_event(UserEvent::FirstRun);
        }
    }

    /// Shows a window that was open at the last quit, without moving it.
    fn reveal(&mut self, win: Win) {
        match win {
            Win::Main => return,
            Win::Milkdrop => {
                // Its own show places it only the first time, so the restored origin is passed.
                let at = self
                    .window_origin(Win::Milkdrop)
                    .map(|(x, y)| (x.round() as i32, y.round() as i32));
                self.milkdrop.show(at);
                return;
            }
            Win::Eq => {
                self.eq_visible = true;
                self.state.eq_open = true;
            }
            Win::Playlist => {
                self.pl_visible = true;
                self.state.pl_open = true;
            }
            Win::Library => {
                self.lib_visible = true;
                self.lib_placed = true;
            }
            Win::Video => {
                self.video_visible = true;
                self.video_placed = true;
            }
        }
        if let Some(sw) = self.window(win) {
            sw.window.set_visible(true);
        }
    }

    /// Everything the config keeps, read from the app. Unknown window entries are kept as they were.
    pub(super) fn capture_config(&mut self) -> Config {
        let mut cfg = self.session.base.clone();
        for (name, win) in NAMED_WINDOWS {
            let Some((x, y)) = self.window_origin(win) else {
                continue;
            };
            let mut p = WindowPrefs {
                x: x.round() as i32,
                y: y.round() as i32,
                visible: self.window_visible(win),
                ..WindowPrefs::default()
            };
            match win {
                Win::Main => p.shade = self.state.shade,
                Win::Eq => p.shade = self.eq_state.shade,
                Win::Playlist => {
                    p.shade = self.pl_state.shade;
                    p.steps = Some(steps_array(self.pl_state.size_steps));
                }
                Win::Library => p.steps = Some(steps_array(self.library.state.size_steps)),
                Win::Video => p.size = Some([self.video_state.w, self.video_state.h]),
                Win::Milkdrop => p.steps = Some(steps_array(self.milkdrop.steps())),
            }
            cfg.windows.insert(name.to_string(), p);
        }
        cfg.main = MainPrefs {
            volume: self.state.volume,
            balance: self.state.balance,
            shuffle: self.state.shuffle,
            repeat: self.state.repeat,
            remaining_mode: self.state.remaining_mode,
            vis_mode: vis_name(self.state.vis_mode).to_string(),
            doubled: self.state.doubled,
        };
        let e = self.eq_state.settings;
        cfg.eq = EqPrefs {
            on: e.on,
            auto: e.auto,
            preamp: e.preamp,
            bands: e.bands,
        };
        cfg.skin = self.session.skin_pref.clone();
        cfg.video_browser = self.video_browser;
        cfg.playlist = PlaylistPrefs {
            current: self.playlist.current(),
            scroll: self.pl_state.scroll,
        };
        cfg.last_played = self.last_played_now();
        cfg.classic_skin_answered = self.session.base.classic_skin_answered;
        cfg
    }

    /// The track to remember. Until the cued track is loaded, the saved one stays.
    fn last_played_now(&mut self) -> Option<LastPlayed> {
        if self.session.cue.is_none() {
            if let Some(video_id) = self.playback.video_id.clone() {
                let secs = resume_secs(
                    f64::from(self.playback.time_at(Instant::now())),
                    f64::from(self.playback.duration_secs),
                );
                self.session.last_played = Some(LastPlayed { video_id, secs });
            }
        }
        self.session.last_played.clone()
    }

    /// Writes the settings when they changed, at most once per [`WRITE_EVERY`]. Called from the
    /// tick, so a change lands within a few seconds.
    pub(super) fn save_config_if_due(&mut self, now: Instant) {
        if now < self.session.next_check || !self.session.writable {
            return;
        }
        self.session.next_check = now + CHECK_EVERY;
        let cfg = self.capture_config();
        let changed = self.session.last_written.as_ref() != Some(&cfg)
            || self.session.last_written_playlist.as_ref() != Some(&self.playlist);
        if !changed {
            return;
        }
        if self
            .session
            .last_write_at
            .is_some_and(|at| now.duration_since(at) < WRITE_EVERY)
        {
            return;
        }
        self.write_settings(cfg, now);
    }

    /// Writes the settings now, whether or not they changed. Called once the event loop has ended.
    pub(super) fn save_config_now(&mut self) {
        if !self.session.writable {
            return;
        }
        let cfg = self.capture_config();
        self.write_settings(cfg, Instant::now());
    }

    fn write_settings(&mut self, cfg: Config, now: Instant) {
        match config::save(&self.session.setup, &cfg, &self.playlist) {
            Ok(()) => {
                self.session.last_written = Some(cfg);
                self.session.last_written_playlist = Some(self.playlist.clone());
                self.log("settings saved".to_string());
            }
            Err(e) => eprintln!(
                "tubeamp: could not save the settings to {}: {e}",
                self.session.setup.dir.display()
            ),
        }
        self.session.last_write_at = Some(now);
    }

    /// The skins folder, or `None` with `--no-config`, where nothing may be written.
    fn skins_folder(&self) -> Option<PathBuf> {
        self.session
            .setup
            .enabled
            .then(|| skins::skins_dir(&self.session.setup.dir))
    }

    /// Called when the player first reports a state: sends the volume, and the cued track.
    pub(super) fn on_player_ready(&mut self) {
        let level = self.state.volume;
        self.send(Cmd::Volume { level });
        if let Some(cue) = self.session.cue.take() {
            self.log(format!("restoring {} at {:.0} s, paused", cue.video_id, cue.secs));
            self.send(Cmd::Cue {
                video_id: cue.video_id.clone(),
                secs: cue.secs,
            });
            self.session.cued = Some(cue.video_id);
        }
    }

    /// The status the main window shows. A restored track shows paused, not stopped.
    pub(super) fn display_status(&self) -> Status {
        if let Some(id) = &self.session.cued {
            if self.playback.video_id.as_deref() == Some(id.as_str())
                && self.playback.ps == Some(PLAYER_CUED)
            {
                return Status::Pause;
            }
        }
        self.playback.status
    }

    // Media keys, Now Playing and the tray.

    /// Registers the media controls and the tray icon. Called once the main window exists. On macOS
    /// there is nothing to register: the page's media session is the Now Playing entry, and its
    /// actions come back as `Evt::Media`.
    pub(super) fn start_system_integration(&mut self) {
        #[cfg(target_os = "windows")]
        if let Some(main) = self.main.as_ref() {
            let proxy = self.proxy.clone();
            match NowPlaying::new(&main.window, move |action| {
                let _ = proxy.send_event(UserEvent::Media(action));
            }) {
                Ok(media) => self.session.media = Some(media),
                Err(e) => eprintln!("tubeamp: media keys are off: {e}"),
            }
        }
        match Tray::new() {
            Ok(tray) => self.session.tray = Some(tray),
            Err(e) => eprintln!("tubeamp: no tray icon: {e}"),
        }
    }

    /// Runs a media key or Now Playing request, as the buttons would.
    pub(super) fn on_media_action(&mut self, action: MediaAction) {
        match action {
            MediaAction::Play => self.send(Cmd::Play),
            MediaAction::Pause => self.send(Cmd::Pause),
            #[cfg(target_os = "windows")]
            MediaAction::Toggle => self.send(Cmd::Toggle),
            MediaAction::Stop => self.send(Cmd::Stop),
            MediaAction::Next => self.next_track(),
            MediaAction::Previous => self.prev_track(),
            MediaAction::Seek(secs) => self.send(Cmd::Seek {
                secs: secs.max(0.0),
            }),
            MediaAction::SeekBy(delta) => self.seek_relative(delta),
        }
        self.request_redraw();
    }

    /// Makes the next sync send the track again: a page has just loaded, and its media session
    /// is new, so it has none of the track yet.
    pub(super) fn forget_now_playing(&mut self) {
        self.session.now_playing = Sent::default();
    }

    /// Pushes the track and the playback state to Now Playing and the tray, when they changed.
    /// The track goes to the page's media session (`Cmd::NowPlaying`), and on Windows to SMTC.
    pub(super) fn sync_system(&mut self, now: Instant) {
        let status = self.display_status();
        let (track, state, secs) = if self.playback.video_id.is_some() {
            (
                Track {
                    title: self.playback.title.clone(),
                    artist: self.playback.author.clone(),
                    duration_secs: self.playback.duration_secs,
                    cover_url: self.playback.thumbnail.clone(),
                },
                media_state(status),
                self.playback.time_at(now),
            )
        } else {
            (Track::default(), State::Stopped, 0)
        };
        if let Some(new_track) = self.session.now_playing.update(&track, state, secs).track {
            self.send(Cmd::NowPlaying {
                title: new_track.title,
                artist: new_track.artist,
                album: String::new(),
                artwork: new_track.cover_url,
            });
        }
        #[cfg(target_os = "windows")]
        if let Some(media) = self.session.media.as_mut() {
            media.update(&track, state, secs);
        }
        let labels = Labels {
            now: tray::now_line(&track.title, &track.artist),
            playing: state == State::Playing,
        };
        if let Some(tray) = self.session.tray.as_mut() {
            tray.sync(labels);
        }
    }

    /// A menu item of the skin browser, the O menu's additions or the tray. Returns whether the id
    /// was one of ours.
    pub(super) fn on_session_menu(&mut self, el: &ActiveEventLoop, id: &str) -> bool {
        let playback_item = matches!(
            id,
            tray::ID_PLAY_PAUSE | tray::ID_NEXT | tray::ID_PREVIOUS | tray::ID_STOP
        );
        if playback_item && self.tray_is_quiet(Instant::now()) {
            self.log(format!("tray: ignored {id} right after the first-run dialog"));
            return true;
        }
        if let Some(choice) = Choice::from_menu_id(id) {
            self.apply_choice(choice);
        } else {
            match id {
                skins::GET_CLASSIC => self.get_classic_skin(),
                skins::OPEN_FOLDER => self.open_skins_folder(),
                skins::LOAD_SKIN => {
                    if let Some(path) = skins::pick_skin_file() {
                        self.import_skin(&path);
                    }
                }
                tray::ID_PLAY_PAUSE => {
                    let cmd = if self.display_status() == Status::Play {
                        Cmd::Pause
                    } else {
                        Cmd::Play
                    };
                    self.send(cmd);
                }
                tray::ID_NEXT => self.next_track(),
                tray::ID_PREVIOUS => self.prev_track(),
                tray::ID_STOP => self.send(Cmd::Stop),
                tray::ID_SHOW_HIDE => self.toggle_all_windows(),
                tray::ID_LIBRARY => self.toggle_library(),
                tray::ID_QUIT => el.exit(),
                _ => return false,
            }
        }
        self.request_redraw();
        true
    }

    /// Whether the tray's playback items are ignored at `now` (see `tray_quiet_until`).
    fn tray_is_quiet(&self, now: Instant) -> bool {
        self.session.tray_quiet_until.is_some_and(|until| now < until)
    }

    /// Hides every window, or shows them again: the ones that were open come back open. The
    /// player keeps going while the windows are hidden.
    pub(super) fn toggle_all_windows(&mut self) {
        let show = self.session.hidden;
        self.session.hidden = !show;
        if let Some(main) = self.main.as_ref() {
            main.window.set_visible(show);
        }
        for win in [Win::Eq, Win::Playlist, Win::Library, Win::Video, Win::Milkdrop] {
            let on = show && self.window_visible(win);
            if let Some(sw) = self.window(win) {
                sw.window.set_visible(on);
            }
        }
        self.request_redraw();
    }

    // Skins.

    /// The main window's O menu: Media Library, Video, then the Skins submenu.
    pub(super) fn main_menu(&self) -> Result<muda::Menu, muda::Error> {
        let menu = library::build_main_menu()?;
        let list = self
            .skins_folder()
            .map(|dir| skins::list(&dir))
            .unwrap_or_default();
        let current = Choice::from_pref(&self.session.skin_pref);
        menu.append(&skins::build_submenu(&current, &list)?)?;
        Ok(menu)
    }

    /// The main window's menu at the pointer (right click).
    pub(super) fn open_main_context_menu(&mut self) {
        let Some(main) = self.main.as_ref() else {
            return;
        };
        let Some((x, y)) = main.skin_point() else {
            return;
        };
        let zoom = f64::from(main.zoom());
        match self.main_menu() {
            Ok(menu) => {
                if let Err(e) = skinned::show_context_menu(&main.window, &menu, x * zoom, y * zoom)
                {
                    eprintln!("tubeamp: could not show the menu: {e}");
                }
            }
            Err(e) => eprintln!("tubeamp: could not build the menu: {e}"),
        }
        self.request_redraw();
    }

    /// Makes `choice` the skin: loads it, and redraws every window. A skin that does not load
    /// keeps the current one, and the error shows in the marquee. Returns whether it applied.
    pub(super) fn apply_choice(&mut self, choice: Choice) -> bool {
        let loaded = match &choice {
            Choice::Default => Skin::load(crate::DEFAULT_SKIN).map_err(|e| e.to_string()),
            Choice::File(path) => Skin::load_path(path).map_err(|e| e.to_string()),
        };
        match loaded {
            Ok(skin) => {
                // The windows draw from the skin on every frame, so the redraw is all they need.
                self.skin = skin;
                self.session.skin_pref = choice.to_pref();
                self.log(format!("skin: {}", choice.to_pref()));
                self.request_redraw();
                true
            }
            Err(e) => {
                self.show_message(format!("Could not load the skin: {e}"), Some(ERROR_SHOWN));
                false
            }
        }
    }

    /// Copies a `.wsz` into the skins folder and applies it. With `--no-config` it is applied from
    /// where it is.
    pub(super) fn import_skin(&mut self, src: &Path) {
        let dest = match self.skins_folder() {
            Some(dir) => match skins::import(&dir, src) {
                Ok(dest) => dest,
                Err(e) => {
                    self.show_message(format!("Could not copy the skin: {e}"), Some(ERROR_SHOWN));
                    return;
                }
            },
            None => src.to_path_buf(),
        };
        self.apply_choice(Choice::File(dest));
    }

    fn open_skins_folder(&mut self) {
        match self.skins_folder() {
            Some(dir) => {
                if let Err(e) = skins::open_folder(&dir) {
                    self.show_message(e, Some(ERROR_SHOWN));
                }
            }
            None => self.show_message(NO_CONFIG_NOTE.to_string(), Some(NOTE_SHOWN)),
        }
    }

    /// Starts the classic skin download on a background thread, unless one is running. The answer
    /// comes back as `UserEvent::Download`.
    pub(super) fn get_classic_skin(&mut self) {
        let Some(dir) = self.skins_folder() else {
            self.show_message(NO_CONFIG_NOTE.to_string(), Some(NOTE_SHOWN));
            return;
        };
        if self.session.downloading {
            return;
        }
        self.session.downloading = true;
        self.show_message("Getting the classic Winamp skin…".to_string(), None);
        let proxy = self.proxy.clone();
        std::thread::spawn(move || {
            let result = firstrun::obtain(&dir);
            let _ = proxy.send_event(UserEvent::Download(result));
        });
    }

    /// The download finished: applies the skin, or says why not.
    pub(super) fn on_download_done(&mut self, result: Result<PathBuf, String>) {
        self.session.downloading = false;
        match result {
            Ok(path) => {
                if self.apply_choice(Choice::File(path)) {
                    self.show_message("Classic Winamp skin applied".to_string(), Some(NOTE_SHOWN));
                }
            }
            Err(e) => {
                eprintln!("tubeamp: classic skin: {e}");
                self.show_message(
                    format!("{e}. Keeping the current skin."),
                    Some(ERROR_SHOWN),
                );
            }
        }
    }

    /// The first-run question, asked once. Yes starts the download.
    pub(super) fn first_run_prompt(&mut self) {
        let answer = firstrun::ask();
        self.session.tray_quiet_until = Some(Instant::now() + TRAY_QUIET_AFTER_DIALOG);
        match answer {
            firstrun::Answer::Yes => {
                self.remember_first_run_answer();
                self.get_classic_skin();
            }
            firstrun::Answer::No => self.remember_first_run_answer(),
            // Not an answer: `classic_skin_answered` stays false, so the question comes back.
            firstrun::Answer::Unanswered => {
                self.log("first run: no answer; asked again next launch".to_string());
            }
        }
    }

    /// Yes or No was chosen: the question is not asked again.
    fn remember_first_run_answer(&mut self) {
        self.session.base.classic_skin_answered = true;
        self.session.next_check = Instant::now();
        self.log("first run: classic skin question answered".to_string());
    }

    /// Shows `text` in the marquee until `for_` has passed (`None`: until another message).
    pub(super) fn show_message(&mut self, text: String, for_: Option<Duration>) {
        self.session.message = Some((text, for_.map(|d| Instant::now() + d)));
    }

    /// Removes the marquee message if it is `text` (another message is left alone).
    pub(super) fn clear_message_if(&mut self, text: &str) {
        if matches!(&self.session.message, Some((shown, _)) if shown == text) {
            self.session.message = None;
        }
    }

    /// The marquee message still to show, if any. An expired one is dropped.
    pub(super) fn active_message(&mut self, now: Instant) -> Option<String> {
        match &self.session.message {
            Some((text, until)) if until.is_none_or(|t| now < t) => Some(text.clone()),
            Some(_) => {
                self.session.message = None;
                None
            }
            None => None,
        }
    }

    // Dropped files.

    /// A file was dropped on one of our windows. A `.wsz` becomes the skin (on any window). A
    /// `.eqf` applies on the equalizer and a `.json` playlist loads on the playlist window.
    pub(super) fn on_dropped_file(&mut self, id: WindowId, path: PathBuf) {
        let Some(win) = ALL_WINDOWS
            .into_iter()
            .find(|&w| self.window(w).is_some_and(|sw| sw.id() == id))
        else {
            return;
        };
        if skins::is_wsz(&path) {
            self.import_skin(&path);
        } else {
            let ext = path
                .extension()
                .map(|e| e.to_string_lossy().to_ascii_lowercase())
                .unwrap_or_default();
            match (win, ext.as_str()) {
                (Win::Eq, "eqf") => self.load_eqf_file(&path),
                (Win::Playlist, "json") => self.load_playlist_file(&path),
                _ => self.log(format!("ignored {}", path.display())),
            }
        }
        self.request_redraw();
    }

    fn load_eqf_file(&mut self, path: &Path) {
        let loaded = std::fs::read(path)
            .map_err(|e| e.to_string())
            .and_then(|bytes| eq::first_entry(&bytes));
        match loaded {
            Ok(entry) => {
                self.apply_curve(entry.preamp, entry.bands);
                self.log(format!("loaded EQ preset \"{}\"", entry.name));
            }
            Err(e) => self.show_message(
                format!("Could not load the EQ preset: {e}"),
                Some(ERROR_SHOWN),
            ),
        }
    }

    fn load_playlist_file(&mut self, path: &Path) {
        let loaded = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|text| Playlist::from_json(&text).map_err(|e| e.to_string()));
        match loaded {
            Ok(list) => {
                self.playlist = list;
                self.pl_state.scroll = 0;
                self.pl_changed();
            }
            Err(e) => self.show_message(
                format!("Could not load the playlist: {e}"),
                Some(ERROR_SHOWN),
            ),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_vis_mode_round_trips_through_its_name() {
        for mode in [VisMode::Spectrum, VisMode::Oscilloscope, VisMode::Off] {
            assert_eq!(vis_from_name(vis_name(mode)), mode);
        }
        assert_eq!(vis_from_name("nonsense"), VisMode::Spectrum);
    }

    #[test]
    fn a_track_saved_at_its_very_end_restarts_from_the_beginning() {
        assert_eq!(resume_secs(83.0, 229.0), 83.0);
        assert_eq!(resume_secs(228.6, 229.0), 0.0);
        // Without a known duration the position is kept.
        assert_eq!(resume_secs(12.0, 0.0), 12.0);
    }

    #[test]
    fn the_media_state_follows_the_status() {
        assert_eq!(media_state(Status::Play), State::Playing);
        assert_eq!(media_state(Status::Pause), State::Paused);
        assert_eq!(media_state(Status::Stop), State::Stopped);
    }

    #[test]
    fn the_steps_are_stored_as_a_pair() {
        assert_eq!(steps_array((3, 7)), [3, 7]);
    }
}
