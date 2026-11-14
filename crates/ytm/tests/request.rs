//! Request builders: endpoint names and bodies as the bridge will post them.

use serde_json::json;
use tubeamp_ytm::{album, continuation, home, next, playlist, search, SearchFilter};

#[test]
fn search_without_a_filter_has_only_the_query() {
    assert_eq!(search("daft punk", None), ("search", json!({"query": "daft punk"})));
}

#[test]
fn each_filter_maps_to_its_own_params() {
    let params = |f| search("x", Some(f)).1["params"].as_str().unwrap().to_string();
    let filters = [
        SearchFilter::Songs,
        SearchFilter::Videos,
        SearchFilter::Albums,
        SearchFilter::Artists,
        SearchFilter::Playlists,
    ];
    let all: Vec<String> = filters.iter().map(|f| params(*f)).collect();
    for (i, p) in all.iter().enumerate() {
        assert!(p.starts_with("Eg"), "{p}");
        assert!(!all[..i].contains(p), "filters share params {p}");
    }
    assert_eq!(params(SearchFilter::Songs), "EgWKAQIIAWoKEAMQBBAJEAoQBQ==");
}

#[test]
fn home_and_album_browse_their_ids() {
    assert_eq!(home(), ("browse", json!({"browseId": "FEmusic_home"})));
    assert_eq!(album("MPREb_7ltM34kr0mH"), ("browse", json!({"browseId": "MPREb_7ltM34kr0mH"})));
}

#[test]
fn playlist_always_uses_the_vl_prefix_once() {
    assert_eq!(playlist("PLabc"), ("browse", json!({"browseId": "VLPLabc"})));
    assert_eq!(playlist("VLPLabc"), ("browse", json!({"browseId": "VLPLabc"})));
}

#[test]
fn continuation_sends_the_token_in_the_body() {
    assert_eq!(continuation("TOKEN"), ("browse", json!({"continuation": "TOKEN"})));
}

#[test]
fn next_seeds_the_radio_playlist_from_the_video() {
    assert_eq!(
        next("kJQP7kiw5Fk"),
        (
            "next",
            json!({"videoId": "kJQP7kiw5Fk", "playlistId": "RDAMVMkJQP7kiw5Fk"})
        )
    );
}
