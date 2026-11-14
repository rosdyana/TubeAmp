//! The Media Library's behaviour: what the nav column and each row do, the list the window shows
//! and the requests behind it. The window owns the pointer and the keyboard; this module decides
//! what they mean. Every effect comes back as an [`Action`] for the window to carry out, so the
//! logic is testable without a window or a page.
//!
//! Artists have no browse page yet (ytmusicapi-style parsing of artist pages is not written), so
//! an artist row runs a Songs search for the artist's name. Radio appends the up-next queue.

use tubeamp_bridge::Cmd;
use tubeamp_playlist::Entry;
use tubeamp_ui::library_window::{LibKind, LibRow, LibraryState, NavKind};
use tubeamp_ytm as ytm;
use tubeamp_ytm::{
    AlbumItem, ArtistItem, ArtistRef, Item, PlaylistItem, Request, SearchFilter, SearchResults,
    Track,
};

use crate::api::{ApiManager, Purpose, Slot};

/// Shown in the status line while a request is out.
pub const LOADING: &str = "Loading...";
/// Shown instead of the library when no one is signed in.
pub const SIGN_IN_HINT: &str = "Sign in to see your library (press L)";
/// The captured response `--render-library` renders when no `--fixture` is given.
pub const DEFAULT_FIXTURE: &str = include_str!("../../ytm/tests/fixtures/search_daft_punk.json");
/// The search text `--render-library` shows in the field.
pub const RENDER_QUERY: &str = "daft punk";
/// How many rows before the end of the list the next page is asked for.
const MORE_MARGIN: usize = 8;
/// Nav items before the playlists: Search, Home, Library, Liked and the caption.
const FIXED_NAV: usize = 5;
/// The label of the row that goes back to the page before.
const BACK_LABEL: &str = "« Back";

/// Something for the window to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Send a command to the YouTube Music page.
    Send(Cmd),
    /// Append the entry to the playlist and play it (a track from a search or a shelf).
    AddAndPlay(Entry),
    /// Replace the playlist with `entries` and play the entry at `start`.
    ReplaceAndPlay { entries: Vec<Entry>, start: usize },
    /// Append the entries to the playlist.
    Append(Vec<Entry>),
}

/// The right-click commands on selected tracks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LibCommand {
    /// Replace the playlist with the selection and play it.
    Play,
    /// Append the selection to the playlist.
    Enqueue,
    /// Append the up-next queue of the first selected track.
    Radio,
}

/// What a row stands for. Kept beside the rows the window draws.
#[derive(Debug, Clone, PartialEq)]
enum Target {
    Back,
    Header(String),
    Track(Track),
    Album(AlbumItem),
    Artist(ArtistItem),
    Playlist(PlaylistItem),
}

/// How a list grows: shelves (search, home, library) or a flat run of tracks (an album, a
/// playlist, liked songs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Shape {
    Shelves,
    Tracks,
}

/// The content of the main list, before the Back row is added.
#[derive(Debug, Clone, PartialEq)]
struct Content {
    shape: Shape,
    targets: Vec<Target>,
    continuation: Option<String>,
    more_pending: bool,
}

impl Content {
    fn empty(shape: Shape) -> Content {
        Content {
            shape,
            targets: Vec::new(),
            continuation: None,
            more_pending: false,
        }
    }
}

/// The Media Library's state and logic.
#[derive(Debug)]
pub struct Library {
    /// What the window shows.
    pub state: LibraryState,
    api: ApiManager,
    content: Content,
    /// Pages under the current one, most recent last. "« Back" pops one.
    history: Vec<Content>,
    /// One target per row of `state.rows`.
    targets: Vec<Target>,
    /// The user's library playlists, in nav order.
    nav_playlists: Vec<PlaylistItem>,
    /// A search to run as soon as the page is ready (`--library-search`).
    startup_search: Option<String>,
    /// The video id that is playing, for the highlight.
    playing: Option<String>,
}

impl Default for Library {
    fn default() -> Self {
        Library::new()
    }
}

impl Library {
    pub fn new() -> Library {
        Library {
            state: LibraryState::default(),
            api: ApiManager::new(),
            content: Content::empty(Shape::Shelves),
            history: Vec::new(),
            targets: Vec::new(),
            nav_playlists: Vec::new(),
            startup_search: None,
            playing: None,
        }
    }

    /// Runs `query` as a search once the page is ready.
    pub fn set_startup_search(&mut self, query: String) {
        self.startup_search = Some(query);
    }

    /// The page is ready. Releases the queued requests, fills the nav column for a signed-in
    /// user, and runs the startup search if one was given.
    pub fn on_ready(&mut self, signed_in: bool) -> Vec<Action> {
        let released = self.api.on_ready(signed_in);
        let mut out: Vec<Action> = released.cmds.into_iter().map(Action::Send).collect();
        if released
            .refused
            .iter()
            .any(|p| matches!(p, Purpose::Library | Purpose::Liked))
        {
            self.state.status = SIGN_IN_HINT.to_string();
        }
        if signed_in {
            out.extend(self.request(Purpose::NavPlaylists, ytm::library_playlists()));
        }
        if let Some(query) = self.startup_search.take() {
            self.state.query = query;
            self.state.caret = self.state.query.chars().count();
            out.extend(self.submit_search());
        }
        out
    }

