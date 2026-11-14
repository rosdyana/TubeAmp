//! Playlist and album pages against captured responses. `playlist.json` is a public playlist
//! ("Best Of Daft Punk" at capture time), `album.json` is "Random Access Memories", and
//! `playlist_continuation.json` is the next section page of the playlist's secondary column.

use tubeamp_ytm::{parse_continuation, parse_playlist, Track};

const PLAYLIST: &str = include_str!("fixtures/playlist.json");
const PLAYLIST_NEXT: &str = include_str!("fixtures/playlist_continuation.json");
const ALBUM: &str = include_str!("fixtures/album.json");

fn assert_complete(tracks: &[Track]) {
    for t in tracks {
        assert_eq!(t.video_id.len(), 11, "video id {:?}", t.video_id);
        assert!(!t.title.is_empty(), "track {} has no title", t.video_id);
        assert!(t.duration_secs.is_some(), "no duration for {}", t.title);
    }
}

#[test]
fn playlist_page_has_title_author_and_tracks() {
    let page = parse_playlist(PLAYLIST);
    assert!(!page.title.is_empty(), "playlist title");
    assert_eq!(page.author.as_deref(), Some("misterepicpants"));
    assert!(page.tracks.len() >= 10, "only {} tracks", page.tracks.len());
    assert_complete(&page.tracks);
    assert!(page.tracks.iter().all(|t| !t.artists.is_empty()));
}

#[test]
fn playlist_tracks_keep_their_order_and_album_links() {
    let page = parse_playlist(PLAYLIST);
    let first = &page.tracks[0];
    assert_eq!(first.video_id, "ByxxFmKKtUg");
    assert_eq!(first.title, "Daft Punk - Da Funk (original version) [HQ]");
    assert_eq!(first.duration_secs, Some(330));
    let daft_track = page
        .tracks
        .iter()
        .find(|t| t.video_id == "5NV6Rdv1a3I")
        .expect("Get Lucky in the playlist");
    assert_eq!(daft_track.artists[0].name, "Daft Punk");
    assert_eq!(daft_track.duration_secs, Some(249));
}

#[test]
fn playlist_without_a_track_continuation_has_no_token() {
    // The secondary column's section token loads carousels, not tracks, so it must not be
    // reported as the playlist's next page.
    let page = parse_playlist(PLAYLIST);
    assert_eq!(page.continuation, None);
}

#[test]
fn album_page_has_its_title_artist_and_tracks() {
    let page = parse_playlist(ALBUM);
    assert_eq!(page.title, "Random Access Memories");
    assert_eq!(page.author.as_deref(), Some("Daft Punk"));
    assert!(page.tracks.len() >= 10, "only {} tracks", page.tracks.len());
    assert_complete(&page.tracks);
    for t in &page.tracks {
        let album = t.album.as_ref();
        // Album tracks do not repeat the album name in their rows, so only check when present.
        if let Some(album) = album {
            assert!(!album.name.is_empty());
        }
    }
}

#[test]
fn playlist_continuation_is_carousels_without_tracks() {
    let next = parse_continuation(PLAYLIST_NEXT);
    assert!(next.tracks.is_empty());
    assert!(!next.shelves.is_empty(), "the section page holds carousels");
    assert!(next.shelves.iter().all(|s| !s.items.is_empty()));
}
