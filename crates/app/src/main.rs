//! TubeAmp desktop player: YouTube Music in a classic Winamp 2.x skin.
//!
//! - `tubeamp [--skin PATH.wsz] [--zoom N]` opens the main window and the hidden video host.
//! - `tubeamp --render-main SKIN.wsz OUT.png [--zoom N]` renders the demo state to a PNG and exits.
//! - `tubeamp --render-eq SKIN.wsz OUT.png [--zoom N] [--eq-preset NAME]` renders the equalizer.
//! - `tubeamp --render-pl SKIN.wsz OUT.png [--zoom N] [--size-steps SX,SY]` renders the playlist
//!   editor with a sample list.
//! - `tubeamp --render-library SKIN.wsz OUT.png [--zoom N] [--fixture PATH.json]` renders the media
//!   library from a captured response (search_daft_punk.json by default), without the network.
//!
//! Test hooks: `--screenshot-after SECS OUT.png` saves the live main window (2×) and keeps running,
//! and the equalizer (`OUT-eq.png`) and playlist (`OUT-pl.png`) too while they are open. `--open-eq`
//! opens the equalizer at startup, `--open-pl` opens the playlist editor, `--add ID1,ID2` appends
//! videos to the playlist at startup, `--eq-preset NAME` applies a built-in preset at startup,
//! `--quit-after SECS` exits after a while, `TUBEAMP_AUTOLOAD=<videoId>` loads a video six seconds
//! after start, and `TUBEAMP_LOG=1` prints bridge events and state changes to stderr.

mod api;
mod demo;
mod eq;
mod hotkeys;
mod layout;
mod library;
mod player;
mod playlist;
mod skinned;
mod window;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use tubeamp_eq::{presets, Preset};
use tubeamp_playlist::parse_video_id;
use tubeamp_skin::Skin;
use tubeamp_ui::eq_window as eqw;
use tubeamp_ui::playlist_window::PlaylistWindowState;
use tubeamp_ui::playlist_window::{self as plw, MAX_STEPS};
use tubeamp_ui::video_window::{self as vw, VideoWindowState};
use tubeamp_ui::{
    render_eq, render_library, render_main, render_playlist, render_video_frame, Canvas,
    EqWindowState, LibraryState, MainState, PlMenu, HEIGHT, WIDTH,
};

/// The built-in default skin (TubeAmp Classic), embedded at build time.
const DEFAULT_SKIN: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/default-skin.wsz"
));