    /// An answer from the page. Stale and unknown answers are ignored.
    pub fn on_result(&mut self, req: u64, ok: bool, status: u16, body: &str) -> Vec<Action> {
        let Some(purpose) = self.api.take(req) else {
            return Vec::new();
        };
        if !ok {
            self.fail(purpose, status, body);
            return Vec::new();
        }
        self.apply(purpose, body)
    }

    /// Nav item `index` was chosen (clicked or entered).
    pub fn open_nav(&mut self, index: usize) -> Vec<Action> {
        let Some(item) = self.state.nav.get(index).cloned() else {
            return Vec::new();
        };
        match item.kind {
            NavKind::Search => {
                self.state.search_active = true;
                self.state.nav_selected = Some(index);
                Vec::new()
            }
            NavKind::Home => {
                self.state.nav_selected = Some(index);
                self.start_main(Purpose::Home, ytm::home())
            }
            NavKind::Library => {
                self.open_signed_in(index, Purpose::Library, ytm::library_playlists())
            }
            NavKind::Liked => self.open_signed_in(index, Purpose::Liked, ytm::liked_songs()),
            NavKind::Playlist => {
                let Some(playlist) = index
                    .checked_sub(FIXED_NAV)
                    .and_then(|k| self.nav_playlists.get(k))
                    .cloned()
                else {
                    return Vec::new();
                };
                self.state.nav_selected = Some(index);
                self.start_main(Purpose::Page, ytm::playlist(&playlist.browse_id))
            }
            NavKind::Heading => Vec::new(),
        }
    }

    /// Runs the search in the search field.
    pub fn submit_search(&mut self) -> Vec<Action> {
        let query = self.state.query.trim().to_string();
        if query.is_empty() {
            self.state.status = "Type a search first".to_string();
            return Vec::new();
        }
        self.state.nav_selected = Some(0);
        self.start_main(Purpose::Search, ytm::search(&query, None))
    }

    /// Activates row `row`: a track plays, an album or playlist opens, an artist searches for its
    /// songs, and "« Back" goes back. Headers do nothing.
    pub fn activate_row(&mut self, row: usize) -> Vec<Action> {
        let Some(target) = self.targets.get(row).cloned() else {
            return Vec::new();
        };
        match target {
            Target::Back => {
                self.back();
                Vec::new()
            }
            Target::Header(_) => Vec::new(),
            Target::Track(track) => {
                if self.content.shape == Shape::Tracks {
                    self.play_from_page(row, &track)
                } else {
                    vec![Action::AddAndPlay(entry_of(&track))]
                }
            }
            Target::Album(album) => self.open_page(ytm::album(&album.browse_id)),
            Target::Playlist(playlist) => self.open_page(ytm::playlist(&playlist.browse_id)),
            Target::Artist(artist) => self.search_artist(&artist.name),
        }
    }

    /// Runs a right-click command on the selected tracks. Rows that are not tracks are ignored.
    pub fn run_command(&mut self, command: LibCommand) -> Vec<Action> {
        let tracks: Vec<Track> = self
            .state
            .selected
            .iter()
            .filter_map(|&i| match self.targets.get(i) {
                Some(Target::Track(t)) => Some(t.clone()),
                _ => None,
            })
            .collect();
        let Some(first) = tracks.first().cloned() else {
            self.state.status = "Select a track first".to_string();
            return Vec::new();
        };
        match command {
            LibCommand::Play => vec![Action::ReplaceAndPlay {
                entries: tracks.iter().map(entry_of).collect(),
                start: 0,
            }],
            LibCommand::Enqueue => vec![Action::Append(tracks.iter().map(entry_of).collect())],
            LibCommand::Radio => self.request(Purpose::Radio, ytm::next(&first.video_id)),
        }
    }

    /// Asks for the next page when the list is near its end and the page has one.
    pub fn maybe_more(&mut self) -> Vec<Action> {
        let Some(token) = self.content.continuation.clone() else {
            return Vec::new();
        };
        if self.content.more_pending {
            return Vec::new();
        }
        let visible = self.state.visible_rows();
        if self.state.scroll + visible + MORE_MARGIN < self.state.rows.len() {
            return Vec::new();
        }
        self.content.more_pending = true;
        self.request(Purpose::More, ytm::continuation(&token))
    }

