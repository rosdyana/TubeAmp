//! Models for the parsed YouTube Music responses. All are plain data, serializable so the app can
//! cache them.

use serde::{Deserialize, Serialize};

/// An artist credited on a track or album. `id` is the channel browse id (`UC…`) when linked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistRef {
    pub name: String,
    pub id: Option<String>,
}

/// An album a track belongs to. `id` is the album browse id (`MPREb_…`) when linked.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumRef {
    pub name: String,
    pub id: Option<String>,
}

/// A playable song or video. Rows without a video id are not tracks and are skipped.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Track {
    pub video_id: String,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub album: Option<AlbumRef>,
    pub duration_secs: Option<u32>,
    pub thumbnail: Option<String>,
    pub explicit: bool,
}

/// An album result, from search, home or library.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AlbumItem {
    /// Browse id (`MPREb_…`). Pass to `request::album`.
    pub browse_id: String,
    pub title: String,
    pub artists: Vec<ArtistRef>,
    pub year: Option<u32>,
    pub thumbnail: Option<String>,
}

/// An artist (channel) result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtistItem {
    /// Channel browse id (`UC…`).
    pub browse_id: String,
    pub name: String,
    pub thumbnail: Option<String>,
}

/// A playlist result. `browse_id` is `VL` plus `playlist_id`, which is what `request::playlist` takes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistItem {
    pub browse_id: String,
    pub playlist_id: String,
    pub title: String,
    pub author: Option<String>,
    /// Number of songs when the row states it.
    pub count: Option<u32>,
    pub thumbnail: Option<String>,
}

/// One result of any kind.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Item {
    Track(Track),
    Album(AlbumItem),
    Artist(ArtistItem),
    Playlist(PlaylistItem),
}

/// A titled row of results (a carousel or a list). Titles are empty when the page gives none.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Shelf {
    pub title: String,
    pub items: Vec<Item>,
}

/// Search results. `top` is the "Top result" card when there is one. The continuation token of
/// search shelves is not exposed yet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SearchResults {
    pub top: Option<Item>,
    pub shelves: Vec<Shelf>,
}

/// A playlist or album page. Albums use the same shape: `author` is the album artist.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlaylistPage {
    pub title: String,
    pub author: Option<String>,
    pub tracks: Vec<Track>,
    /// Token for the next page of tracks, to pass to `request::continuation`.
    pub continuation: Option<String>,
}

/// The home page: carousels in the order YouTube Music gives them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HomePage {
    pub shelves: Vec<Shelf>,
    pub continuation: Option<String>,
}

/// One page of the library playlists grid.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryPlaylists {
    pub items: Vec<PlaylistItem>,
    pub continuation: Option<String>,
}

/// A continuation response. `shelves` holds every titled section (as on the home page), and
/// `tracks` holds every track from list-style sections, in order (as on a playlist page).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Continuation {
    pub shelves: Vec<Shelf>,
    pub tracks: Vec<Track>,
    pub continuation: Option<String>,
}
