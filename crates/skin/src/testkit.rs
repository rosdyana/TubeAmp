//! Synthetic skins for tests and for the app's no-skin fallback.
//!
//! Every sheet is filled with a pattern that encodes its own pixel coordinate
//! (see [`encode`] / [`decode_coord`]), so tests can assert that a sprite or a
//! blit landed on exactly the expected source pixels. Not a stable API.

use crate::bitmap::Rgba;
use crate::parse::DEFAULT_VISCOLORS;
use crate::sheet::Sheet;
use crate::skin::Skin;
use std::io::{Cursor, Write};

/// Pixel size of the synthetic bitmap for each sheet (large enough for every Webamp sprite).
pub fn sheet_size(sheet: Sheet) -> (u32, u32) {
    match sheet {
        Sheet::Main => (275, 116),
        Sheet::Titlebar => (344, 87),
        Sheet::Cbuttons => (136, 36),
        Sheet::Numbers | Sheet::NumsEx => (108, 13),
        Sheet::Playpaus => (48, 9),
        Sheet::Monoster => (56, 24),
        Sheet::Posbar => (307, 10),
        Sheet::Volume => (68, 433),
        Sheet::Balance => (47, 433),
        Sheet::Shufrep => (92, 85),
        Sheet::Text => (155, 18),
        Sheet::Pledit => (280, 190),
        Sheet::Eqmain => (275, 315),
        Sheet::EqEx => (275, 60),
        Sheet::Gen => (275, 100),
        Sheet::Genex | Sheet::Video | Sheet::Mb => (16, 16),
    }
}

/// Encodes a sheet coordinate as an opaque, non-magenta `0xAARRGGBB` value.
/// Coordinates up to 4095 round-trip through [`decode_coord`].
pub fn encode(x: u32, y: u32) -> u32 {
    let b = (((x >> 8) & 0xF) << 4) | ((y >> 8) & 0xF);
    0xFF00_0000 | ((x & 0xFF) << 16) | ((y & 0xFF) << 8) | b
}

/// Inverse of [`encode`].
pub fn decode_coord(px: u32) -> (u32, u32) {
    let r = (px >> 16) & 0xFF;
    let g = (px >> 8) & 0xFF;
    let b = px & 0xFF;
    (r | (((b >> 4) & 0xF) << 8), g | ((b & 0xF) << 8))
}

/// A sheet whose every pixel encodes its own coordinate.
pub fn coordinate_sheet(sheet: Sheet) -> Rgba {
    let (w, h) = sheet_size(sheet);
    let px = (0..h)
        .flat_map(|y| (0..w).map(move |x| encode(x, y)))
        .collect();
    Rgba { w, h, px }
}

/// Encodes an RGBA image as a 24-bit BMP (alpha is dropped).
pub fn bmp_bytes(img: &Rgba) -> Vec<u8> {
    let mut rgb = Vec::with_capacity(img.px.len() * 3);
    for &p in &img.px {
        rgb.extend_from_slice(&[(p >> 16) as u8, (p >> 8) as u8, p as u8]);
    }
    let buffer = image::RgbImage::from_raw(img.w, img.h, rgb).expect("pixel buffer matches size");
    let mut out = Vec::new();
    image::DynamicImage::ImageRgb8(buffer)
        .write_to(&mut Cursor::new(&mut out), image::ImageFormat::Bmp)
        .expect("encode bmp");
    out
}

/// Builds a stored (uncompressed) zip archive from `(entry name, bytes)` pairs.
pub fn zip_bytes<S: AsRef<str>>(entries: &[(S, Vec<u8>)]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    for (name, data) in entries {
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        writer
            .start_file(name.as_ref(), options)
            .expect("start zip entry");
        writer.write_all(data).expect("write zip entry");
    }
    writer.finish().expect("finish zip").into_inner()
}

const PLEDIT_TEXT: &str =
    "[text]\r\nnormal=#00FF00\r\ncurrent=#FFFFFF\r\nnormalbg=#000000\r\nselectedbg=#0000FF\r\nfont=Arial\r\n";
const REGION_TEXT: &str = "[Normal]\nNumPoints=4\nPointList=0,0 275,0 275,116 0,116\n";

fn viscolor_text() -> String {
    let mut out = String::from("// synthetic viscolor\n");
    for c in DEFAULT_VISCOLORS {
        out.push_str(&format!(
            "{},{},{}\n",
            (c >> 16) & 0xFF,
            (c >> 8) & 0xFF,
            c & 0xFF
        ));
    }
    out
}

/// Zip bytes for a synthetic skin with every sheet except `skip`.
///
/// Bitmap names alternate between lowercase in a lowercase folder and
/// mixed-case in a `Synth Skin/` folder, so lookups are exercised.
pub fn synthetic_zip(skip: &[Sheet]) -> Vec<u8> {
    synthetic_zip_with(skip, &[])
}

/// Like [`synthetic_zip`], but the listed sheets are replaced by `overrides`.
pub fn synthetic_zip_with(skip: &[Sheet], overrides: &[(Sheet, Rgba)]) -> Vec<u8> {
    let mut entries: Vec<(String, Vec<u8>)> = Vec::new();
    for (i, sheet) in Sheet::ALL.iter().enumerate() {
        if skip.contains(sheet) {
            continue;
        }
        let name = if i % 2 == 0 {
            format!("synth skin/{}.bmp", sheet.base_name().to_lowercase())
        } else {
            format!("Synth Skin/{}.BMP", sheet.base_name())
        };
        let image = match overrides.iter().find(|(s, _)| s == sheet) {
            Some((_, img)) => img.clone(),
            None => coordinate_sheet(*sheet),
        };
        entries.push((name, bmp_bytes(&image)));
    }
    entries.push((
        "Synth Skin/viscolor.txt".to_string(),
        viscolor_text().into_bytes(),
    ));
    entries.push((
        "Synth Skin/PLEDIT.TXT".to_string(),
        PLEDIT_TEXT.as_bytes().to_vec(),
    ));
    entries.push((
        "Synth Skin/Region.txt".to_string(),
        REGION_TEXT.as_bytes().to_vec(),
    ));
    zip_bytes(&entries)
}

/// A complete synthetic skin (all sheets, coordinate-encoded).
pub fn synthetic_skin() -> Skin {
    synthetic_skin_without(&[])
}

/// A synthetic skin with the listed sheets omitted.
pub fn synthetic_skin_without(skip: &[Sheet]) -> Skin {
    Skin::load(&synthetic_zip(skip)).expect("synthetic skin is valid")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sprites;

    #[test]
    fn synthetic_sheets_cover_every_sprite_rectangle() {
        for sheet in Sheet::ALL {
            let (w, h) = sheet_size(sheet);
            for r in sprites::table(sheet) {
                assert!(
                    r.x + r.w <= w && r.y + r.h <= h,
                    "{:?} {} exceeds {w}x{h}",
                    sheet,
                    r.name
                );
            }
        }
    }

    #[test]
    fn encode_round_trips_and_is_never_magenta() {
        for (x, y) in [(0, 0), (23, 18), (255, 255), (300, 433), (4095, 4095)] {
            assert_eq!(decode_coord(encode(x, y)), (x, y));
            assert_ne!(encode(x, y) & 0x00FF_FFFF, 0x00FF_00FF);
        }
    }
}