    /// The video that is playing now (for the highlight in the list).
    pub fn set_playing(&mut self, video_id: Option<&str>) {
        if self.playing.as_deref() == video_id {
            return;
        }
        self.playing = video_id.map(str::to_owned);
        for (row, target) in self.state.rows.iter_mut().zip(&self.targets) {
            row.playing = matches!(
                target,
                Target::Track(t) if Some(t.video_id.as_str()) == self.playing.as_deref()
            );
        }
    }

    /// Whether the window has to show the sign-in hint instead of a library list.
    fn signed_out(&self) -> bool {
        self.api.ready() && !self.api.signed_in()
    }

    fn open_signed_in(&mut self, index: usize, purpose: Purpose, request: Request) -> Vec<Action> {
        if self.signed_out() {
            self.reset_main();
            self.state.nav_selected = Some(index);
            self.state.status = SIGN_IN_HINT.to_string();
            return Vec::new();
        }
        self.state.nav_selected = Some(index);
        self.start_main(purpose, request)
    }

    /// Starts a request for the main list: clears the list, shows the loading note.
    fn start_main(&mut self, purpose: Purpose, request: Request) -> Vec<Action> {
        self.reset_main();
        self.state.status = LOADING.to_string();
        self.request(purpose, request)
    }

    /// Opens an album or playlist: its current list goes on the history for "« Back".
    fn open_page(&mut self, request: Request) -> Vec<Action> {
        let current = std::mem::replace(&mut self.content, Content::empty(Shape::Tracks));
        self.history.push(current);
        self.state.selected.clear();
        self.state.anchor = None;
        self.state.scroll = 0;
        self.state.status = LOADING.to_string();
        self.refresh();
        self.request(Purpose::Page, request)
    }

    fn search_artist(&mut self, name: &str) -> Vec<Action> {
        self.state.query = name.to_string();
        self.state.caret = name.chars().count();
        self.state.nav_selected = Some(0);
        self.start_main(
            Purpose::Search,
            ytm::search(name, Some(SearchFilter::Songs)),
        )
    }

    /// Goes back to the page before, if there is one. Requests for the page left are dropped.
    fn back(&mut self) {
        let Some(previous) = self.history.pop() else {
            return;
        };
        self.api.cancel(Slot::Main);
        self.content = previous;
        self.state.selected.clear();
        self.state.anchor = None;
        self.state.scroll = 0;
        self.state.status = String::new();
        self.refresh();
    }

    /// Plays track row `row` of an album or playlist page: the whole page becomes the playlist,
    /// starting at this track.
    fn play_from_page(&mut self, row: usize, track: &Track) -> Vec<Action> {
        let offset = usize::from(!self.history.is_empty());
        let Some(content_index) = row.checked_sub(offset) else {
            return Vec::new();
        };
        let entries: Vec<Entry> = self
            .content
            .targets
            .iter()
            .filter_map(|t| match t {
                Target::Track(t) => Some(entry_of(t)),
                _ => None,
            })
            .collect();
        let start = self.content.targets[..content_index]
            .iter()
            .filter(|t| matches!(t, Target::Track(_)))
            .count();
        debug_assert_eq!(entries[start].video_id, track.video_id);
        vec![Action::ReplaceAndPlay { entries, start }]
    }

    /// Empties the main list and its history, and drops the selection.
    fn reset_main(&mut self) {
        self.history.clear();
        self.content = Content::empty(Shape::Shelves);
        self.state.selected.clear();
        self.state.anchor = None;
        self.state.scroll = 0;
        self.refresh();
    }

    fn request(&mut self, purpose: Purpose, request: Request) -> Vec<Action> {
        self.api
            .start(purpose, request)
            .map(|cmd| vec![Action::Send(cmd)])
            .unwrap_or_default()
    }

    fn fail(&mut self, purpose: Purpose, status: u16, body: &str) {
        let reason = if status == 0 {
            body.lines().next().unwrap_or("request failed").to_string()
        } else {
            format!("HTTP {status}")
        };
        match purpose {
            Purpose::More => self.content.more_pending = false,
            Purpose::Radio => self.state.status = format!("Radio failed: {reason}"),
            Purpose::NavPlaylists => {}
            _ => self.state.status = format!("Could not load: {reason}"),
        }
    }