const USAGE: &str = "usage:
  tubeamp [--skin PATH.wsz] [--zoom N] [--screenshot-after SECS OUT.png] [--quit-after SECS]
          [--open-eq] [--eq-preset NAME]
  tubeamp --render-main SKIN.wsz OUT.png [--zoom N]
  tubeamp --render-eq SKIN.wsz OUT.png [--zoom N] [--eq-preset NAME]
  tubeamp --render-pl SKIN.wsz OUT.png [--zoom N] [--size-steps SX,SY] [--open-menu add|rem|sel|misc|list]
  tubeamp --render-library SKIN.wsz OUT.png [--zoom N] [--fixture PATH.json]
  tubeamp --render-video SKIN.wsz OUT.png [--zoom N] [--size WxH]
  tubeamp --render-shade main|eq|pl SKIN.wsz OUT.png [--zoom N] [--size-steps SX,SY]

  --zoom N                 integer scale of the 275x116 window (1..=8, default 1)
  --screenshot-after S F   save the live main window (2x) to F after S seconds, and the
                           equalizer (2x) to F-eq.png and the playlist to F-pl.png when open,
                           the video frame to F-video.png and the real video window (macOS,
                           screencapture) to F-video-live.png when it is showing
  --quit-after S           exit after S seconds
  --open-eq                open the equalizer window below the main window at startup
  --eq-preset NAME         apply a built-in EQ preset (e.g. \"Rock\") at startup or in a render
  --open-pl                open the playlist editor at startup
  --add ID1,ID2,...        append YouTube videos (ids or links) to the playlist at startup
  --size-steps SX,SY       playlist size in resize steps (width 275+25*SX, height 116+29*SY)
  --open-menu NAME         with --render-pl: open the add, rem, sel, misc or list popup
  --open-library           open the media library window at startup (Alt+L toggles it)
  --library-search QUERY   run QUERY in the media library once the page is ready
  --fixture PATH.json      with --render-library: a captured response to render instead
  --open-video             open the video window at startup (Alt+V toggles it)
  --video-mode MODE        video (default: only the video shows) or browser (the whole
                           YouTube Music page, and a larger window)
  --size WxH               with --render-video: the frame size in skin pixels (default 275x232)
  --shade main,eq,pl       start with these windows in windowshade mode (any subset)
  --render-shade WHICH     with --render-shade: the windowshade of main, eq or pl, headless
  --layout-dump            print the window rectangles and their connections to stderr when a
                           drag ends (for tests)

environment:
  TUBEAMP_AUTOLOAD=ID      load YouTube video ID six seconds after start
  TUBEAMP_LOG=1            print bridge events and state changes to stderr";

/// The info line a headless `--render-video` shows under the frame (a fake track).
const RENDER_VIDEO_INFO: &str = "Luis Fonsi - Despacito";

/// Time shown by the headless render, in seconds (01:23).
const RENDER_TIME_SECS: u32 = 83;

#[derive(Debug, Default)]
struct Args {
    skin: Option<PathBuf>,
    render: Option<(PathBuf, PathBuf)>,
    render_eq: Option<(PathBuf, PathBuf)>,
    render_pl: Option<(PathBuf, PathBuf)>,
    render_library: Option<(PathBuf, PathBuf)>,
    fixture: Option<PathBuf>,
    open_library: bool,
    library_search: Option<String>,
    size_steps: Option<(u32, u32)>,
    open_menu: Option<PlMenu>,
    zoom: Option<u32>,
    screenshot: Option<(Duration, PathBuf)>,
    quit_after: Option<Duration>,
    open_eq: bool,
    open_pl: bool,
    add: Vec<String>,
    eq_preset: Option<Preset>,
    render_video: Option<(PathBuf, PathBuf)>,
    /// `--size WxH` for `--render-video`, in skin pixels.
    size: Option<(u32, u32)>,
    open_video: bool,
    video_browser: bool,
    /// `--shade`: which windows start in windowshade mode.
    shade: window::ShadeSet,
    /// `--render-shade`: the window, skin and output file.
    render_shade: Option<(ShadeTarget, PathBuf, PathBuf)>,
    layout_dump: bool,
    help: bool,
}

/// The windows that have a windowshade, for `--render-shade`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShadeTarget {
    Main,
    Eq,
    Pl,
}

/// Parses the window name for `--render-shade`.
fn parse_shade_target(value: &str) -> Result<ShadeTarget, String> {
    match value {
        "main" => Ok(ShadeTarget::Main),
        "eq" => Ok(ShadeTarget::Eq),
        "pl" => Ok(ShadeTarget::Pl),
        other => Err(format!("--render-shade takes main, eq or pl, got {other}")),
    }
}

/// Parses `--shade`: a comma-separated subset of `main`, `eq` and `pl`.
fn parse_shade_set(value: &str) -> Result<window::ShadeSet, String> {
    let mut set = window::ShadeSet::default();
    for name in value.split(',').map(str::trim).filter(|n| !n.is_empty()) {
        match name {
            "main" => set.main = true,
            "eq" => set.eq = true,
            "pl" => set.pl = true,
            other => return Err(format!("--shade takes main, eq or pl, got {other}")),
        }
    }
    Ok(set)
}

