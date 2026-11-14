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
mod paint;
mod sheets;
mod text_files;

use image::{DynamicImage, ImageFormat};
use std::error::Error;
use std::fs;
use std::io::{Cursor, Write};
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn Error>> {
    let out_dir = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: tubeamp-skingen <output-dir>")?;

    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for (name, img) in sheets::render_all() {
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

    let dir = out_dir.to_string_lossy();
    let wsz = PathBuf::from(format!("{}.wsz", dir.trim_end_matches('/')));
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