    /// Reads an answer body for `purpose` into the lists.
    fn apply(&mut self, purpose: Purpose, body: &str) -> Vec<Action> {
        match purpose {
            Purpose::Search => {
                let results = ytm::parse_search(body);
                let empty =
                    results.top.is_none() && results.shelves.iter().all(|s| s.items.is_empty());
                self.content = Content {
                    shape: Shape::Shelves,
                    targets: search_targets(&results),
                    continuation: None,
                    more_pending: false,
                };
                self.state.status = if empty {
                    format!("No results for \"{}\"", self.state.query.trim())
                } else {
                    String::new()
                };
            }
            Purpose::Home => {
                let home = ytm::parse_home(body);
                self.content = Content {
                    shape: Shape::Shelves,
                    targets: shelf_targets(&home.shelves),
                    continuation: home.continuation,
                    more_pending: false,
                };
                self.state.status = String::new();
            }
            Purpose::Library => {
                let library = ytm::parse_library_playlists(body);
                self.content = Content {
                    shape: Shape::Shelves,
                    targets: library
                        .items
                        .iter()
                        .cloned()
                        .map(Target::Playlist)
                        .collect(),
                    continuation: library.continuation,
                    more_pending: false,
                };
                self.state.status = if self.content.targets.is_empty() {
                    "No playlists in your library".to_string()
                } else {
                    String::new()
                };
                // The nav column shows the same playlists, if it has none yet.
                if self.nav_playlists.is_empty() {
                    self.set_nav_playlists(library.items);
                }
            }
            Purpose::Liked | Purpose::Page => {
                let page = ytm::parse_playlist(body);
                let count = page.tracks.len();
                self.content = Content {
                    shape: Shape::Tracks,
                    targets: page.tracks.iter().cloned().map(Target::Track).collect(),
                    continuation: page.continuation,
                    more_pending: false,
                };
                self.state.status = match (page.title.is_empty(), count) {
                    (_, 0) => "Nothing here".to_string(),
                    (true, _) => String::new(),
                    (false, _) => match &page.author {
                        Some(author) => format!("{} - {author}", page.title),
                        None => page.title.clone(),
                    },
                };
            }
            Purpose::More => {
                let more = ytm::parse_continuation(body);
                match self.content.shape {
                    Shape::Shelves => self.content.targets.extend(shelf_targets(&more.shelves)),
                    Shape::Tracks => self
                        .content
                        .targets
                        .extend(more.tracks.into_iter().map(Target::Track)),
                }
                self.content.continuation = more.continuation;
                self.content.more_pending = false;
            }
            Purpose::NavPlaylists => {
                let library = ytm::parse_library_playlists(body);
                self.set_nav_playlists(library.items);
            }
            Purpose::Radio => {
                let tracks = ytm::parse_next(body);
                self.state.status = format!("Radio: {} tracks added", tracks.len());
                if tracks.is_empty() {
                    return Vec::new();
                }
                return vec![Action::Append(tracks.iter().map(entry_of).collect())];
            }
        }
        self.refresh();
        Vec::new()
    }

    fn set_nav_playlists(&mut self, items: Vec<PlaylistItem>) {
        let titles: Vec<String> = items.iter().map(|p| p.title.clone()).collect();
        self.nav_playlists = items;
        self.state.set_nav_playlists(&titles);
    }

    /// Rebuilds the rows the window draws from the content, with the Back row on top when there
    /// is history. Selection survives only where its rows are still there.
    fn refresh(&mut self) {
        let mut rows = Vec::new();
        let mut targets = Vec::new();
        if !self.history.is_empty() {
            rows.push(LibRow::new(LibKind::Back, BACK_LABEL));
            targets.push(Target::Back);
        }
        let mut number = 0;
        for target in &self.content.targets {
            let row = match target {
                Target::Back => continue,
                Target::Header(title) => {
                    number = 0;
                    LibRow::new(LibKind::Header, title.clone())
                }
                Target::Track(track) => {
                    number += 1;
                    LibRow {
                        kind: LibKind::Track,
                        text: track_line(number, track),
                        duration_secs: track.duration_secs,
                        playing: false,
                    }
                }
                Target::Album(album) => LibRow::new(LibKind::Album, album_line(album)),
                Target::Artist(artist) => {
                    LibRow::new(LibKind::Artist, format!("[ARTIST] {}", artist.name))
                }
                Target::Playlist(playlist) => {
                    LibRow::new(LibKind::Playlist, playlist_line(playlist))
                }
            };
            rows.push(row);
            targets.push(target.clone());
        }
        self.state.rows = rows;
        self.targets = targets;
        let len = self.state.rows.len();
        self.state.selected.retain(|&i| i < len);
        self.state
            .selected
            .retain(|&i| self.state.rows.get(i).is_some_and(|r| r.kind.selectable()));
        if self.state.anchor.is_some_and(|a| a >= len) {
            self.state.anchor = None;
        }
        let playing = self.playing.clone();
        for (row, target) in self.state.rows.iter_mut().zip(&self.targets) {
            row.playing = matches!(
                target,
                Target::Track(t) if Some(t.video_id.as_str()) == playing.as_deref()
            );
        }
        self.state.clamp_scroll();
    }
}

/// The entry a track becomes in the playlist.
pub fn entry_of(track: &Track) -> Entry {
    Entry {
        video_id: track.video_id.clone(),
        title: track.title.clone(),
        artist: join_artists(&track.artists),
        duration_secs: track.duration_secs,
    }
}