fn next_path(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<PathBuf, String> {
    args.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{flag} needs a value"))
}

fn next_seconds(args: &mut impl Iterator<Item = String>, flag: &str) -> Result<Duration, String> {
    let value = args
        .next()
        .ok_or_else(|| format!("{flag} needs a number of seconds"))?;
    let secs: f64 = value
        .parse()
        .map_err(|_| format!("invalid seconds for {flag}: {value}"))?;
    if !secs.is_finite() || secs < 0.0 {
        return Err(format!("{flag} needs a non-negative number, got {value}"));
    }
    Ok(Duration::from_secs_f64(secs))
}

/// Parses a popup name for `--open-menu`.
fn parse_menu(value: &str) -> Result<PlMenu, String> {
    match value {
        "add" => Ok(PlMenu::Add),
        "rem" => Ok(PlMenu::Remove),
        "sel" => Ok(PlMenu::Select),
        "misc" => Ok(PlMenu::Misc),
        "list" => Ok(PlMenu::List),
        other => Err(format!(
            "--open-menu takes add, rem, sel, misc or list, got {other}"
        )),
    }
}

/// Parses `SX,SY` for `--size-steps`. Each step count is at most [`MAX_STEPS`].
fn parse_steps(value: &str) -> Result<(u32, u32), String> {
    let (sx, sy) = value
        .split_once(',')
        .ok_or_else(|| format!("--size-steps needs SX,SY, got {value}"))?;
    let parse = |v: &str| -> Result<u32, String> {
        let n: u32 = v
            .trim()
            .parse()
            .map_err(|_| format!("invalid --size-steps value: {value}"))?;
        if n > MAX_STEPS {
            return Err(format!(
                "--size-steps values must be at most {MAX_STEPS}, got {n}"
            ));
        }
        Ok(n)
    };
    Ok((parse(sx)?, parse(sy)?))
}

/// Parses `--video-mode`: `video` or `browser`. Returns true for the browser view.
fn parse_video_mode(value: &str) -> Result<bool, String> {
    match value {
        "video" => Ok(false),
        "browser" => Ok(true),
        other => Err(format!("--video-mode takes video or browser, got {other}")),
    }
}

/// Parses `WxH` for `--size`: the frame size in skin pixels, at least the frame's minimum.
fn parse_frame_size(value: &str) -> Result<(u32, u32), String> {
    let (w, h) = value
        .split_once(['x', 'X'])
        .ok_or_else(|| format!("--size needs WxH, got {value}"))?;
    let parse = |v: &str| -> Result<u32, String> {
        v.trim()
            .parse()
            .map_err(|_| format!("invalid --size value: {value}"))
    };
    let (w, h) = (parse(w)?, parse(h)?);
    if w < vw::MIN_W || h < vw::MIN_H {
        return Err(format!(
            "--size must be at least {}x{}, got {value}",
            vw::MIN_W,
            vw::MIN_H
        ));
    }
    Ok((w, h))
}

/// Parses `--add`: comma-separated video ids or YouTube links.
fn parse_add(value: &str) -> Result<Vec<String>, String> {
    value
        .split(',')
        .filter(|token| !token.trim().is_empty())
        .map(|token| {
            parse_video_id(token).ok_or_else(|| format!("not a YouTube video id or link: {token}"))
        })
        .collect()
}

fn parse_args(mut args: impl Iterator<Item = String>) -> Result<Args, String> {
    let mut out = Args::default();
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--skin" => out.skin = Some(next_path(&mut args, "--skin")?),
            "--render-main" => {
                let skin = next_path(&mut args, "--render-main")?;
                let png = next_path(&mut args, "--render-main")?;
                out.render = Some((skin, png));
            }
            "--render-eq" => {
                let skin = next_path(&mut args, "--render-eq")?;
                let png = next_path(&mut args, "--render-eq")?;
                out.render_eq = Some((skin, png));
            }
            "--render-pl" => {
                let skin = next_path(&mut args, "--render-pl")?;
                let png = next_path(&mut args, "--render-pl")?;
                out.render_pl = Some((skin, png));
            }
            "--render-library" => {
                let skin = next_path(&mut args, "--render-library")?;
                let png = next_path(&mut args, "--render-library")?;
                out.render_library = Some((skin, png));
            }
            "--fixture" => out.fixture = Some(next_path(&mut args, "--fixture")?),
            "--open-library" => out.open_library = true,
            "--library-search" => {
                let query = args.next().ok_or("--library-search needs a query")?;
                out.library_search = Some(query);
            }
            "--size-steps" => {
                let value = args.next().ok_or("--size-steps needs SX,SY")?;
                out.size_steps = Some(parse_steps(&value)?);
            }
            "--open-menu" => {
                let value = args
                    .next()
                    .ok_or("--open-menu needs add, rem, sel, misc or list")?;
                out.open_menu = Some(parse_menu(&value)?);
            }
            "--open-pl" => out.open_pl = true,
            "--add" => {
                let value = args.next().ok_or("--add needs a list of video ids")?;
                out.add.extend(parse_add(&value)?);
            }
            "--open-eq" => out.open_eq = true,
            "--render-video" => {
                let skin = next_path(&mut args, "--render-video")?;
                let png = next_path(&mut args, "--render-video")?;
                out.render_video = Some((skin, png));
            }
            "--size" => {
                let value = args.next().ok_or("--size needs WxH")?;
                out.size = Some(parse_frame_size(&value)?);
            }
            "--open-video" => out.open_video = true,
            "--shade" => {
                let value = args.next().ok_or("--shade needs main, eq or pl")?;
                out.shade = parse_shade_set(&value)?;
            }
            "--render-shade" => {
                let which = args.next().ok_or("--render-shade needs main, eq or pl")?;
                let target = parse_shade_target(&which)?;
                let skin = next_path(&mut args, "--render-shade")?;
                let png = next_path(&mut args, "--render-shade")?;
                out.render_shade = Some((target, skin, png));
            }
            "--layout-dump" => out.layout_dump = true,
            "--video-mode" => {
                let value = args.next().ok_or("--video-mode needs video or browser")?;
                out.video_browser = parse_video_mode(&value)?;
            }
            "--eq-preset" => {
                let name = args.next().ok_or("--eq-preset needs a preset name")?;
                let preset = presets::find(&name).ok_or_else(|| {
                    let known: Vec<&str> = tubeamp_eq::BUILTIN.iter().map(|p| p.name).collect();
                    format!(
                        "unknown EQ preset \"{name}\"; known presets: {}",
                        known.join(", ")
                    )
                })?;
                out.eq_preset = Some(*preset);
            }
            "--zoom" => {
                let value = args.next().ok_or("--zoom needs a value")?;
                let zoom: u32 = value
                    .parse()
                    .map_err(|_| format!("invalid --zoom value: {value}"))?;
                if !(1..=8).contains(&zoom) {
                    return Err(format!("--zoom must be between 1 and 8, got {zoom}"));
                }
                out.zoom = Some(zoom);
            }
            "--screenshot-after" => {
                let after = next_seconds(&mut args, "--screenshot-after")?;
                let png = next_path(&mut args, "--screenshot-after")?;
                out.screenshot = Some((after, png));
            }
            "--quit-after" => out.quit_after = Some(next_seconds(&mut args, "--quit-after")?),
            "-h" | "--help" => out.help = true,
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(out)
}

/// Renders the main window at integer `factor` into `0x00RRGGBB` pixels.
fn render_scaled(skin: &Skin, state: &MainState, factor: u32) -> Vec<u32> {
    let mut canvas = Canvas::new(WIDTH, HEIGHT);
    render_main(skin, state, &mut canvas);
    let mut out = vec![0u32; ((WIDTH * factor) * (HEIGHT * factor)) as usize];
    canvas.scale_into(&mut out, factor);
    out
}

/// The windowshade of `target` at its demo state, for `--render-shade`. The playlist uses the
/// demo tracks and `size_steps` (default: the classic size).
fn render_shade_canvas(
    skin: &Skin,
    target: ShadeTarget,
    size_steps: Option<(u32, u32)>,
    preset: Option<&Preset>,
) -> Canvas {
    match target {
        ShadeTarget::Main => {
            let state = MainState {
                shade: true,
                ..demo::demo_state(RENDER_TIME_SECS, 0, true)
            };
            let mut canvas = Canvas::new(WIDTH, tubeamp_ui::main_window::SHADE_HEIGHT);
            render_main(skin, &state, &mut canvas);
            canvas
        }
        ShadeTarget::Eq => {
            let state = EqWindowState {
                shade: true,
                // The demo's volume and balance, as the main window's render shows them.
                volume: 75,
                balance: 0,
                ..demo::eq_demo_state(preset)
            };
            let mut canvas = Canvas::new(eqw::WIDTH, eqw::SHADE_HEIGHT);
            render_eq(skin, &state, &mut canvas);
            canvas
        }
        ShadeTarget::Pl => {
            let steps = size_steps.unwrap_or(plw::DEFAULT_STEPS);
            let playlist = demo::pl_demo_playlist();
            let state = PlaylistWindowState {
                shade: true,
                mini_time_secs: None,
                ..demo::pl_demo_state(steps)
            };
            let (w, h) = plw::window_size_for(steps, true);
            let mut canvas = Canvas::new(w, h);
            render_playlist(skin, &playlist, &state, &mut canvas);
            canvas
        }
    }
}

fn save_png(pixels: &[u32], width: u32, height: u32, path: &Path) -> Result<(), Box<dyn Error>> {
    let mut img = image::RgbaImage::new(width, height);
    for (dst, &px) in img.pixels_mut().zip(pixels) {
        dst.0 = [(px >> 16) as u8, (px >> 8) as u8, px as u8, 0xFF];
    }
    img.save(path)?;
    Ok(())
}

fn run() -> Result<(), Box<dyn Error>> {
    let args = match parse_args(std::env::args().skip(1)) {
        Ok(args) => args,
        Err(msg) => return Err(format!("{msg}\n{USAGE}").into()),
    };
    if args.help {
        println!("{USAGE}");
        return Ok(());
    }
    let zoom = args.zoom.unwrap_or(1);

    if let Some((skin_path, png_path)) = args.render {
        let skin = Skin::load_path(&skin_path)?;
        let state = demo::demo_state(RENDER_TIME_SECS, 0, true);
        let pixels = render_scaled(&skin, &state, zoom);
        save_png(&pixels, WIDTH * zoom, HEIGHT * zoom, &png_path)?;
        println!("wrote {}", png_path.display());
        return Ok(());
    }

    if let Some((skin_path, png_path)) = args.render_eq {
        let skin = Skin::load_path(&skin_path)?;
        let state = demo::eq_demo_state(args.eq_preset.as_ref());
        let mut canvas = Canvas::new(eqw::WIDTH, eqw::HEIGHT);
        render_eq(&skin, &state, &mut canvas);
        let pixels = skinned::scaled(&canvas, zoom);
        save_png(&pixels, eqw::WIDTH * zoom, eqw::HEIGHT * zoom, &png_path)?;
        println!("wrote {}", png_path.display());
        return Ok(());
    }

    if let Some((skin_path, png_path)) = args.render_library {
        let skin = Skin::load_path(&skin_path)?;
        let (name, body) = match &args.fixture {
            Some(path) => {
                let body = std::fs::read_to_string(path)?;
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                (name, body)
            }
            None => (
                "search_daft_punk.json".to_string(),
                library::DEFAULT_FIXTURE.to_string(),
            ),
        };
        let state = LibraryState {
            focused: true,
            ..library::fixture_state(&name, &body, library::RENDER_QUERY)?
        };
        let (w, h) = tubeamp_ui::gen_window::window_size(state.size_steps);
        let mut canvas = Canvas::new(w, h);
        render_library(&skin, &state, &mut canvas);
        let pixels = skinned::scaled(&canvas, zoom);
        save_png(&pixels, w * zoom, h * zoom, &png_path)?;
        println!("wrote {}", png_path.display());
        return Ok(());
    }

    if let Some((skin_path, png_path)) = args.render_video {
        let skin = Skin::load_path(&skin_path)?;
        let (w, h) = args.size.unwrap_or((vw::DEFAULT_W, vw::DEFAULT_H));
        let state = VideoWindowState {
            w,
            h,
            focused: true,
            pressed: None,
            info_text: RENDER_VIDEO_INFO.to_string(),
        };
        let mut canvas = Canvas::new(w, h);
        render_video_frame(&skin, &state, &mut canvas);
        let pixels = skinned::scaled(&canvas, zoom);
        save_png(&pixels, w * zoom, h * zoom, &png_path)?;
        println!("wrote {}", png_path.display());
        return Ok(());
    }

    if let Some((target, skin_path, png_path)) = args.render_shade {
        let skin = Skin::load_path(&skin_path)?;
        let canvas = render_shade_canvas(&skin, target, args.size_steps, args.eq_preset.as_ref());
        let pixels = skinned::scaled(&canvas, zoom);
        save_png(&pixels, canvas.w * zoom, canvas.h * zoom, &png_path)?;
        println!("wrote {}", png_path.display());
        return Ok(());
    }

    if let Some((skin_path, png_path)) = args.render_pl {
        let skin = Skin::load_path(&skin_path)?;
        let steps = args.size_steps.unwrap_or(plw::DEFAULT_STEPS);
        let playlist = demo::pl_demo_playlist();
        let state = PlaylistWindowState {
            open_menu: args.open_menu,
            ..demo::pl_demo_state(steps)
        };
        let (w, h) = plw::window_size(steps);
        let mut canvas = Canvas::new(w, h);
        render_playlist(&skin, &playlist, &state, &mut canvas);
        let pixels = skinned::scaled(&canvas, zoom);
        save_png(&pixels, w * zoom, h * zoom, &png_path)?;
        println!("wrote {}", png_path.display());
        return Ok(());
    }

    let skin = match args.skin {
        Some(path) => Skin::load_path(&path)?,
        None => Skin::load(DEFAULT_SKIN)?,
    };
    let options = window::Options {
        zoom,
        screenshot: args.screenshot,
        quit_after: args.quit_after,
        autoload: std::env::var("TUBEAMP_AUTOLOAD")
            .ok()
            .filter(|id| !id.trim().is_empty()),
        log: std::env::var_os("TUBEAMP_LOG").is_some(),
        open_eq: args.open_eq,
        open_pl: args.open_pl,
        add: args.add,
        eq_preset: args.eq_preset,
        open_library: args.open_library,
        library_search: args.library_search,
        open_video: args.open_video,
        video_browser: args.video_browser,
        shade: args.shade,
        layout_dump: args.layout_dump,
    };
    window::run(skin, options)?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tubeamp: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_sizes_parse_and_respect_the_minimum() {
        assert_eq!(parse_frame_size("350x260"), Ok((350, 260)));
        assert_eq!(parse_frame_size("275X232"), Ok((275, 232)));
        assert!(parse_frame_size("200x260").is_err());
        assert!(parse_frame_size("350x100").is_err());
        assert!(parse_frame_size("350").is_err());
        assert!(parse_frame_size("widexhigh").is_err());
    }

    #[test]
    fn video_modes_parse_to_the_browser_flag() {
        assert_eq!(parse_video_mode("video"), Ok(false));
        assert_eq!(parse_video_mode("browser"), Ok(true));
        assert!(parse_video_mode("both").is_err());
    }
}
