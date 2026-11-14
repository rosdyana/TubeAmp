//! Home page against captured responses: `fixtures/home.json` is the first page (logged out) and
//! `fixtures/home_continuation.json` is the next page of sections.

use tubeamp_ytm::{parse_continuation, parse_home, Item};

const HOME: &str = include_str!("fixtures/home.json");
const HOME_NEXT: &str = include_str!("fixtures/home_continuation.json");

#[test]
fn first_page_has_titled_carousels_with_items() {
    let home = parse_home(HOME);
    assert!(home.shelves.len() >= 2, "only {} shelves", home.shelves.len());
    for shelf in &home.shelves {
        assert!(!shelf.title.is_empty(), "a home shelf without a title");
        assert!(!shelf.items.is_empty(), "shelf {:?} has no items", shelf.title);
    }
    let mixes = home
        .shelves
        .iter()
        .find(|s| s.title == "Song mixes")
        .expect("the Song mixes shelf");
    assert!(mixes.items.len() >= 5, "only {} mixes", mixes.items.len());
    assert!(
        mixes.items.iter().any(|i| matches!(i, Item::Track(t) if !t.video_id.is_empty() && !t.title.is_empty())),
        "Song mixes has tracks with ids and titles"
    );
}

#[test]
fn first_page_carries_a_continuation_token() {
    let home = parse_home(HOME);
    let token = home.continuation.expect("continuation token");
    assert!(token.len() > 50, "token looks short: {} chars", token.len());
    assert!(!token.contains('"'));
}

#[test]
fn home_shelves_across_the_first_page_and_its_continuation_reach_three() {
    let first = parse_home(HOME);
    let next = parse_continuation(HOME_NEXT);
    let total = first.shelves.len() + next.shelves.len();
    assert!(total >= 3, "only {total} shelves across both pages");
    let titles: Vec<&str> = next.shelves.iter().map(|s| s.title.as_str()).collect();
    assert!(titles.contains(&"New releases"), "titles: {titles:?}");
    for shelf in &next.shelves {
        assert!(!shelf.items.is_empty(), "continuation shelf {:?} is empty", shelf.title);
    }
}

#[test]
fn continuation_response_has_no_playlist_tracks_it_is_only_sections() {
    let next = parse_continuation(HOME_NEXT);
    assert!(next.tracks.is_empty(), "home sections are not a track list");
}
