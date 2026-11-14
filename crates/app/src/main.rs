//! TubeAmp desktop player: YouTube Music in a classic Winamp 2.x skin.
//!
//! - `tubeamp [--skin PATH.wsz] [--zoom N]` opens the main window and the hidden video host.
//! - `tubeamp --render-main SKIN.wsz OUT.png [--zoom N]` renders the demo state to a PNG and exits.
//!
//! Test hooks: `--screenshot-after SECS OUT.png` saves the live main window (2×) and keeps running,
//! `--quit-after SECS` exits after a while, `TUBEAMP_AUTOLOAD=<videoId>` loads a video six seconds
//! after start, and `TUBEAMP_LOG=1` prints bridge events and state changes to stderr.

mod demo;
mod player;
mod window;

use std::error::Error;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Duration;

use tubeamp_skin::Skin;
use tubeamp_ui::{render_main, Canvas, MainState, HEIGHT, WIDTH};

/// The built-in default skin (TubeAmp Classic), embedded at build time.
const DEFAULT_SKIN: &[u8] = include_bytes!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../assets/default-skin.wsz"
));

const USAGE: &str = "usage:
  tubeamp [--skin PATH.wsz] [--zoom N] [--screenshot-after SECS OUT.png] [--quit-after SECS]
  tubeamp --render-main SKIN.wsz OUT.png [--zoom N]

  --zoom N                 integer scale of the 275x116 window (1..=8, default 1)
  --screenshot-after S F   save the live main window (2x) to F after S seconds
  --quit-after S           exit after S seconds

environment:
  TUBEAMP_AUTOLOAD=ID      load YouTube video ID six seconds after start
  TUBEAMP_LOG=1            print bridge events and state changes to stderr";

/// Time shown by the headless render, in seconds (01:23).
const RENDER_TIME_SECS: u32 = 83;

#[derive(Debug, Default)]
struct Args {
    skin: Option<PathBuf>,
    render: Option<(PathBuf, PathBuf)>,
    zoom: Option<u32>,
    screenshot: Option<(Duration, PathBuf)>,
    quit_after: Option<Duration>,
    help: bool,
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
