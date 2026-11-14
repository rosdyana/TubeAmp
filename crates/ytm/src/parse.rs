//! Parsers for InnerTube responses. Each takes the raw JSON text and never panics: malformed input
//! gives empty results, unknown renderers are skipped, and missing fields become `None` or empty.
//!
//! One walker reads the section structure that search, home, playlist, library and continuation
//! responses share. The public functions pick out what each page needs.

use serde_json::Value;

use crate::fields::{arr, runs, str_of, subtitle, text};
use crate::item::{author_of, card, panel_track, responsive, two_row};
use crate::model::{
    Continuation, HomePage, Item, LibraryPlaylists, PlaylistItem, PlaylistPage, SearchResults,
    Shelf, Track,
};

/// Header renderers that carry a playlist or album title and subtitle.
const HEADER_KEYS: &[&str] = &[
    "musicResponsiveHeaderRenderer",
    "musicDetailHeaderRenderer",
    "musicEditablePlaylistDetailHeaderRenderer",
    "musicVisualHeaderRenderer",
];

/// Parses search results (any filter).
pub fn parse_search(json: &str) -> SearchResults {
    let root = load(json);
    let mut walked = Walked::default();
    walk_root(&root, &mut walked);
    SearchResults {
        top: walked.top,
        shelves: shelves(walked.sections),
    }
}

/// Parses the home page.
pub fn parse_home(json: &str) -> HomePage {
    let root = load(json);
    let mut walked = Walked::default();
    walk_root(&root, &mut walked);
    HomePage {
        shelves: shelves(walked.sections),
        continuation: walked.continuation,
    }
}

/// Parses a playlist page. Also used for liked songs (`VLLM`) and album pages, whose tracks are
/// in list-style shelves. The album artist is reported as `author`.
pub fn parse_playlist(json: &str) -> PlaylistPage {
    let root = load(json);
    let mut walked = Walked::default();
    walk_root(&root, &mut walked);
    let (title, author) = page_header(&root);
    PlaylistPage {
        title,
        author,
        tracks: collect_tracks(&walked.sections),
        continuation: walked.list_continuation,
    }
}

/// Parses the library playlists grid.
pub fn parse_library_playlists(json: &str) -> LibraryPlaylists {
    let root = load(json);
    let mut walked = Walked::default();
    walk_root(&root, &mut walked);
    let items = walked
        .sections
        .into_iter()
        .flat_map(|s| s.items)
        .filter_map(|item| match item {
            Item::Playlist(p) => Some(p),
            _ => None,
        })
        .collect::<Vec<PlaylistItem>>();
    LibraryPlaylists {
        items,
        continuation: walked.continuation,
    }
}

/// Parses a continuation response (the next page of home, playlist, search or library).
pub fn parse_continuation(json: &str) -> Continuation {
    let root = load(json);
    let mut walked = Walked::default();
    walk_root(&root, &mut walked);
    let tracks = collect_tracks(&walked.sections);
    Continuation {
        shelves: shelves(walked.sections),
        tracks,
        continuation: walked.list_continuation.or(walked.continuation),
    }
}

/// Parses the up-next queue of a `next` response, in play order.
pub fn parse_next(json: &str) -> Vec<Track> {
    let root = load(json);
    let mut tracks = Vec::new();
    collect_panel_tracks(&root, &mut tracks);
    tracks
}

// ---- Walker ------------------------------------------------------------------------------------

/// How a section lays out its items.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Layout {
    /// Carousel or grid of cards.
    Carousel,
    /// List of rows (search shelves, playlist and album tracks).
    List,
}

#[derive(Debug)]
struct Section {
    title: String,
    layout: Layout,
    items: Vec<Item>,
}

#[derive(Debug, Default)]
struct Walked {
    top: Option<Item>,
    sections: Vec<Section>,
    /// Token from a section list, carousel or grid: the next page of sections (home, library).
    continuation: Option<String>,
    /// Token from a list-style shelf: the next page of its rows (playlist and album tracks). A
    /// playlist's secondary column also has a section token, but that one loads the carousels.
    list_continuation: Option<String>,
}

fn load(json: &str) -> Value {
    serde_json::from_str(json).unwrap_or(Value::Null)
}

/// Walks the response's main content and any continuation.
fn walk_root(root: &Value, out: &mut Walked) {
    for list in section_lists(root) {
        walk_entry("sectionListRenderer", list, out);
    }
    if let Some(map) = root["continuationContents"].as_object() {
        for (key, value) in map {
            walk_entry(key, value, out);
        }
    }
    for action in arr(&root["onResponseReceivedActions"]) {
        walk_items(&action["appendContinuationItemsAction"]["continuationItems"], out);
    }
}

