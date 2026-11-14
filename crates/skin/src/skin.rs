//! Loading a `.wsz` (zip) skin and looking up sprites in its sheets.
//!
//! Archive matching follows Webamp's `skinParserUtils.ts`: names are matched
//! case-insensitively, in any subdirectory, and when several entries match the
//! last one in archive order wins.

use crate::bitmap::{self, Rgba};
use crate::parse::{self, PlEditColors, Regions};
use crate::sheet::Sheet;
use crate::sprites;
use std::fmt;
use std::io::{Cursor, Read};
use std::path::Path;
use zip::ZipArchive;

/// Errors from loading a skin.
#[derive(Debug)]
pub enum SkinError {
    /// The file could not be read.
    Io(std::io::Error),
    /// The file is not a readable zip archive.
    Zip(zip::result::ZipError),
    /// The archive has no `MAIN.BMP` (or `.PNG`).
    MissingMain,
    /// `MAIN.BMP` exists but cannot be decoded.
    BadMain(image::ImageError),
}

impl fmt::Display for SkinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SkinError::Io(e) => write!(f, "could not read skin: {e}"),
            SkinError::Zip(e) => write!(f, "skin is not a valid zip archive: {e}"),
            SkinError::MissingMain => write!(f, "skin has no MAIN.BMP"),
            SkinError::BadMain(e) => write!(f, "MAIN.BMP could not be decoded: {e}"),
        }
    }
}

impl std::error::Error for SkinError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            SkinError::Io(e) => Some(e),
            SkinError::Zip(e) => Some(e),
            SkinError::BadMain(e) => Some(e),
            SkinError::MissingMain => None,
        }
    }
}

impl From<std::io::Error> for SkinError {
    fn from(e: std::io::Error) -> Self {
        SkinError::Io(e)
    }
}

impl From<zip::result::ZipError> for SkinError {
    fn from(e: zip::result::ZipError) -> Self {
        SkinError::Zip(e)
    }
}

/// A rectangle inside a sheet, clipped to the sheet bounds.
#[derive(Debug, Clone, Copy)]
pub struct SpriteView<'a> {
    pub sheet: &'a Rgba,
    pub x: u32,
    pub y: u32,
    pub w: u32,
    pub h: u32,
}

impl SpriteView<'_> {
    /// Pixel at `(x, y)` relative to the view origin. Callers must stay within `w` × `h`.
    pub fn pixel(&self, x: u32, y: u32) -> u32 {
        debug_assert!(x < self.w && y < self.h);
        self.sheet.px[((self.y + y) * self.sheet.w + self.x + x) as usize]
    }
}

fn clip_view(sheet: &Rgba, x: u32, y: u32, w: u32, h: u32) -> SpriteView<'_> {
    let x = x.min(sheet.w);
    let y = y.min(sheet.h);
    SpriteView {
        sheet,
        x,
        y,
        w: w.min(sheet.w - x),
        h: h.min(sheet.h - y),
    }
}

/// A loaded classic skin.
#[derive(Debug, Clone)]
pub struct Skin {
    sheets: Vec<Option<Rgba>>,
    /// Visualiser colours, `0xFFRRGGBB`.
    pub viscolor: [u32; 24],
    /// Playlist editor colours.
    pub pledit: PlEditColors,
    /// Window regions from `REGION.TXT` (parsed and stored; not used for rendering yet).
    pub regions: Regions,
}

impl Skin {
    /// Loads a skin from the bytes of a `.wsz` (zip) archive.
    pub fn load(bytes: &[u8]) -> Result<Skin, SkinError> {
        let mut archive = ZipArchive::new(Cursor::new(bytes))?;

        let mut sheets: Vec<Option<Rgba>> = vec![None; Sheet::ALL.len()];
        for sheet in Sheet::ALL {
            let Some(index) = find_last(&mut archive, sheet.base_name(), &["bmp", "png"])? else {
                if sheet == Sheet::Main {
                    return Err(SkinError::MissingMain);
                }
                continue;
            };
            let data = read_entry(&mut archive, index)?;
            match bitmap::decode(&data) {
                Ok(img) => sheets[sheet.index()] = Some(img),
                Err(e) if sheet == Sheet::Main => return Err(SkinError::BadMain(e)),
                // An undecodable optional sheet is treated as absent.
                Err(_) => {}
            }
        }

        let viscolor = match text_entry(&mut archive, "VISCOLOR")? {
            Some(text) => parse::parse_viscolor(&text),
            None => parse::DEFAULT_VISCOLORS,
        };
        let pledit = match text_entry(&mut archive, "PLEDIT")? {
            Some(text) => parse::parse_pledit(&text),
            None => PlEditColors::default(),
        };
        let regions = match text_entry(&mut archive, "REGION")? {
            Some(text) => parse::parse_region(&text),
            None => Regions::new(),
        };

        Ok(Skin {
            sheets,
            viscolor,
            pledit,
            regions,
        })
    }

