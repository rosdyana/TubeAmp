//! Request builders. Each returns `(endpoint, body)` for `tubeamp_bridge::Cmd::Api`, which posts it
//! to `/youtubei/v1/<endpoint>` with the page's `INNERTUBE_CONTEXT` merged in.

use serde_json::{json, Value};

/// `(endpoint, body)`: the endpoint is `"search"`, `"browse"` or `"next"`.
pub type Request = (&'static str, Value);

/// Search filters, mapped to YouTube Music's search `params`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SearchFilter {
    Songs,
    Videos,
    Albums,
    Artists,
    /// Community playlists. (YouTube Music's "Playlists" filter in the web UI.)
    Playlists,
}

impl SearchFilter {
    fn params(self) -> &'static str {
        match self {
            SearchFilter::Songs => "EgWKAQIIAWoKEAMQBBAJEAoQBQ==",
            SearchFilter::Videos => "EgWKAQIQAWoKEAMQBBAJEAoQBQ==",
            SearchFilter::Albums => "EgWKAQIYAWoKEAMQBBAJEAoQBQ==",
            SearchFilter::Artists => "EgWKAQIgAWoKEAMQBBAJEAoQBQ==",
            SearchFilter::Playlists => "EgeKAQQoAEABagwQDhAKEAMQBBAJEAU=",
        }
    }
}

/// Search for `query`, optionally restricted to one kind of result.
pub fn search(query: &str, filter: Option<SearchFilter>) -> Request {
    let mut body = json!({ "query": query });
    if let Some(filter) = filter {
        body["params"] = Value::from(filter.params());
    }
    ("search", body)
}

/// The home page (`FEmusic_home`).
pub fn home() -> Request {
    browse("FEmusic_home")
}

/// The library's playlists grid (`FEmusic_liked_playlists`). Needs sign-in.
pub fn library_playlists() -> Request {
    browse("FEmusic_liked_playlists")
}

/// Liked songs (`VLLM`). Needs sign-in.
pub fn liked_songs() -> Request {
    browse("VLLM")
}

/// A playlist page. Accepts either `PL…` or `VL…`; the request always uses `VL…`.
pub fn playlist(browse_id: &str) -> Request {
    if browse_id.starts_with("VL") {
        browse(browse_id)
    } else {
        browse(&format!("VL{browse_id}"))
    }
}

/// An album page, from an `AlbumItem::browse_id` (`MPREb_…`).
pub fn album(browse_id: &str) -> Request {
    browse(browse_id)
}

/// An artist's channel page, from its `UC…` id (`ArtistItem::browse_id`).
pub fn artist(channel_id: &str) -> Request {
    browse(channel_id)
}

/// The next page of a browse or search response, from a continuation token.
pub fn continuation(token: &str) -> Request {
    ("browse", json!({ "continuation": token }))
}

/// Up-next and radio for a video: the watch queue seeded with the video's radio playlist.
pub fn next(video_id: &str) -> Request {
    (
        "next",
        json!({
            "videoId": video_id,
            "playlistId": format!("RDAMVM{video_id}"),
        }),
    )
}

fn browse(browse_id: &str) -> Request {
    ("browse", json!({ "browseId": browse_id }))
}
