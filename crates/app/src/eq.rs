//! Equalizer helpers that sit outside the event loop: the PRESETS menu (`muda`), `.eqf` bytes
//! (`tubeamp_eq::eqf`) and the file dialogs (`rfd`).

use std::path::{Path, PathBuf};

use tubeamp_eq::eqf::{self, EqfEntry};
use tubeamp_eq::{EqSettings, BUILTIN};

/// Menu id of "From EQF..." under Load.
pub const LOAD_EQF: &str = "eq.load.eqf";
/// Menu id of "To EQF..." under Save.
pub const SAVE_EQF: &str = "eq.save.eqf";
/// Menu id of "Reset" (flat).
pub const RESET: &str = "eq.reset";
/// Prefix of the built-in preset ids: `eq.load.preset.<index>`.
const PRESET_PREFIX: &str = "eq.load.preset.";

/// What a PRESETS menu item asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EqCommand {
    /// Index into [`BUILTIN`].
    Builtin(usize),
    LoadEqf,
    SaveEqf,
    Reset,
}

/// Maps a menu id back to its command.
pub fn command_for_id(id: &str) -> Option<EqCommand> {
    match id {
        LOAD_EQF => Some(EqCommand::LoadEqf),
        SAVE_EQF => Some(EqCommand::SaveEqf),
        RESET => Some(EqCommand::Reset),
        _ => {
            let index: usize = id.strip_prefix(PRESET_PREFIX)?.parse().ok()?;
            (index < BUILTIN.len()).then_some(EqCommand::Builtin(index))
        }
    }
}

/// The PRESETS menu, as Winamp has it: Load ▸ (built-in presets, separator, From EQF...),
/// Save ▸ (To EQF...), then Reset.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn build_presets_menu() -> Result<muda::Menu, muda::Error> {
    use muda::{Menu, MenuItem, PredefinedMenuItem, Submenu};

    let menu = Menu::new();
    let load = Submenu::new("Load", true);
    for (i, preset) in BUILTIN.iter().enumerate() {
        load.append(&MenuItem::with_id(
            format!("{PRESET_PREFIX}{i}"),
            crate::menu_label::escape(preset.name),
            true,
            None,
        ))?;
    }
    load.append(&PredefinedMenuItem::separator())?;
    load.append(&MenuItem::with_id(LOAD_EQF, "From EQF...", true, None))?;

    let save = Submenu::new("Save", true);
    save.append(&MenuItem::with_id(SAVE_EQF, "To EQF...", true, None))?;

    menu.append(&load)?;
    menu.append(&save)?;
    menu.append(&MenuItem::with_id(RESET, "Reset", true, None))?;
    Ok(menu)
}

/// Builds the PRESETS menu and shows it as a context menu with its top-left at `(x, y)`, in
/// logical pixels of `window`'s content view. The call blocks until the menu closes, so it must
/// run on the main thread. Item clicks arrive later as menu events.
#[cfg(any(target_os = "macos", target_os = "windows"))]
pub fn show_presets_menu(window: &winit::window::Window, x: f64, y: f64) -> Result<(), String> {
    let menu = build_presets_menu().map_err(|e| e.to_string())?;
    crate::skinned::show_context_menu(window, &menu, x, y)
        .map_err(|e| format!("the equalizer window: {e}"))
}

/// Other platforms have no PRESETS menu yet (macOS and Windows only).
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
pub fn show_presets_menu(_window: &winit::window::Window, _x: f64, _y: f64) -> Result<(), String> {
    Err("the PRESETS menu needs macOS or Windows".to_string())
}

/// Asks for an `.eqf` or `.q1` file to load.
pub fn pick_eqf_to_load() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Load EQ preset")
        .add_filter("Winamp EQ presets", &["eqf", "q1"])
        .pick_file()
}

/// Asks where to save an `.eqf` file.
pub fn pick_eqf_to_save() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Save EQ preset")
        .add_filter("Winamp EQ preset", &["eqf"])
        .set_file_name("TubeAmp.eqf")
        .save_file()
}

/// The entry name written for a file: its stem (`Rock.eqf` → `Rock`).
pub fn entry_name(path: &Path) -> String {
    path.file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("TubeAmp")
        .to_string()
}

/// The `.eqf` image for the current curve, as one entry named `name`.
pub fn eqf_bytes(name: &str, settings: &EqSettings) -> Vec<u8> {
    eqf::write(&[EqfEntry {
        name: name.to_string(),
        preamp: settings.preamp,
        bands: settings.bands,
    }])
}

/// The first entry of an `.eqf`/`.q1` image. A `.q1` may hold several presets; only the first
/// is applied for now.
pub fn first_entry(bytes: &[u8]) -> Result<EqfEntry, String> {
    let entries = eqf::parse(bytes).map_err(|e| e.to_string())?;
    entries
        .into_iter()
        .next()
        .ok_or_else(|| "no preset in the file".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_ids_map_back_to_commands() {
        assert_eq!(command_for_id(LOAD_EQF), Some(EqCommand::LoadEqf));
        assert_eq!(command_for_id(SAVE_EQF), Some(EqCommand::SaveEqf));
        assert_eq!(command_for_id(RESET), Some(EqCommand::Reset));
        assert_eq!(
            command_for_id("eq.load.preset.0"),
            Some(EqCommand::Builtin(0))
        );
        assert_eq!(
            command_for_id("eq.load.preset.16"),
            Some(EqCommand::Builtin(16))
        );
        assert_eq!(command_for_id("eq.load.preset.17"), None);
        assert_eq!(command_for_id("eq.load.preset.x"), None);
        assert_eq!(command_for_id("something else"), None);
    }

    #[test]
    fn every_builtin_has_a_menu_id() {
        for i in 0..BUILTIN.len() {
            assert_eq!(
                command_for_id(&format!("{PRESET_PREFIX}{i}")),
                Some(EqCommand::Builtin(i))
            );
        }
    }

    #[test]
    fn saved_eqf_round_trips_the_curve_and_names_the_entry_by_stem() {
        let settings = EqSettings {
            preamp: 40,
            bands: [1, 2, 3, 33, 33, 33, 33, 33, 60, 64],
            ..EqSettings::default()
        };
        let name = entry_name(Path::new("/some/dir/My Curve.eqf"));
        assert_eq!(name, "My Curve");
        let entry = first_entry(&eqf_bytes(&name, &settings)).expect("parse");
        assert_eq!(entry.name, "My Curve");
        assert_eq!(entry.preamp, 40);
        assert_eq!(entry.bands, settings.bands);
    }

    #[test]
    fn first_entry_rejects_non_eqf_bytes() {
        assert!(first_entry(b"not an eq file").is_err());
    }
}
