//! Search against captured responses: `fixtures/search_*.json` were recorded from YouTube Music
//! with the query "daft punk".

use tubeamp_ytm::{parse_search, Item, Track};

const ALL: &str = include_str!("fixtures/search_daft_punk.json");
const SONGS: &str = include_str!("fixtures/search_songs.json");
const ALBUMS: &str = include_str!("fixtures/search_albums.json");
const PLAYLISTS: &str = include_str!("fixtures/search_playlists.json");

const DAFT_PUNK_CHANNEL: &str = "UCRr1xG_2WIDs18a6cIiCxeA";

fn as_track(item: &Item) -> Option<&Track> {
    match item {
        Item::Track(t) => Some(t),
        _ => None,
    }
}

#[test]
fn songs_filter_yields_a_songs_shelf_of_complete_tracks() {
    let results = parse_search(SONGS);
    let shelf = results
        .shelves
        .iter()
        .find(|s| s.title == "Songs")
        .expect("a shelf titled Songs");
    let tracks: Vec<&Track> = shelf.items.iter().filter_map(as_track).collect();
    assert!(tracks.len() >= 10, "only {} tracks", tracks.len());
    assert_eq!(tracks.len(), shelf.items.len(), "every row of the Songs filter is a track");
    for t in &tracks {
        assert_eq!(t.video_id.len(), 11, "video id {:?}", t.video_id);
        assert!(!t.title.is_empty());
        assert!(t.duration_secs.is_some(), "no duration for {}", t.title);
    }
    let daft = tracks
        .iter()
        .filter(|t| t.artists.iter().any(|a| a.name == "Daft Punk"))
        .count();
    assert!(daft >= 10, "only {daft} tracks credit Daft Punk");
}

#[test]
fn songs_tracks_link_their_artist_and_album() {
    let results = parse_search(SONGS);
    let shelf = results.shelves.iter().find(|s| s.title == "Songs").unwrap();
    let first = as_track(&shelf.items[0]).expect("first row is a track");
    assert_eq!(first.title, "Harder, Better, Faster, Stronger");
    assert_eq!(first.video_id, "JhulBGMA7G4");
    assert_eq!(first.duration_secs, Some(227));
    assert_eq!(first.artists[0].name, "Daft Punk");
    assert_eq!(first.artists[0].id.as_deref(), Some(DAFT_PUNK_CHANNEL));
    let album = first.album.as_ref().expect("album link");
    assert_eq!(album.name, "Discovery");
    assert!(album.id.as_deref().unwrap().starts_with("MPREb_"));
    assert!(first.thumbnail.as_deref().unwrap().starts_with("https://"));
}

#[test]
fn unfiltered_search_has_a_top_result_and_mixed_shelves() {
    let results = parse_search(ALL);
    match &results.top {
        Some(Item::Artist(a)) => {
            assert_eq!(a.name, "Daft Punk");
            assert_eq!(a.browse_id, DAFT_PUNK_CHANNEL);
        }
        other => panic!("top result should be the Daft Punk artist, got {other:?}"),
    }
    let all_items: Vec<&Item> = results.shelves.iter().flat_map(|s| s.items.iter()).collect();
    assert!(
        all_items.iter().any(|i| matches!(i, Item::Album(a) if a.title == "Random Access Memories" && a.year == Some(2013))),
        "the album Random Access Memories with its year"
    );
    assert!(all_items.iter().any(|i| matches!(i, Item::Artist(a) if a.name == "The Weeknd")));
    assert!(all_items.iter().any(|i| matches!(i, Item::Playlist(p) if p.browse_id.starts_with("VL"))));
}

#[test]
fn unfiltered_search_has_a_shelf_of_daft_punk_tracks() {
    // The unfiltered page gives rows without durations (its subtitles are "Song • artists"), so
    // durations are checked on the Songs filter. Here: ids, titles and linked artists.
    let results = parse_search(ALL);
    let shelf_with_tracks = results.shelves.iter().any(|s| {
        s.items.iter().filter_map(as_track).any(|t| {
            !t.video_id.is_empty()
                && !t.title.is_empty()
                && t.artists.iter().any(|a| a.name == "Daft Punk")
        })
    });
    assert!(shelf_with_tracks, "no Daft Punk track with an id and title in any shelf");
    let with_duration = results
        .shelves
        .iter()
        .flat_map(|s| s.items.iter().filter_map(as_track))
        .filter(|t| t.duration_secs.is_some())
        .count();
    assert!(with_duration >= 1, "the top-result rows carry 'Song • 3:47' durations");
}

#[test]
fn albums_filter_yields_album_items_only() {
    let results = parse_search(ALBUMS);
    assert!(results.top.is_none());
    let shelf = results
        .shelves
        .iter()
        .find(|s| s.title == "Albums")
        .expect("a shelf titled Albums");
    assert!(shelf.items.len() >= 10, "only {} albums", shelf.items.len());
    for item in &shelf.items {
        match item {
            Item::Album(a) => {
                assert!(a.browse_id.starts_with("MPRE") || a.browse_id.starts_with("OLAK5uy"));
                assert!(!a.title.is_empty());
            }
            other => panic!("Albums filter returned {other:?}"),
        }
    }
    assert!(shelf
        .items
        .iter()
        .any(|i| matches!(i, Item::Album(a) if a.title == "Discovery" && a.year == Some(2001))));
}

#[test]
fn playlists_filter_yields_playlist_items_with_vl_ids() {
    let results = parse_search(PLAYLISTS);
    let shelf = results
        .shelves
        .iter()
        .find(|s| s.title == "Community playlists")
        .expect("a shelf titled Community playlists");
    assert!(shelf.items.len() >= 10, "only {} playlists", shelf.items.len());
    for item in &shelf.items {
        match item {
            Item::Playlist(p) => {
                assert_eq!(p.browse_id, format!("VL{}", p.playlist_id));
                assert!(!p.title.is_empty());
            }
            other => panic!("Playlists filter returned {other:?}"),
        }
    }
}
