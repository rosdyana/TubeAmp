//! Skins: the skins folder (`<config>/skins/`), the skin browser's menu (Winamp's Alt+S, as a muda
//! submenu), copying a `.wsz` into the folder, and the file picker. The loading and applying happen
//! in `window/session.rs`, which has the app's state.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use tubeamp_skin::{Skin, SkinError};

use crate::config::{SKINS_DIR, SKIN_DEFAULT};

/// The label of the built-in skin in the browser.
pub const DEFAULT_LABEL: &str = "TubeAmp Classic (default)";
/// Menu id prefix of a skin choice: `skin.pick:default` or `skin.pick:<absolute path>`.
pub const PICK_PREFIX: &str = "skin.pick:";
/// Menu id of "Get the classic Winamp skin…".
pub const GET_CLASSIC: &str = "skin.classic";
/// Menu id of "Open skins folder".
pub const OPEN_FOLDER: &str = "skin.folder";
/// Menu id of "Load skin…".
pub const LOAD_SKIN: &str = "skin.load";

/// Which skin is applied: the built-in one, or a `.wsz` file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Choice {
    Default,
    File(PathBuf),
}

impl Choice {
    /// The value stored in the config: `"default"` or the file's path.
    pub fn to_pref(&self) -> String {
        match self {
            Choice::Default => SKIN_DEFAULT.to_string(),
            Choice::File(path) => path.to_string_lossy().into_owned(),
        }
    }

    /// Reads a config value. An empty value means the default skin.
    pub fn from_pref(pref: &str) -> Choice {
        if pref.trim().is_empty() || pref == SKIN_DEFAULT {
            Choice::Default
        } else {
            Choice::File(PathBuf::from(pref))
        }
    }

    /// The browser item's menu id for this choice.
    pub fn menu_id(&self) -> String {
        format!("{PICK_PREFIX}{}", self.to_pref())
    }

    /// The choice a browser item's menu id stands for, if it is one.
    pub fn from_menu_id(id: &str) -> Option<Choice> {
        id.strip_prefix(PICK_PREFIX).map(Choice::from_pref)
    }
}

/// A `.wsz` in the skins folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkinFile {
    pub path: PathBuf,
    /// The file name without `.wsz`.
    pub label: String,
}

/// The skin to start with, from the config value. A skin file that is missing or broken gives the
/// built-in skin and a note for the marquee (the second value). `default` is the built-in skin's
/// bytes.
pub fn startup_skin(pref: &str, default: &[u8]) -> Result<(Skin, Option<String>), SkinError> {
    match Choice::from_pref(pref) {
        Choice::Default => Ok((Skin::load(default)?, None)),
        Choice::File(path) => match Skin::load_path(&path) {
            Ok(skin) => Ok((skin, None)),
            Err(e) => Ok((
                Skin::load(default)?,
                Some(format!(
                    "Could not load {}: {e}. Using TubeAmp Classic.",
                    path.display()
                )),
            )),
        },
    }
}

/// The skins folder under the config folder.
pub fn skins_dir(config_dir: &Path) -> PathBuf {
    config_dir.join(SKINS_DIR)
}

/// Whether the path names a `.wsz` (any case).
pub fn is_wsz(path: &Path) -> bool {
    path.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("wsz"))
}

/// The `.wsz` files in `dir`, sorted by name without regard to case. A missing folder is empty.
pub fn list(dir: &Path) -> Vec<SkinFile> {
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut skins: Vec<SkinFile> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_file() && is_wsz(path))
        .map(|path| SkinFile {
            label: path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            path,
        })
        .collect();
    skins.sort_by_key(|skin| skin.label.to_lowercase());
    skins
}

/// Copies `src` into `dir` and returns its new path. A file with the same name and the same bytes
/// is reused. A different file with the same name gets " (2)", " (3)" and so on. A `src` that is
/// already in `dir` is used as it is.
pub fn import(dir: &Path, src: &Path) -> io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    if src.parent().is_some_and(|parent| parent == dir) {
        return Ok(src.to_path_buf());
    }
    let bytes = fs::read(src)?;
    let stem = src
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "skin".to_string());
    let mut n = 1;
    loop {
        let name = if n == 1 {
            format!("{stem}.wsz")
        } else {
            format!("{stem} ({n}).wsz")
        };
        let dest = dir.join(name);
        match fs::read(&dest) {
            Ok(existing) if existing == bytes => return Ok(dest),
            Ok(_) => n += 1,
            Err(_) => {
                crate::config::write_atomic(&dest, &bytes)?;
                return Ok(dest);
            }
        }
    }
}