    /// Loads a skin from a `.wsz` file on disk.
    pub fn load_path(path: &Path) -> Result<Skin, SkinError> {
        let bytes = std::fs::read(path)?;
        Skin::load(&bytes)
    }

    /// The decoded bitmap for a sheet, if the skin provides it.
    pub fn sheet(&self, sheet: Sheet) -> Option<&Rgba> {
        self.sheets[sheet.index()].as_ref()
    }

    /// Whether the skin ships its own `NUMS_EX.BMP` (affects the minus-sign layout).
    pub fn has_nums_ex(&self) -> bool {
        self.sheet(Sheet::NumsEx).is_some()
    }

    /// A named sprite from a sheet, clipped to the sheet bounds.
    ///
    /// `Sheet::NumsEx` falls back to `NUMBERS` when the skin has no
    /// `NUMS_EX.BMP`: the `_EX` suffix is dropped and the NUMBERS table is used.
    /// Returns `None` if the sheet or the sprite name is unknown.
    pub fn sprite(&self, sheet: Sheet, name: &str) -> Option<SpriteView<'_>> {
        if let Some(img) = self.sheet(sheet) {
            let r = sprites::lookup(sheet, name)?;
            return Some(clip_view(img, r.x, r.y, r.w, r.h));
        }
        if sheet == Sheet::NumsEx {
            let base = name.strip_suffix("_EX")?;
            let img = self.sheet(Sheet::Numbers)?;
            let r = sprites::lookup(Sheet::Numbers, base)?;
            return Some(clip_view(img, r.x, r.y, r.w, r.h));
        }
        None
    }

    /// An arbitrary rectangle of a sheet, clipped to the sheet bounds.
    pub fn region(&self, sheet: Sheet, x: u32, y: u32, w: u32, h: u32) -> Option<SpriteView<'_>> {
        let img = self.sheet(sheet)?;
        Some(clip_view(img, x, y, w, h))
    }
}

/// Returns true if an archive entry name refers to `base.ext` for one of `exts`.
fn matches_name(name: &str, base: &str, exts: &[&str]) -> bool {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    let Some((stem, ext)) = file.rsplit_once('.') else {
        return false;
    };
    stem.eq_ignore_ascii_case(base) && exts.iter().any(|e| ext.eq_ignore_ascii_case(e))
}

/// Index of the last archive entry matching `base.ext` (any directory).
fn find_last(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    base: &str,
    exts: &[&str],
) -> Result<Option<usize>, SkinError> {
    let mut found = None;
    for i in 0..archive.len() {
        let entry = archive.by_index_raw(i)?;
        if entry.is_dir() {
            continue;
        }
        if matches_name(entry.name(), base, exts) {
            found = Some(i);
        }
    }
    Ok(found)
}

fn read_entry(archive: &mut ZipArchive<Cursor<&[u8]>>, index: usize) -> Result<Vec<u8>, SkinError> {
    let mut file = archive.by_index(index)?;
    let mut data = Vec::with_capacity(file.size() as usize);
    file.read_to_end(&mut data)?;
    Ok(data)
}

