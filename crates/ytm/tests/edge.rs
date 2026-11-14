//! Hand-made edge cases: malformed and empty input, unknown renderers, the two continuation
//! styles, and rows that are not tracks.

use tubeamp_ytm::{
    parse_continuation, parse_home, parse_next, parse_playlist, parse_search, Item,
};

#[test]
fn malformed_or_empty_input_gives_empty_results_without_panicking() {
    for body in ["", "not json", "[]", "null", "{}", r#"{"contents": 5}"#, r#"{"contents": {"twoColumnBrowseResultsRenderer": null}}"#] {
        let s = parse_search(body);
        assert!(s.top.is_none() && s.shelves.is_empty(), "search of {body:?}");
        let h = parse_home(body);
        assert!(h.shelves.is_empty() && h.continuation.is_none(), "home of {body:?}");
        let p = parse_playlist(body);
        assert!(p.tracks.is_empty() && p.title.is_empty() && p.author.is_none(), "playlist of {body:?}");
        assert!(parse_next(body).is_empty(), "next of {body:?}");
        let c = parse_continuation(body);
        assert!(c.shelves.is_empty() && c.tracks.is_empty() && c.continuation.is_none());
    }
}

#[test]
fn unknown_renderers_are_skipped_and_known_neighbours_still_parse() {
    let body = r#"{"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [
        {"musicMysteryRenderer": {"title": {"runs": [{"text": "??"}]}, "contents": []}},
        {"musicCarouselShelfRenderer": {"header": {"musicCarouselShelfBasicHeaderRenderer": {"title": {"runs": [{"text": "Known"}]}}},
          "contents": [
            {"somethingNew": {}},
            {"musicTwoRowItemRenderer": {"title": {"runs": [{"text": "Solo"}]}, "navigationEndpoint": {"watchEndpoint": {"videoId": "abcdefghijk"}}, "subtitle": {"runs": [{"text": "Song"}, {"text": " • "}, {"text": "Ann Artist", "navigationEndpoint": {"browseEndpoint": {"browseId": "UCannxxxxxxxxxxxxxxxxxx"}}}]}}}
          ]}}
      ]}}}}]}}}"#;
    let home = parse_home(body);
    assert_eq!(home.shelves.len(), 1);
    assert_eq!(home.shelves[0].title, "Known");
    match &home.shelves[0].items[..] {
        [Item::Track(t)] => {
            assert_eq!(t.video_id, "abcdefghijk");
            assert_eq!(t.title, "Solo");
            assert_eq!(t.artists[0].name, "Ann Artist");
            assert_eq!(t.duration_secs, None);
            assert!(!t.explicit);
        }
        other => panic!("expected one track, got {other:?}"),
    }
}

#[test]
fn rows_without_a_video_or_a_known_target_are_not_items() {
    let body = r#"{"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [
        {"musicShelfRenderer": {"title": {"runs": [{"text": "Mixed"}]}, "contents": [
          {"musicResponsiveListItemRenderer": {"flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Unavailable"}]}}}]}},
          {"musicResponsiveListItemRenderer": {"flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Podcast show"}]}}}],
            "navigationEndpoint": {"browseEndpoint": {"browseId": "MPSPPLshow"}}}},
          {"musicResponsiveListItemRenderer": {"flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Real"}]}}}],
            "playlistItemData": {"videoId": "realvideo01"}, "fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"simpleText": "1:02:03"}}}]}}
        ]}}
      ]}}}}]}}}"#;
    let home = parse_home(body);
    let shelf = &home.shelves[0];
    assert_eq!(shelf.items.len(), 1, "only the playable row survives");
    match &shelf.items[0] {
        Item::Track(t) => {
            assert_eq!(t.title, "Real");
            assert_eq!(t.duration_secs, Some(3723), "1:02:03 is over an hour");
        }
        other => panic!("expected a track, got {other:?}"),
    }
}

#[test]
fn explicit_badge_is_read_from_badges() {
    let body = r#"{"contents": {"singleColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [
        {"musicShelfRenderer": {"contents": [
          {"musicResponsiveListItemRenderer": {"flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Loud"}]}}}],
            "playlistItemData": {"videoId": "loudvideo001"},
            "badges": [{"musicInlineBadgeRenderer": {"accessibilityData": {"accessibilityData": {"label": "Explicit"}}, "icon": {"iconType": "MUSIC_EXPLICIT_BADGE"}}}]}}
        ]}}
      ]}}}}]}}}"#;
    let page = parse_playlist(body);
    assert_eq!(page.tracks.len(), 1);
    assert!(page.tracks[0].explicit);
}