/// The skin browser: the built-in skin, every skin in `skins` (the current one ticked), then the
/// fixed items. `current` is the applied choice.
pub fn build_submenu(current: &Choice, skins: &[SkinFile]) -> Result<muda::Submenu, muda::Error> {
    use muda::{CheckMenuItem, MenuItem, PredefinedMenuItem, Submenu};

    let menu = Submenu::new("Skins", true);
    menu.append(&CheckMenuItem::with_id(
        Choice::Default.menu_id(),
        crate::menu_label::escape(DEFAULT_LABEL),
        true,
        *current == Choice::Default,
        None,
    ))?;
    for skin in skins {
        let choice = Choice::File(skin.path.clone());
        menu.append(&CheckMenuItem::with_id(
            choice.menu_id(),
            crate::menu_label::escape(&skin.label),
            true,
            *current == choice,
            None,
        ))?;
    }
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(
        GET_CLASSIC,
        "Get the classic Winamp skin…",
        true,
        None,
    ))?;
    menu.append(&MenuItem::with_id(OPEN_FOLDER, "Open skins folder", true, None))?;
    menu.append(&MenuItem::with_id(LOAD_SKIN, "Load skin…", true, None))?;
    Ok(menu)
}

/// Shows the folder in Finder or Explorer.
pub fn open_folder(dir: &Path) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let program = if cfg!(target_os = "macos") {
        "open"
    } else if cfg!(target_os = "windows") {
        "explorer"
    } else {
        return Err("opening a folder needs macOS or Windows".to_string());
    };
    std::process::Command::new(program)
        .arg(dir)
        .spawn()
        .map(|_| ())
        .map_err(|e| format!("could not open {}: {e}", dir.display()))
}

/// Asks for a `.wsz` file to load.
pub fn pick_skin_file() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Load skin")
        .add_filter("Winamp skins", &["wsz"])
        .pick_file()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "tubeamp-skins-test-{}-{name}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn choices_round_trip_through_the_config_and_the_menu() {
        assert_eq!(Choice::from_pref("default"), Choice::Default);
        assert_eq!(Choice::from_pref(""), Choice::Default);
        let file = Choice::File(PathBuf::from("/skins/Classic.wsz"));
        assert_eq!(Choice::from_pref(&file.to_pref()), file);
        assert_eq!(Choice::from_menu_id(&file.menu_id()), Some(file));
        assert_eq!(Choice::from_menu_id(&Choice::Default.menu_id()), Some(Choice::Default));
        assert_eq!(Choice::from_menu_id("eq.reset"), None);
    }

    #[test]
    fn the_browser_lists_only_wsz_files_sorted_by_name() {
        let dir = test_dir("list");
        fs::create_dir_all(&dir).unwrap();
        for name in ["zeta.wsz", "Alpha.WSZ", "beta.wsz", "notes.txt", "gamma.zip"] {
            fs::write(dir.join(name), b"x").unwrap();
        }
        fs::create_dir_all(dir.join("folder.wsz")).unwrap();
        let labels: Vec<String> = list(&dir).into_iter().map(|s| s.label).collect();
        assert_eq!(labels, ["Alpha", "beta", "zeta"]);
        assert!(list(&dir.join("missing")).is_empty());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn importing_copies_the_file_and_names_clashes_apart() {
        let root = test_dir("import");
        let src_dir = root.join("downloads");
        let skins = root.join("skins");
        fs::create_dir_all(&src_dir).unwrap();
        let src = src_dir.join("Classic.wsz");
        fs::write(&src, b"first").unwrap();

        let first = import(&skins, &src).expect("import");
        assert_eq!(first, skins.join("Classic.wsz"));
        assert_eq!(fs::read(&first).unwrap(), b"first");

        // The same bytes again: reused, not copied twice.
        assert_eq!(import(&skins, &src).unwrap(), first);

        // Different bytes under the same name: a new name.
        fs::write(&src, b"second").unwrap();
        let second = import(&skins, &src).unwrap();
        assert_eq!(second, skins.join("Classic (2).wsz"));
        assert_eq!(fs::read(&second).unwrap(), b"second");

        // A file already in the folder is used where it is.
        assert_eq!(import(&skins, &first).unwrap(), first);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn wsz_is_recognised_in_any_case() {
        assert!(is_wsz(Path::new("a/Classic.wsz")));
        assert!(is_wsz(Path::new("a/Classic.WSZ")));
        assert!(!is_wsz(Path::new("a/Classic.zip")));
        assert!(!is_wsz(Path::new("a/Classic")));
    }
}
