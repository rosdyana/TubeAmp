//! Library playlists and liked songs. Sign-in is needed to capture these live, so the grid is a
//! hand-trimmed fixture (`fixtures/library_grid.hand.json`) that follows the grid structure that
//! the logged-out pages use. Live verification against a signed-in capture is still pending.

use tubeamp_ytm::{liked_songs, library_playlists, parse_library_playlists};

const GRID: &str = include_str!("fixtures/library_grid.hand.json");

#[test]
fn library_grid_yields_only_playlists() {
    let page = parse_library_playlists(GRID);
    assert_eq!(page.items.len(), 2, "the album row is not a playlist");
    assert!(page.continuation.is_some());
}

#[test]
fn liked_music_row_is_the_vllm_playlist() {
    let page = parse_library_playlists(GRID);
    let liked = &page.items[0];
    assert_eq!(liked.title, "Liked Music");
    assert_eq!(liked.browse_id, "VLLM");
    assert_eq!(liked.playlist_id, "LM");
    assert_eq!(liked.count, Some(312));
    assert_eq!(liked.author, None, "the subtitle has no author");
    assert_eq!(liked.thumbnail.as_deref(), Some("https://example.test/liked-544.jpg"));
}

#[test]
fn playlist_row_takes_its_author_past_the_label_and_count() {
    let page = parse_library_playlists(GRID);
    let road = &page.items[1];
    assert_eq!(road.title, "Road trip");
    assert_eq!(road.browse_id, "VLPLhand0trip0list");
    assert_eq!(road.playlist_id, "PLhand0trip0list");
    assert_eq!(road.author.as_deref(), Some("Sample Author"));
    assert_eq!(road.count, Some(18));
}

#[test]
fn library_request_builders_target_the_signed_in_pages() {
    assert_eq!(library_playlists(), ("browse", serde_json::json!({"browseId": "FEmusic_liked_playlists"})));
    assert_eq!(liked_songs(), ("browse", serde_json::json!({"browseId": "VLLM"})));
}