#[test]
fn playlist_track_continuation_comes_from_the_shelf() {
    // Continuation in the shelf's own `continuations`.
    let body = r#"{"contents": {"twoColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [
        {"musicPlaylistShelfRenderer": {"contents": [], "continuations": [{"nextContinuationData": {"continuation": "SHELF_TOKEN"}}]}}
      ]}}}}]}}}"#;
    assert_eq!(parse_playlist(body).continuation.as_deref(), Some("SHELF_TOKEN"));

    // Continuation as a trailing continuationItemRenderer inside the shelf.
    let body = r#"{"contents": {"twoColumnBrowseResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [
        {"musicPlaylistShelfRenderer": {"contents": [
          {"continuationItemRenderer": {"continuationEndpoint": {"continuationCommand": {"token": "ITEM_TOKEN"}}}}
        ]}}
      ]}}}}]}}}"#;
    assert_eq!(parse_playlist(body).continuation.as_deref(), Some("ITEM_TOKEN"));
}

#[test]
fn continuation_responses_in_both_styles_are_read() {
    // Style 1: continuationContents with a music shelf continuation and nested tokens.
    let style_one = r#"{"continuationContents": {"musicPlaylistShelfContinuation": {"contents": [
        {"musicResponsiveListItemRenderer": {"flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Next one"}]}}}],
          "playlistItemData": {"videoId": "nextvideo001"}, "fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"simpleText": "4:01"}}}]}}
      ], "continuations": [{"nextContinuationData": {"continuation": "PAGE_3"}}]}}}"#;
    let c = parse_continuation(style_one);
    assert_eq!(c.tracks.len(), 1);
    assert_eq!(c.tracks[0].video_id, "nextvideo001");
    assert_eq!(c.tracks[0].duration_secs, Some(241));
    assert_eq!(c.continuation.as_deref(), Some("PAGE_3"));

    // Style 2: onResponseReceivedActions with bare rows and a trailing continuationItemRenderer.
    let style_two = r#"{"onResponseReceivedActions": [{"appendContinuationItemsAction": {"continuationItems": [
        {"musicResponsiveListItemRenderer": {"flexColumns": [{"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Bare"}]}}}],
          "playlistItemData": {"videoId": "barevideo001"}}},
        {"continuationItemRenderer": {"continuationEndpoint": {"continuationCommand": {"token": "PAGE_4"}}}}
      ]}}]}"#;
    let c = parse_continuation(style_two);
    assert_eq!(c.tracks.iter().map(|t| t.video_id.as_str()).collect::<Vec<_>>(), vec!["barevideo001"]);
    assert_eq!(c.continuation.as_deref(), Some("PAGE_4"));
}

#[test]
fn up_next_skips_the_counterpart_copy_of_a_video() {
    let body = r#"{"playlistPanelVideoWrapperRenderer": {
        "primaryRenderer": {"playlistPanelVideoRenderer": {"title": {"runs": [{"text": "Main"}]}, "videoId": "mainvideo001", "lengthText": {"runs": [{"text": "2:00"}]}}},
        "counterpart": [{"counterpartRenderer": {"playlistPanelVideoRenderer": {"title": {"runs": [{"text": "Main"}]}, "videoId": "mainvideo001"}}}]
      }}"#;
    let tracks = parse_next(body);
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].video_id, "mainvideo001");
    assert_eq!(tracks[0].duration_secs, Some(120));
}

#[test]
fn search_top_result_card_can_be_a_track() {
    let body = r#"{"contents": {"tabbedSearchResultsRenderer": {"tabs": [{"tabRenderer": {"content": {"sectionListRenderer": {"contents": [
        {"musicCardShelfRenderer": {"title": {"runs": [{"text": "Song Title"}]},
          "subtitle": {"runs": [{"text": "Song"}, {"text": " • "}, {"text": "Solo Act", "navigationEndpoint": {"browseEndpoint": {"browseId": "UCsoloxxxxxxxxxxxxxxxxx"}}}]},
          "onTap": {"watchEndpoint": {"videoId": "topvideo001"}},
          "contents": []}}
      ]}}}}]}}}"#;
    let results = parse_search(body);
    match results.top {
        Some(Item::Track(t)) => {
            assert_eq!(t.video_id, "topvideo001");
            assert_eq!(t.title, "Song Title");
            assert_eq!(t.artists[0].name, "Solo Act");
        }
        other => panic!("expected a track top result, got {other:?}"),
    }
}
