//! YouTube Music (InnerTube) support with no networking. The app sends the requests built here
//! through the bridge, from inside the signed-in page, and feeds the JSON replies to the parsers.
//!
//! Field paths follow the structures used by the open-source ytmusicapi project, learned from its
//! documentation and from captured responses. No code is copied. Every parser is defensive:
//! unknown renderers are skipped, missing fields become `None`, and malformed input never panics.

mod fields;
mod item;
mod model;
mod parse;
mod request;

pub use fields::parse_duration;
pub use model::{
    AlbumItem, AlbumRef, ArtistItem, ArtistPage, ArtistRef, Continuation, HomePage, Item,
    LibraryPlaylists, PlaylistItem, PlaylistPage, SearchResults, Shelf, Track,
};
pub use parse::{
    parse_artist, parse_continuation, parse_home, parse_library_playlists, parse_next,
    parse_playlist, parse_search,
};
pub use request::{
    album, artist, continuation, home, library_playlists, liked_songs, next, playlist, search,
    Request, SearchFilter,
};