/// The `sectionListRenderer` of each tab, then of the secondary column (two-column pages keep
/// their playlist tracks there).
fn section_lists(root: &Value) -> Vec<&Value> {
    let mut lists = Vec::new();
    for key in [
        "singleColumnBrowseResultsRenderer",
        "twoColumnBrowseResultsRenderer",
        "tabbedSearchResultsRenderer",
    ] {
        for tab in arr(&root["contents"][key]["tabs"]) {
            let list = &tab["tabRenderer"]["content"]["sectionListRenderer"];
            if list.is_object() {
                lists.push(list);
            }
        }
    }
    let secondary = &root["contents"]["twoColumnBrowseResultsRenderer"]["secondaryContents"]
        ["sectionListRenderer"];
    if secondary.is_object() {
        lists.push(secondary);
    }
    lists
}

/// Walks a list of entries. Each entry is an object with one renderer key. Bare rows, and
/// `itemSectionRenderer`s that hold nothing but rows, are gathered into one list section in page
/// order; a shelf or other section ends the run.
fn walk_items(list: &Value, out: &mut Walked) {
    let mut loose = Vec::new();
    for entry in arr(list) {
        if let Some(item) = entry_item(entry) {
            loose.push(item);
            continue;
        }
        if let Some(rows) = item_section_rows(entry) {
            loose.extend(rows);
            continue;
        }
        flush_loose(&mut loose, out);
        if let Some(map) = entry.as_object() {
            for (key, value) in map {
                walk_entry(key, value, out);
            }
        }
    }
    flush_loose(&mut loose, out);
}

fn flush_loose(loose: &mut Vec<Item>, out: &mut Walked) {
    if !loose.is_empty() {
        out.sections.push(Section {
            title: String::new(),
            layout: Layout::List,
            items: std::mem::take(loose),
        });
    }
}

/// The rows of an `itemSectionRenderer` whose contents are all rows.
fn item_section_rows(entry: &Value) -> Option<Vec<Item>> {
    let contents = arr(&entry["itemSectionRenderer"]["contents"]);
    if contents.is_empty() {
        return None;
    }
    let rows: Vec<Item> = contents.iter().filter_map(entry_item).collect();
    (rows.len() == contents.len()).then_some(rows)
}

/// Handles one renderer. Unknown renderers are ignored.
fn walk_entry(key: &str, v: &Value, out: &mut Walked) {
    match key {
        "itemSectionRenderer" => walk_items(&v["contents"], out),
        "sectionListRenderer" | "sectionListContinuation" => {
            walk_items(&v["contents"], out);
            take_token(out, continuation_of(v));
        }
        "musicCarouselShelfRenderer"
        | "musicImmersiveCarouselShelfRenderer"
        | "musicCarouselShelfContinuation" => {
            push(
                out,
                header_title(&v["header"]),
                Layout::Carousel,
                items_of(&v["contents"]),
            );
            take_token(out, continuation_of(v));
        }
        "musicShelfRenderer"
        | "musicPlaylistShelfRenderer"
        | "musicShelfContinuation"
        | "musicPlaylistShelfContinuation" => {
            push(out, text(&v["title"]), Layout::List, items_of(&v["contents"]));
            take_list_token(out, continuation_of(v));
        }
        "musicCardShelfRenderer" => {
            if out.top.is_none() {
                out.top = card(v);
            }
            push(out, String::new(), Layout::List, items_of(&v["contents"]));
            take_list_token(out, continuation_of(v));
        }
        "gridRenderer" | "gridContinuation" => {
            push(out, header_title(&v["header"]), Layout::Carousel, items_of(&v["items"]));
            take_token(out, continuation_of(v));
        }
        "continuationItemRenderer" => take_token(
            out,
            str_of(&v["continuationEndpoint"]["continuationCommand"]["token"]),
        ),
        _ => {}
    }
}

fn push(out: &mut Walked, title: String, layout: Layout, items: Vec<Item>) {
    if !items.is_empty() {
        out.sections.push(Section {
            title,
            layout,
            items,
        });
    }
}

fn take_token(out: &mut Walked, token: Option<String>) {
    if out.continuation.is_none() {
        out.continuation = token;
    }
}

fn take_list_token(out: &mut Walked, token: Option<String>) {
    if out.list_continuation.is_none() {
        out.list_continuation = token;
    }
}

/// Items of a `contents` or `items` list: each entry is a row, and anything else is ignored.
fn items_of(list: &Value) -> Vec<Item> {
    arr(list).iter().filter_map(entry_item).collect()
}

fn entry_item(entry: &Value) -> Option<Item> {
    if let Some(r) = entry.get("musicResponsiveListItemRenderer") {
        return responsive(r);
    }
    if let Some(r) = entry.get("musicTwoRowItemRenderer") {
        return two_row(r);
    }
    None
}