/// "Artist A, Artist B".
fn join_artists(artists: &[ArtistRef]) -> String {
    artists
        .iter()
        .map(|a| a.name.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// "1. Artist - Title", or "1. Title" when the artist is unknown.
fn track_line(number: usize, track: &Track) -> String {
    let artist = join_artists(&track.artists);
    if artist.trim().is_empty() {
        format!("{number}. {}", track.title)
    } else {
        format!("{number}. {artist} - {}", track.title)
    }
}

/// "[ALBUM] Title - Artist".
fn album_line(album: &AlbumItem) -> String {
    let artists = join_artists(&album.artists);
    if artists.trim().is_empty() {
        format!("[ALBUM] {}", album.title)
    } else {
        format!("[ALBUM] {} - {artists}", album.title)
    }
}

/// "[PLAYLIST] Title (n)", with the count when the row states it.
fn playlist_line(playlist: &PlaylistItem) -> String {
    match playlist.count {
        Some(count) => format!("[PLAYLIST] {} ({count})", playlist.title),
        None => format!("[PLAYLIST] {}", playlist.title),
    }
}

/// The rows of a search: the top result under its label, then each shelf under its title.
fn search_targets(results: &SearchResults) -> Vec<Target> {
    let mut out = Vec::new();
    if let Some(top) = &results.top {
        out.push(Target::Header("Top result".to_string()));
        out.push(target_of(top));
    }
    for shelf in &results.shelves {
        if !shelf.title.is_empty() {
            out.push(Target::Header(shelf.title.clone()));
        }
        out.extend(shelf.items.iter().map(target_of));
    }
    out
}

/// The rows of shelves (home, library): each titled shelf under its title.
fn shelf_targets(shelves: &[tubeamp_ytm::Shelf]) -> Vec<Target> {
    let mut out = Vec::new();
    for shelf in shelves {
        if !shelf.title.is_empty() {
            out.push(Target::Header(shelf.title.clone()));
        }
        out.extend(shelf.items.iter().map(target_of));
    }
    out
}

fn target_of(item: &Item) -> Target {
    match item {
        Item::Track(t) => Target::Track(t.clone()),
        Item::Album(a) => Target::Album(a.clone()),
        Item::Artist(a) => Target::Artist(a.clone()),
        Item::Playlist(p) => Target::Playlist(p.clone()),
    }
}

// Menus.

/// Menu item ids for the right-click menu on tracks.
pub const CMD_PLAY: &str = "lib.play";
pub const CMD_ENQUEUE: &str = "lib.enqueue";
pub const CMD_RADIO: &str = "lib.radio";
/// The main window's O menu: opens the library.
pub const MAIN_LIBRARY: &str = "main.library";
/// The O menu's Video item: shows or hides the video window.
pub const MAIN_VIDEO: &str = "main.video";

/// Maps a menu id from the library's menus to its command.
pub fn command_for_id(id: &str) -> Option<LibCommand> {
    match id {
        CMD_PLAY => Some(LibCommand::Play),
        CMD_ENQUEUE => Some(LibCommand::Enqueue),
        CMD_RADIO => Some(LibCommand::Radio),
        _ => None,
    }
}

/// The right-click menu on the library's rows.
pub fn build_row_menu() -> Result<muda::Menu, muda::Error> {
    use muda::{Menu, MenuItem};

    let menu = Menu::new();
    menu.append(&MenuItem::with_id(CMD_PLAY, "Play", true, None))?;
    menu.append(&MenuItem::with_id(CMD_ENQUEUE, "Enqueue", true, None))?;
    menu.append(&MenuItem::with_id(CMD_RADIO, "Start radio", true, None))?;
    Ok(menu)
}

/// The main window's O menu.
pub fn build_main_menu() -> Result<muda::Menu, muda::Error> {
    use muda::{Menu, MenuItem};

    let menu = Menu::new();
    menu.append(&MenuItem::with_id(
        MAIN_LIBRARY,
        "Media Library",
        true,
        None,
    ))?;
    menu.append(&MenuItem::with_id(MAIN_VIDEO, "Video", true, None))?;
    menu.append(&MenuItem::with_id(
        crate::milkdrop::MAIN_MILKDROP,
        "Milkdrop",
        true,
        None,
    ))?;
    Ok(menu)
}

// Fixtures.

/// Renders a captured response into the library state, for `--render-library`. The purpose comes
/// from the file name: `search*`, `home*`, `library*`, or `album*`/`playlist*` for a page.
pub fn fixture_state(file_name: &str, body: &str, query: &str) -> Result<LibraryState, String> {
    let purpose = if file_name.starts_with("search") {
        Purpose::Search
    } else if file_name.starts_with("home") && !file_name.contains("continuation") {
        Purpose::Home
    } else if file_name.starts_with("library") {
        Purpose::Library
    } else if (file_name.starts_with("album") || file_name.starts_with("playlist"))
        && !file_name.contains("continuation")
    {
        Purpose::Page
    } else {
        return Err(format!(
            "no parser for fixture {file_name}; use a search, home, library, album or playlist fixture"
        ));
    };
    let mut library = Library::new();
    library.state.query = query.to_string();
    library.state.nav_selected = Some(0);
    library.apply(purpose, body);
    Ok(library.state)
}

#[cfg(test)]
mod tests {
    use super::*;

    const SEARCH: &str = include_str!("../../ytm/tests/fixtures/search_daft_punk.json");

    fn track(id: &str, title: &str, artist: &str, secs: u32) -> Track {
        Track {
            video_id: id.to_string(),
            title: title.to_string(),
            artists: vec![ArtistRef {
                name: artist.to_string(),
                id: None,
            }],
            album: None,
            duration_secs: Some(secs),
            thumbnail: None,
            explicit: false,
        }
    }

    /// A library with a ready, signed-in page and one request answered.
    fn ready_library() -> Library {
        let mut lib = Library::new();
        lib.on_ready(true);
        lib
    }

    fn sent_req(actions: &[Action]) -> u64 {
        match actions {
            [Action::Send(Cmd::Api { req, .. })] => *req,
            other => panic!("expected one API request, got {other:?}"),
        }
    }

    fn page_with_tracks(lib: &mut Library, tracks: Vec<Track>) {
        lib.content = Content {
            shape: Shape::Tracks,
            targets: tracks.into_iter().map(Target::Track).collect(),
            continuation: None,
            more_pending: false,
        };
        lib.refresh();
    }

    #[test]
    fn a_search_answer_fills_the_rows_with_headers_and_types() {
        let mut lib = ready_library();
        lib.state.query = "daft punk".to_string();
        let req = sent_req(&lib.submit_search());
        assert_eq!(lib.state.status, LOADING);
        let actions = lib.on_result(req, true, 200, SEARCH);
        assert!(actions.is_empty());
        assert!(lib.state.status.is_empty());
        let rows = &lib.state.rows;
        assert_eq!(rows[0].kind, LibKind::Header, "the top result label");
        assert_eq!(rows[0].text, "Top result");
        assert_eq!(rows[1].kind, LibKind::Artist);
        assert_eq!(rows[1].text, "[ARTIST] Daft Punk");
        assert!(
            rows.iter().any(|r| r.kind == LibKind::Header),
            "a shelf title row"
        );
        assert!(rows
            .iter()
            .any(|r| r.kind == LibKind::Album && r.text.starts_with("[ALBUM] ")));
        assert!(rows
            .iter()
            .any(|r| r.kind == LibKind::Track && r.text.starts_with("1. ")));
        assert!(rows
            .iter()
            .any(|r| r.kind == LibKind::Playlist && r.text.starts_with("[PLAYLIST] ")));
    }

    #[test]
    fn a_stale_search_answer_is_ignored() {
        let mut lib = ready_library();
        lib.state.query = "first".to_string();
        let old = sent_req(&lib.submit_search());
        lib.state.query = "second".to_string();
        let _new = sent_req(&lib.submit_search());
        lib.on_result(old, true, 200, SEARCH);
        assert!(
            lib.state.rows.is_empty(),
            "the old answer must not fill the list"
        );
    }

    #[test]
    fn enter_on_a_search_track_adds_and_plays_it() {
        let mut lib = ready_library();
        lib.content = Content {
            shape: Shape::Shelves,
            targets: vec![Target::Track(track("abc12345678", "One", "Band", 200))],
            continuation: None,
            more_pending: false,
        };
        lib.refresh();
        let actions = lib.activate_row(0);
        assert_eq!(
            actions,
            vec![Action::AddAndPlay(Entry {
                video_id: "abc12345678".into(),
                title: "One".into(),
                artist: "Band".into(),
                duration_secs: Some(200),
            })]
        );
    }

    #[test]
    fn enter_on_a_page_track_replaces_the_playlist_and_starts_there() {
        let mut lib = ready_library();
        page_with_tracks(
            &mut lib,
            vec![
                track("aaaaaaaaaaa", "A", "X", 60),
                track("bbbbbbbbbbb", "B", "X", 60),
                track("ccccccccccc", "C", "X", 60),
            ],
        );
        let actions = lib.activate_row(1);
        match &actions[..] {
            [Action::ReplaceAndPlay { entries, start }] => {
                assert_eq!(entries.len(), 3);
                assert_eq!(*start, 1);
                assert_eq!(entries[1].video_id, "bbbbbbbbbbb");
            }
            other => panic!("unexpected {other:?}"),
        }
    }

    #[test]
    fn the_back_row_returns_to_the_previous_list() {
        let mut lib = ready_library();
        lib.content = Content {
            shape: Shape::Shelves,
            targets: vec![Target::Album(AlbumItem {
                browse_id: "MPREb_x".into(),
                title: "Discovery".into(),
                artists: vec![],
                year: None,
                thumbnail: None,
            })],
            continuation: None,
            more_pending: false,
        };
        lib.refresh();
        sent_req(&lib.activate_row(0));
        assert_eq!(
            lib.state.rows[0].kind,
            LibKind::Back,
            "the page opened, with Back on top"
        );
        assert_eq!(lib.state.rows.len(), 1, "and nothing in it until it loads");
        let back = lib.activate_row(0);
        assert!(back.is_empty());
        assert_eq!(lib.state.rows.len(), 1);
        assert_eq!(
            lib.state.rows[0].kind,
            LibKind::Album,
            "back on the search list"
        );
        assert!(lib.history.is_empty());
    }

    #[test]
    fn going_back_drops_the_page_request_still_out() {
        let mut lib = ready_library();
        lib.content = Content {
            shape: Shape::Shelves,
            targets: vec![Target::Playlist(PlaylistItem {
                browse_id: "VLPL1".into(),
                playlist_id: "PL1".into(),
                title: "Mix".into(),
                author: None,
                count: Some(3),
                thumbnail: None,
            })],
            continuation: None,
            more_pending: false,
        };
        lib.refresh();
        let page_req = sent_req(&lib.activate_row(0));
        lib.activate_row(0);
        assert!(lib.on_result(page_req, true, 200, "{}").is_empty());
        assert_eq!(
            lib.state.rows.len(),
            1,
            "the back-and-forth left the playlist row"
        );
        assert_eq!(
            lib.state.rows[0].kind,
            LibKind::Playlist,
            "the late page did not open"
        );
    }

    #[test]
    fn an_artist_row_searches_for_its_songs() {
        let mut lib = ready_library();
        lib.content = Content {
            shape: Shape::Shelves,
            targets: vec![Target::Artist(ArtistItem {
                browse_id: "UCx".into(),
                name: "Daft Punk".into(),
                thumbnail: None,
            })],
            continuation: None,
            more_pending: false,
        };
        lib.refresh();
        let actions = lib.activate_row(0);
        assert!(
            matches!(&actions[..], [Action::Send(Cmd::Api { endpoint, body, .. })]
            if endpoint == "search" && body["params"].is_string())
        );
        assert_eq!(lib.state.query, "Daft Punk");
    }

    #[test]
    fn library_and_liked_need_a_signed_in_user() {
        let mut lib = Library::new();
        lib.on_ready(false);
        assert!(
            lib.open_nav(2).is_empty(),
            "no request for a signed-out user"
        );
        assert_eq!(lib.state.status, SIGN_IN_HINT);
        assert_eq!(lib.state.nav_selected, Some(2));
    }

    #[test]
    fn queued_library_request_is_refused_when_signed_out() {
        let mut lib = Library::new();
        assert!(lib.open_nav(3).is_empty(), "queued until the page is ready");
        let actions = lib.on_ready(false);
        assert!(actions.iter().all(|a| !matches!(a, Action::Send(_))));
        assert_eq!(lib.state.status, SIGN_IN_HINT);
    }

    #[test]
    fn signed_in_ready_asks_for_the_nav_playlists() {
        let mut lib = Library::new();
        let actions = lib.on_ready(true);
        assert!(
            matches!(&actions[..], [Action::Send(Cmd::Api { endpoint, body, .. })]
            if endpoint == "browse" && body["browseId"] == "FEmusic_liked_playlists")
        );
    }

    #[test]
    fn the_startup_search_runs_once_ready() {
        let mut lib = Library::new();
        lib.set_startup_search("daft punk".to_string());
        let actions = lib.on_ready(true);
        let searches = actions
            .iter()
            .filter(
                |a| matches!(a, Action::Send(Cmd::Api { endpoint, .. }) if endpoint == "search"),
            )
            .count();
        assert_eq!(searches, 1);
        assert_eq!(lib.state.query, "daft punk");
    }

    #[test]
    fn empty_search_is_refused_with_a_hint() {
        let mut lib = ready_library();
        lib.state.query = "   ".to_string();
        assert!(lib.submit_search().is_empty());
        assert_eq!(lib.state.status, "Type a search first");
    }

    #[test]
    fn commands_act_on_the_selected_tracks_only() {
        let mut lib = ready_library();
        page_with_tracks(
            &mut lib,
            vec![
                track("aaaaaaaaaaa", "A", "X", 60),
                track("bbbbbbbbbbb", "B", "X", 60),
            ],
        );
        assert!(
            lib.run_command(LibCommand::Play).is_empty(),
            "nothing selected"
        );
        lib.state.click(1);
        lib.state.ctrl_click(0);
        let actions = lib.run_command(LibCommand::Enqueue);
        match &actions[..] {
            [Action::Append(entries)] => {
                let ids: Vec<&str> = entries.iter().map(|e| e.video_id.as_str()).collect();
                assert_eq!(ids, vec!["aaaaaaaaaaa", "bbbbbbbbbbb"]);
            }
            other => panic!("unexpected {other:?}"),
        }
        let play = lib.run_command(LibCommand::Play);
        assert!(matches!(
            &play[..],
            [Action::ReplaceAndPlay { start: 0, .. }]
        ));
        let radio = lib.run_command(LibCommand::Radio);
        assert!(
            matches!(&radio[..], [Action::Send(Cmd::Api { endpoint, body, .. })]
            if endpoint == "next" && body["videoId"] == "aaaaaaaaaaa")
        );
    }

    #[test]
    fn radio_appends_the_up_next_queue() {
        let mut lib = ready_library();
        page_with_tracks(&mut lib, vec![track("aaaaaaaaaaa", "A", "X", 60)]);
        lib.state.click(0);
        let req = sent_req(&lib.run_command(LibCommand::Radio));
        let body = r#"{"contents":{"singleColumnMusicWatchNextResults":{"tabs":[]}}}"#;
        let actions = lib.on_result(req, true, 200, body);
        assert!(actions.is_empty(), "nothing in the queue, nothing appended");
        assert_eq!(lib.state.status, "Radio: 0 tracks added");
    }

    #[test]
    fn a_failed_answer_shows_its_status() {
        let mut lib = ready_library();
        lib.state.query = "x".to_string();
        let req = sent_req(&lib.submit_search());
        lib.on_result(req, false, 403, "forbidden");
        assert_eq!(lib.state.status, "Could not load: HTTP 403");
    }

    #[test]
    fn playing_track_is_highlighted_wherever_it_is() {
        let mut lib = ready_library();
        page_with_tracks(
            &mut lib,
            vec![
                track("aaaaaaaaaaa", "A", "X", 60),
                track("bbbbbbbbbbb", "B", "X", 60),
            ],
        );
        lib.set_playing(Some("bbbbbbbbbbb"));
        assert!(!lib.state.rows[0].playing);
        assert!(lib.state.rows[1].playing);
        lib.set_playing(None);
        assert!(lib.state.rows.iter().all(|r| !r.playing));
    }

    #[test]
    fn scrolling_near_the_end_asks_for_the_next_page_once() {
        let mut lib = ready_library();
        lib.content = Content {
            shape: Shape::Tracks,
            targets: (0..30)
                .map(|i| Target::Track(track(&format!("id{i:09}"), "T", "X", 60)))
                .collect(),
            continuation: Some("next-token".into()),
            more_pending: false,
        };
        lib.refresh();
        assert!(
            lib.maybe_more().is_empty(),
            "the top of the list is not the end"
        );
        lib.state.scroll_by(1000);
        let actions = lib.maybe_more();
        assert!(
            matches!(&actions[..], [Action::Send(Cmd::Api { endpoint, body, .. })]
            if endpoint == "browse" && body["continuation"] == "next-token")
        );
        assert!(lib.maybe_more().is_empty(), "one request at a time");
    }

    #[test]
    fn a_continuation_appends_tracks_to_a_page() {
        let mut lib = ready_library();
        lib.content = Content {
            shape: Shape::Tracks,
            targets: vec![Target::Track(track("aaaaaaaaaaa", "A", "X", 60))],
            continuation: Some("tok".into()),
            more_pending: true,
        };
        lib.refresh();
        let req = sent_req(&lib.request(Purpose::More, ytm::continuation("tok")));
        let body = r#"{"continuationContents":{"musicPlaylistShelfContinuation":{"contents":[]}}}"#;
        lib.on_result(req, true, 200, body);
        assert_eq!(lib.state.rows.len(), 1, "an empty page adds nothing");
        assert!(!lib.content.more_pending);
        assert_eq!(lib.content.continuation, None);
    }

    #[test]
    fn fixtures_map_to_their_parsers() {
        let state = fixture_state("search_daft_punk.json", SEARCH, "daft punk").expect("search");
        assert_eq!(state.query, "daft punk");
        assert!(state.rows.iter().any(|r| r.kind == LibKind::Track));
        assert!(fixture_state("next.json", "{}", "q").is_err());
        assert!(fixture_state("home_continuation.json", "{}", "q").is_err());
    }

    #[test]
    fn track_and_entry_formatting() {
        let t = Track {
            artists: vec![
                ArtistRef {
                    name: "A".into(),
                    id: None,
                },
                ArtistRef {
                    name: "B".into(),
                    id: None,
                },
            ],
            ..track("aaaaaaaaaaa", "Title", "", 90)
        };
        assert_eq!(track_line(3, &t), "3. A, B - Title");
        assert_eq!(entry_of(&t).artist, "A, B");
        let solo = track("aaaaaaaaaaa", "Only", "", 90);
        let solo = Track {
            artists: vec![],
            ..solo
        };
        assert_eq!(track_line(1, &solo), "1. Only");
        assert_eq!(
            playlist_line(&PlaylistItem {
                browse_id: "VLx".into(),
                playlist_id: "x".into(),
                title: "Road".into(),
                author: None,
                count: None,
                thumbnail: None,
            }),
            "[PLAYLIST] Road"
        );
    }
}
