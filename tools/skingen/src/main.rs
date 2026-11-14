//! `tubeamp-skingen`: generates **TubeAmp Classic**, an original classic-style skin.
//!
//! Every sheet is drawn procedurally from code (no pixels are copied from any
//! other skin). Sprite geometry matches the sheet layout the skin loader expects.
//!
//! Usage: `cargo run -p tubeamp-skingen -- assets/default-skin`
//!
//! Writes the sheets, `VISCOLOR.TXT`, `PLEDIT.TXT` and `README.txt` into the given
//! directory, plus a `.wsz` (zip) of the same files next to it. Output is
//! byte-identical across runs.

mod font;
mod icon;
mod paint;
mod sheets;
mod text_files;
mod theme;

use image::{DynamicImage, ImageFormat};
use std::error::Error;
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;

const USAGE: &str = "usage: tubeamp-skingen [--theme classic|crimson-edge] <output-dir>
       tubeamp-skingen icon <output-dir>   (writes TubeAmp.iconset/ and TubeAmp.icns)";

fn main() -> Result<(), Box<dyn Error>> {
    let mut args = std::env::args_os().skip(1);
    let mut first = args.next().ok_or(USAGE)?;
    let mut style = theme::Style::Classic;
    if first == "--theme" {
        let name = args.next().ok_or(USAGE)?;
        style = match name.to_str() {
            Some("classic") => theme::Style::Classic,
            Some("crimson-edge") => theme::Style::CrimsonEdge,
            _ => return Err(format!("unknown theme {name:?}: use classic or crimson-edge").into()),
        };
        first = args.next().ok_or(USAGE)?;
    }
    theme::set_style(style);
    if first == "icon" {
        let out_dir = args.next().map(PathBuf::from).ok_or(USAGE)?;
        for path in icon::write(&out_dir)? {
            println!("wrote {}", path.display());
        }
        return Ok(());
    }
    let out_dir = PathBuf::from(first);

    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for (name, mut img) in sheets::render_all() {
        if theme::crimson() {
            theme::recolour(&mut img);
        }
        let mut bytes = Vec::new();
        DynamicImage::ImageRgb8(img).write_to(&mut Cursor::new(&mut bytes), ImageFormat::Bmp)?;
        files.push((format!("{name}.BMP"), bytes));
    }
    files.push(("VISCOLOR.TXT".into(), text_files::viscolor().into_bytes()));
    files.push(("PLEDIT.TXT".into(), text_files::pledit().into_bytes()));
    files.push(("README.txt".into(), text_files::readme().into_bytes()));

    fs::create_dir_all(&out_dir)?;
    for (name, data) in &files {
        fs::write(out_dir.join(name), data)?;
    }

    let wsz = match style {
        theme::Style::Classic => {
            let dir = out_dir.to_string_lossy();
            PathBuf::from(format!("{}.wsz", dir.trim_end_matches('/')))
        }
        theme::Style::CrimsonEdge => out_dir
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("Crimson Edge.wsz"),
    };
    fs::write(&wsz, zip_files(&files)?)?;

    println!(
        "wrote {} files to {} and {}",
        files.len(),
        out_dir.display(),
        wsz.display()
    );
    Ok(())
}

/// Zips the files in the given order with fixed timestamps and permissions, so
/// the archive is reproducible.
fn zip_files(files: &[(String, Vec<u8>)]) -> Result<Vec<u8>, Box<dyn Error>> {
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated)
        .last_modified_time(zip::DateTime::default())
        .unix_permissions(0o644);
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in files {
        writer.start_file(name.as_str(), options)?;
        writer.write_all(data)?;
    }
    Ok(writer.finish()?.into_inner())
}