/// The token for the next page of a section, from its `continuations` or a trailing
/// `continuationItemRenderer`.
fn continuation_of(v: &Value) -> Option<String> {
    for c in arr(&v["continuations"]) {
        let token = str_of(&c["nextContinuationData"]["continuation"])
            .or_else(|| str_of(&c["reloadContinuationData"]["continuation"]));
        if token.is_some() {
            return token;
        }
    }
    for key in ["contents", "items"] {
        for entry in arr(&v[key]) {
            let token = str_of(
                &entry["continuationItemRenderer"]["continuationEndpoint"]["continuationCommand"]
                    ["token"],
            );
            if token.is_some() {
                return token;
            }
        }
    }
    None
}

fn shelves(sections: Vec<Section>) -> Vec<Shelf> {
    sections
        .into_iter()
        .map(|s| Shelf {
            title: s.title,
            items: s.items,
        })
        .collect()
}

/// Every track from the list-style sections, in order.
fn collect_tracks(sections: &[Section]) -> Vec<Track> {
    sections
        .iter()
        .filter(|s| s.layout == Layout::List)
        .flat_map(|s| s.items.iter())
        .filter_map(|item| match item {
            Item::Track(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

/// Title of a carousel or grid header, found within two levels.
fn header_title(header: &Value) -> String {
    find_title(header, 2).map(text).unwrap_or_default()
}

fn find_title(v: &Value, depth: u8) -> Option<&Value> {
    if v["title"].is_object() {
        return Some(&v["title"]);
    }
    if depth == 0 {
        return None;
    }
    v.as_object()?
        .values()
        .find_map(|child| find_title(child, depth - 1))
}

// ---- Playlist header ---------------------------------------------------------------------------

/// Title and author of a playlist or album page. The author comes from the subtitle, then the
/// strapline (albums name their artist there), then the page description ("Album • Daft Punk").
fn page_header(root: &Value) -> (String, Option<String>) {
    let header = find_header(root).and_then(|h| header_fields(h, 3));
    let title = header
        .map(|h| text(&h["title"]))
        .filter(|t| !t.is_empty())
        .or_else(|| str_of(&root["microformat"]["microformatDataRenderer"]["title"]))
        .unwrap_or_default();
    let author = header
        .and_then(|h| author_of(&subtitle(runs(&h["subtitle"]))))
        .or_else(|| header.and_then(|h| author_of(&subtitle(runs(&h["straplineTextOne"])))))
        .or_else(|| description_author(root));
    (title, author)
}

/// The second part of "Album • Daft Punk" or "Playlist • Jonathan Titou".
fn description_author(root: &Value) -> Option<String> {
    let description = str_of(&root["microformat"]["microformatDataRenderer"]["description"])?;
    description
        .split('•')
        .map(str::trim)
        .nth(1)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
}

/// The header renderer: at the root's `header`, or in a section list.
fn find_header(root: &Value) -> Option<&Value> {
    let entries = std::iter::once(&root["header"]).chain(
        section_lists(root)
            .into_iter()
            .flat_map(|list| arr(&list["contents"]).iter()),
    );
    for obj in entries {
        if let Some(map) = obj.as_object() {
            for (key, value) in map {
                if HEADER_KEYS.contains(&key.as_str()) {
                    return Some(value);
                }
            }
        }
    }
    None
}

/// The header object that has the title. Editable headers nest it under `header`.
fn header_fields(h: &Value, depth: u8) -> Option<&Value> {
    if h["title"].is_object() {
        return Some(h);
    }
    if depth == 0 {
        return None;
    }
    for (key, value) in h.as_object()? {
        if value.is_object() && (key == "header" || HEADER_KEYS.contains(&key.as_str())) {
            if let Some(found) = header_fields(value, depth - 1) {
                return Some(found);
            }
        }
    }
    None
}

// ---- Up next -----------------------------------------------------------------------------------

/// Finds every `playlistPanelVideoRenderer` in the response, in document order. The
/// `counterpart` renderers are the same video, so they are skipped.
fn collect_panel_tracks(v: &Value, out: &mut Vec<Track>) {
    match v {
        Value::Object(map) => {
            for (key, child) in map {
                match key.as_str() {
                    "playlistPanelVideoRenderer" => {
                        if let Some(track) = panel_track(child) {
                            out.push(track);
                        }
                    }
                    "counterpart" => {}
                    _ => collect_panel_tracks(child, out),
                }
            }
        }
        Value::Array(list) => {
            for child in list {
                collect_panel_tracks(child, out);
            }
        }
        _ => {}
    }
}
