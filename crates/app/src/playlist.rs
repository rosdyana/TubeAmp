//! Playlist editor helpers that sit outside the event loop: which action each popup item runs,
//! the MISC ▸ Sort menu (`muda`), the LIST file dialogs (`rfd`) and reading a link from the
//! clipboard (`arboard`).

use std::path::{Path, PathBuf};

use tubeamp_playlist::{parse_links, Entry, Link, Playlist};
use tubeamp_ui::playlist_window::PlMenu;

/// What a popup item does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlAction {
    /// ADD ▸ URL: add the YouTube links on the clipboard.
    AddUrl,
    /// ADD ▸ FILE: add the playlists or links in the files the user picks.
    AddFile,
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
/// yet: REM ▸ MISC, and the MISC ▸ FILE INFO and MISC ▸ OPTIONS.
pub fn action_for(menu: PlMenu, index: usize) -> Option<PlAction> {
    match (menu, index) {
        (PlMenu::Add, 0) => Some(PlAction::AddUrl),
        (PlMenu::Add, 1) => Some(PlAction::AddFile),
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

/// Asks for a playlist file to load: JSON from SAVE, or a text file of links.
pub fn pick_list_to_load() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Load playlist")
        .add_filter("TubeAmp playlist", &["json", "txt"])
        .pick_file()
}

/// Asks where to save the playlist. The extension picked decides the format.
pub fn pick_list_to_save() -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Save playlist")
        .add_filter("TubeAmp playlist", &["json"])
        .add_filter("Text of YouTube links", &["txt"])
        .set_file_name("playlist.json")
        .save_file()
}

/// Asks for the files to add to the playlist: text, M3U or JSON.
pub fn pick_files_to_add() -> Option<Vec<PathBuf>> {
    rfd::FileDialog::new()
        .set_title("Add files")
        .add_filter("Playlists", &["txt", "m3u", "json"])
        .pick_files()
}

/// The YouTube links on the clipboard. A `@handle` link has its own message, because it can not be
/// read without a request.
pub fn clipboard_links() -> Result<Vec<Link>, String> {
    let mut clipboard = arboard::Clipboard::new().map_err(|e| format!("no clipboard: {e}"))?;
    let text = clipboard
        .get_text()
        .map_err(|e| format!("the clipboard has no text: {e}"))?;
    let links = parse_links(&text);
    if !links.is_empty() {
        Ok(links)
    } else if text.contains("youtube.com/@") {
        Err("handle links (youtube.com/@name) are not supported yet; copy the channel link".into())
    } else {
        Err("the clipboard has no YouTube link or video id".into())
    }
}

/// Whether a playlist file holds the JSON that SAVE writes (an array), rather than links.
pub fn is_json_list(text: &str) -> bool {
    text.trim_start().starts_with('[')
}

/// The list a LOAD file gives, and how many links in it were left out. JSON loads whole. From
/// text only the video links load: a playlist, album or artist would need a request, so it is
/// left for ADD ▸ FILE.
pub fn list_from_text(text: &str) -> Result<(Playlist, usize), String> {
    if is_json_list(text) {
        return Playlist::from_json(text)
            .map(|list| (list, 0))
            .map_err(|e| e.to_string());
    }
    let mut skipped = 0;
    let entries: Vec<Entry> = parse_links(text)
        .into_iter()
        .filter_map(|link| match link {
            Link::Video(id) => Some(Entry::from_id(&id)),
            _ => {
                skipped += 1;
                None
            }
        })
        .collect();
    Ok((Playlist::from_entries(entries), skipped))
}

/// The text LIST ▸ SAVE writes for `path`: one link per line when the name ends in `.txt`, and
/// JSON otherwise.
pub fn list_text(list: &Playlist, path: &Path) -> String {
    if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("txt"))
    {
        list.to_links_text()
    } else {
        list.to_json()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_items_map_to_their_actions() {
        assert_eq!(action_for(PlMenu::Add, 0), Some(PlAction::AddUrl));
        assert_eq!(action_for(PlMenu::Add, 1), Some(PlAction::AddFile));
        assert_eq!(action_for(PlMenu::Add, 2), None, "ADD has two items");
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
    fn a_json_list_starts_with_an_array_and_anything_else_is_links() {
        assert!(is_json_list("  \n[{\"video_id\":\"kJQP7kiw5Fk\"}]"));
        assert!(!is_json_list("https://youtu.be/kJQP7kiw5Fk\n"));
        assert!(!is_json_list("#EXTM3U\n#EXTINF:1,x\n"));
    }

    #[test]
    fn load_reads_json_and_keeps_only_the_video_links_of_a_text_file() {
        let (list, skipped) = list_from_text(
            "kJQP7kiw5Fk, https://music.youtube.com/playlist?list=PLabc\nfJ9rUzIMcZQ\n",
        )
        .expect("text");
        let ids: Vec<&str> = list.entries().iter().map(|e| e.video_id.as_str()).collect();
        assert_eq!(ids, vec!["kJQP7kiw5Fk", "fJ9rUzIMcZQ"]);
        assert_eq!(skipped, 1, "the playlist link is left for ADD FILE");

        let saved = Playlist::from_entries(vec![Entry {
            video_id: "kJQP7kiw5Fk".into(),
            title: "Despacito".into(),
            artist: "Luis Fonsi".into(),
            duration_secs: Some(229),
        }]);
        let (back, skipped) = list_from_text(&saved.to_json()).expect("json");
        assert_eq!(back.entries(), saved.entries());
        assert_eq!(skipped, 0);
        assert!(list_from_text("[not json").is_err());
    }

    #[test]
    fn save_writes_text_for_a_txt_name_and_json_otherwise() {
        let list = Playlist::from_entries(vec![Entry::from_id("kJQP7kiw5Fk")]);
        assert_eq!(
            list_text(&list, Path::new("/tmp/list.txt")),
            "https://music.youtube.com/watch?v=kJQP7kiw5Fk\n"
        );
        assert_eq!(
            list_text(&list, Path::new("/tmp/list.TXT")),
            list.to_links_text()
        );
        assert_eq!(
            list_text(&list, Path::new("/tmp/list.json")),
            list.to_json()
        );
        assert_eq!(list_text(&list, Path::new("/tmp/list")), list.to_json());
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
