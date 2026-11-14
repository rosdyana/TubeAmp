//! Playlist editor helpers that sit outside the event loop: which action each popup item runs,
//! the MISC ▸ Sort menu (`muda`), the LIST file dialogs (`rfd`) and reading a link from the
//! clipboard (`arboard`).

use std::path::PathBuf;

use tubeamp_playlist::parse_video_id;
use tubeamp_ui::playlist_window::PlMenu;

/// What a popup item does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlAction {
    /// ADD ▸ URL: append the video from a link on the clipboard.
    AddUrl,
    /// REM ▸ ALL: remove everything.
    RemoveAll,
    /// REM ▸ CROP: keep only the selected rows.
    Crop,
    /// REM ▸ SEL: remove the selected rows.
    RemoveSelected,
    /// SEL ▸ ALL.
    SelectAll,
    /// SEL ▸ CLR.
    SelectNone,
    /// SEL ▸ INV.
    InvertSelection,
    /// MISC ▸ SORT: opens the sort menu.
    Sort,
    /// LIST ▸ NEW: same as REM ▸ ALL.
    NewList,
    /// LIST ▸ SAVE: write the list as JSON to a file the user picks.
    SaveList,
    /// LIST ▸ LOAD: replace the list with a JSON file the user picks.
    LoadList,
}

/// The action of popup item `index` (from the top) of `menu`. `None` for items that do nothing
/// yet: ADD ▸ DIR and ADD ▸ FILE, REM ▸ MISC, and the MISC ▸ FILE INFO and MISC ▸ OPTIONS.
pub fn action_for(menu: PlMenu, index: usize) -> Option<PlAction> {
    match (menu, index) {
        (PlMenu::Add, 0) => Some(PlAction::AddUrl),
        (PlMenu::Remove, 0) => Some(PlAction::RemoveAll),
        (PlMenu::Remove, 1) => Some(PlAction::Crop),
        (PlMenu::Remove, 2) => Some(PlAction::RemoveSelected),
        (PlMenu::Select, 0) => Some(PlAction::InvertSelection),
        (PlMenu::Select, 1) => Some(PlAction::SelectNone),
        (PlMenu::Select, 2) => Some(PlAction::SelectAll),
        (PlMenu::Misc, 0) => Some(PlAction::Sort),
        (PlMenu::List, 0) => Some(PlAction::NewList),
        (PlMenu::List, 1) => Some(PlAction::SaveList),
        (PlMenu::List, 2) => Some(PlAction::LoadList),
        _ => None,
    }
}

/// Sort menu commands. Their menu ids start with `pl.` so they do not clash with the EQ menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SortCommand {
    Title,
    Artist,
    Reverse,
    Randomize,
}

const SORT_TITLE: &str = "pl.sort.title";
const SORT_ARTIST: &str = "pl.sort.artist";
const SORT_REVERSE: &str = "pl.sort.reverse";
const SORT_RANDOMIZE: &str = "pl.sort.randomize";

/// Maps a sort menu id back to its command.
pub fn sort_command_for_id(id: &str) -> Option<SortCommand> {
    match id {
        SORT_TITLE => Some(SortCommand::Title),
        SORT_ARTIST => Some(SortCommand::Artist),
        SORT_REVERSE => Some(SortCommand::Reverse),
        SORT_RANDOMIZE => Some(SortCommand::Randomize),
        _ => None,
    }
}

/// The MISC ▸ SORT menu: by title, by artist, a separator, reverse, randomize.
pub fn build_sort_menu() -> Result<muda::Menu, muda::Error> {
    use muda::{Menu, MenuItem, PredefinedMenuItem};

    let menu = Menu::new();
    menu.append(&MenuItem::with_id(SORT_TITLE, "Sort by title", true, None))?;
    menu.append(&MenuItem::with_id(
        SORT_ARTIST,
        "Sort by artist",
        true,
        None,
    ))?;
    menu.append(&PredefinedMenuItem::separator())?;
    menu.append(&MenuItem::with_id(SORT_REVERSE, "Reverse", true, None))?;
    menu.append(&MenuItem::with_id(SORT_RANDOMIZE, "Randomize", true, None))?;
    Ok(menu)
}

/// Asks for a playlist file to load.
pub fn pick_list_to_load() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Load playlist")
        .add_filter("TubeAmp playlist", &["json"])
        .pick_file()
}

/// Asks where to save the playlist.
pub fn pick_list_to_save() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Save playlist")
        .add_filter("TubeAmp playlist", &["json"])
        .set_file_name("playlist.json")
        .save_file()
}

/// The video id in a YouTube link (or bare id) on the clipboard.
pub fn clipboard_video_id() -> Result<String, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("no clipboard: {e}"))?;
    let text = clipboard
        .get_text()
        .map_err(|e| format!("the clipboard has no text: {e}"))?;
    parse_video_id(&text).ok_or_else(|| "the clipboard has no YouTube link or video id".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_items_map_to_their_actions() {
        assert_eq!(action_for(PlMenu::Add, 0), Some(PlAction::AddUrl));
        assert_eq!(
            action_for(PlMenu::Add, 1),
            None,
            "ADD DIR is a no-op for now"
        );
        assert_eq!(
            action_for(PlMenu::Add, 2),
            None,
            "ADD FILE is a no-op for now"
        );
        assert_eq!(action_for(PlMenu::Remove, 0), Some(PlAction::RemoveAll));
        assert_eq!(action_for(PlMenu::Remove, 1), Some(PlAction::Crop));
        assert_eq!(
            action_for(PlMenu::Remove, 2),
            Some(PlAction::RemoveSelected)
        );
        assert_eq!(
            action_for(PlMenu::Remove, 3),
            None,
            "REM MISC is a no-op for now"
        );
        assert_eq!(
            action_for(PlMenu::Select, 0),
            Some(PlAction::InvertSelection)
        );
        assert_eq!(action_for(PlMenu::Select, 1), Some(PlAction::SelectNone));
        assert_eq!(action_for(PlMenu::Select, 2), Some(PlAction::SelectAll));
        assert_eq!(action_for(PlMenu::Misc, 0), Some(PlAction::Sort));
        assert_eq!(action_for(PlMenu::Misc, 1), None);
        assert_eq!(action_for(PlMenu::List, 0), Some(PlAction::NewList));
        assert_eq!(action_for(PlMenu::List, 1), Some(PlAction::SaveList));
        assert_eq!(action_for(PlMenu::List, 2), Some(PlAction::LoadList));
    }

    #[test]
    fn every_popup_item_has_an_entry_in_the_table() {
        // Each menu's items are the same count as `PlMenu::len`; the table answers for all of them.
        for menu in PlMenu::ALL {
            for i in 0..menu.len() {
                let _ = action_for(menu, i);
            }
            assert_eq!(action_for(menu, menu.len()), None);
        }
    }

    #[test]
    fn sort_ids_round_trip() {
        assert_eq!(sort_command_for_id(SORT_TITLE), Some(SortCommand::Title));
        assert_eq!(sort_command_for_id(SORT_ARTIST), Some(SortCommand::Artist));
        assert_eq!(
            sort_command_for_id(SORT_REVERSE),
            Some(SortCommand::Reverse)
        );
        assert_eq!(
            sort_command_for_id(SORT_RANDOMIZE),
            Some(SortCommand::Randomize)
        );
        assert_eq!(sort_command_for_id("eq.reset"), None);
    }
}