/// Reads a `base.txt` entry as text (lossy UTF-8, BOM removed).
fn text_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    base: &str,
) -> Result<Option<String>, SkinError> {
    let Some(index) = find_last(archive, base, &["txt"])? else {
        return Ok(None);
    };
    let data = read_entry(archive, index)?;
    let text = String::from_utf8_lossy(&data);
    Ok(Some(text.trim_start_matches('\u{feff}').to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bitmap::argb;
    use crate::testkit;

    fn solid(w: u32, h: u32, color: u32) -> Rgba {
        Rgba {
            w,
            h,
            px: vec![color; (w * h) as usize],
        }
    }

    fn bmp_of(img: &Rgba) -> Vec<u8> {
        testkit::bmp_bytes(img)
    }

    #[test]
    fn name_matching_is_case_insensitive_and_recurses_into_folders() {
        assert!(matches_name("MAIN.BMP", "main", &["bmp"]));
        assert!(matches_name(
            "skins/Sub Dir/main.Bmp",
            "MAIN",
            &["bmp", "png"]
        ));
        assert!(matches_name("a\\b\\Main.png", "MAIN", &["bmp", "png"]));
        assert!(!matches_name("MAIN.TXT", "MAIN", &["bmp"]));
        assert!(!matches_name("XMAIN.BMP", "MAIN", &["bmp"]));
        assert!(!matches_name("MAIN_EX.BMP", "MAIN", &["bmp"]));
    }

    #[test]
    fn lookup_finds_mixed_case_entries_in_subfolders() {
        let bytes = testkit::zip_bytes(&[
            (
                "Deep/Folder/mAiN.bMp",
                bmp_of(&solid(275, 116, argb(0xFF, 9, 9, 9))),
            ),
            (
                "Deep/Folder/titlebar.BMP",
                bmp_of(&solid(344, 87, argb(0xFF, 1, 1, 1))),
            ),
        ]);
        let skin = Skin::load(&bytes).expect("load");
        assert_eq!(
            skin.sheet(Sheet::Main).map(|s| (s.w, s.h)),
            Some((275, 116))
        );
        assert!(skin.sheet(Sheet::Titlebar).is_some());
        assert!(skin.sheet(Sheet::Cbuttons).is_none());
    }

    #[test]
    fn last_matching_entry_wins() {
        let bytes = testkit::zip_bytes(&[
            ("MAIN.BMP", bmp_of(&solid(10, 10, argb(0xFF, 255, 0, 0)))),
            (
                "old/main.bmp",
                bmp_of(&solid(275, 116, argb(0xFF, 0, 0, 255))),
            ),
        ]);
        let skin = Skin::load(&bytes).expect("load");
        let main = skin.sheet(Sheet::Main).expect("main");
        assert_eq!((main.w, main.h), (275, 116));
        assert_eq!(main.px[0], argb(0xFF, 0, 0, 255));
    }

    #[test]
    fn sprite_views_read_the_expected_sheet_coordinates() {
        let skin = testkit::synthetic_skin();
        let play = skin
            .sprite(Sheet::Cbuttons, "MAIN_PLAY_BUTTON")
            .expect("play");
        assert_eq!((play.x, play.y, play.w, play.h), (23, 0, 23, 18));
        assert_eq!(testkit::decode_coord(play.pixel(0, 0)), (23, 0));
        assert_eq!(testkit::decode_coord(play.pixel(22, 17)), (45, 17));

        let active = skin
            .sprite(Sheet::Cbuttons, "MAIN_PLAY_BUTTON_ACTIVE")
            .expect("active");
        assert_eq!(testkit::decode_coord(active.pixel(3, 4)), (26, 22));

        let digit = skin.sprite(Sheet::Numbers, "DIGIT_7").expect("digit");
        assert_eq!(testkit::decode_coord(digit.pixel(0, 12)), (63, 12));
    }

    #[test]
    fn nums_ex_falls_back_to_numbers_when_absent() {
        let skin = testkit::synthetic_skin_without(&[Sheet::NumsEx]);
        assert!(!skin.has_nums_ex());
        assert!(skin.sheet(Sheet::NumsEx).is_none());
        let digit = skin
            .sprite(Sheet::NumsEx, "DIGIT_3_EX")
            .expect("fallback digit");
        assert_eq!(testkit::decode_coord(digit.pixel(0, 0)), (27, 0));
        let minus = skin
            .sprite(Sheet::NumsEx, "MINUS_SIGN_EX")
            .expect("fallback minus");
        assert_eq!((minus.w, minus.h), (5, 1));
        assert!(skin.sprite(Sheet::NumsEx, "DIGIT_3").is_none());
    }

    #[test]
    fn nums_ex_is_used_when_present() {
        let bytes = testkit::zip_bytes(&[
            ("MAIN.BMP", bmp_of(&solid(275, 116, argb(0xFF, 0, 0, 0)))),
            ("NUMBERS.BMP", bmp_of(&solid(108, 13, argb(0xFF, 1, 1, 1)))),
            ("NUMS_EX.BMP", bmp_of(&solid(108, 13, argb(0xFF, 2, 2, 2)))),
        ]);
        let skin = Skin::load(&bytes).expect("load");
        assert!(skin.has_nums_ex());
        let digit = skin.sprite(Sheet::NumsEx, "DIGIT_0_EX").expect("digit");
        assert_eq!(digit.pixel(0, 0), argb(0xFF, 2, 2, 2));
    }

    #[test]
    fn undersized_sheets_are_clipped_and_do_not_panic() {
        let bytes = testkit::zip_bytes(&[
            ("MAIN.BMP", bmp_of(&solid(10, 10, argb(0xFF, 5, 5, 5)))),
            ("TITLEBAR.BMP", bmp_of(&solid(4, 4, argb(0xFF, 6, 6, 6)))),
        ]);
        let skin = Skin::load(&bytes).expect("load");
        let bg = skin
            .sprite(Sheet::Main, "MAIN_WINDOW_BACKGROUND")
            .expect("bg");
        assert_eq!((bg.x, bg.y, bg.w, bg.h), (0, 0, 10, 10));
        let close = skin
            .sprite(Sheet::Titlebar, "MAIN_CLOSE_BUTTON")
            .expect("close");
        assert_eq!((close.x, close.y, close.w, close.h), (4, 0, 0, 4));
        // A name that belongs to another sheet's table is not found here.
        assert!(skin.sprite(Sheet::Main, "MAIN_TITLE_BAR").is_none());
        assert!(skin
            .region(Sheet::Main, 8, 8, 100, 100)
            .is_some_and(|v| (v.w, v.h) == (2, 2)));
    }

    #[test]
    fn missing_main_is_an_error() {
        let bytes = testkit::zip_bytes(&[("TITLEBAR.BMP", bmp_of(&solid(4, 4, 0)))]);
        assert!(matches!(Skin::load(&bytes), Err(SkinError::MissingMain)));
    }

    #[test]
    fn undecodable_main_is_an_error_and_not_a_zip_is_an_error() {
        let bytes = testkit::zip_bytes(&[("MAIN.BMP", b"garbage".to_vec())]);
        assert!(matches!(Skin::load(&bytes), Err(SkinError::BadMain(_))));
        assert!(matches!(Skin::load(b"not a zip"), Err(SkinError::Zip(_))));
    }

    #[test]
    fn viscolor_pledit_and_region_come_from_the_archive_or_defaults() {
        let bytes = testkit::zip_bytes(&[
            ("MAIN.BMP", bmp_of(&solid(275, 116, 0))),
            ("sub/VisColor.TXT", b"1,2,3\n".to_vec()),
            ("PLEDIT.txt", b"[text]\nnormal=#010203\n".to_vec()),
            (
                "region.txt",
                b"[Normal]\nNumPoints=3\nPointList=0,0 4,0 0,4\n".to_vec(),
            ),
        ]);
        let skin = Skin::load(&bytes).expect("load");
        assert_eq!(skin.viscolor[0], argb(0xFF, 1, 2, 3));
        assert_eq!(skin.viscolor[1], parse::DEFAULT_VISCOLORS[1]);
        assert_eq!(skin.pledit.normal, argb(0xFF, 1, 2, 3));
        assert_eq!(skin.regions["normal"].len(), 1);

        let bare = Skin::load(&testkit::zip_bytes(&[(
            "MAIN.BMP",
            bmp_of(&solid(1, 1, 0)),
        )]))
        .expect("load bare");
        assert_eq!(bare.viscolor, parse::DEFAULT_VISCOLORS);
        assert_eq!(bare.pledit, PlEditColors::default());
        assert!(bare.regions.is_empty());
    }
}
