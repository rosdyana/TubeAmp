//! Builds models from the row renderers: `musicResponsiveListItemRenderer` (list rows),
//! `musicTwoRowItemRenderer` (carousel and grid cards), `musicCardShelfRenderer` (top result) and
//! `playlistPanelVideoRenderer` (up next).

use serde_json::Value;

use crate::fields::{
    arr, explicit, pick, runs, str_of, subtitle, target, text, thumbnail, Kind, Sub, Target,
    classify, parse_duration,
};
use crate::model::{AlbumItem, ArtistItem, ArtistRef, Item, PlaylistItem, Track};

/// The parts shared by every row kind, before it becomes a model.
pub(crate) struct Row {
    pub title: String,
    pub sub: Sub,
    pub thumbnail: Option<String>,
    pub explicit: bool,
    /// Duration from a dedicated column, when the row has one.
    pub duration: Option<u32>,
    pub target: Option<Target>,
}

/// A `musicResponsiveListItemRenderer`. Title is the first flex column, the rest are subtitle.
pub(crate) fn responsive(r: &Value) -> Option<Item> {
    let flex = arr(&r["flexColumns"]);
    let title_text = &flex.first()?["musicResponsiveListItemFlexColumnRenderer"]["text"];
    let sub_runs = flex
        .iter()
        .skip(1)
        .flat_map(|c| runs(&c["musicResponsiveListItemFlexColumnRenderer"]["text"]).iter());
    let fixed = text(&r["fixedColumns"][0]["musicResponsiveListItemFixedColumnRenderer"]["text"]);

    let video_from_data = str_of(&r["playlistItemData"]["videoId"]).map(Target::Watch);
    let row_target = pick(&[
        video_from_data,
        target(&r["navigationEndpoint"]),
        target(&title_text["runs"][0]["navigationEndpoint"]),
    ])
    .or_else(|| {
        target(
            &r["overlay"]["musicItemThumbnailOverlayRenderer"]["content"]["musicPlayButtonRenderer"]
                ["playNavigationEndpoint"],
        )
    });

    to_item(Row {
        title: text(title_text),
        sub: subtitle(sub_runs),
        thumbnail: thumbnail(&r["thumbnail"]),
        explicit: explicit(r),
        duration: parse_duration(&fixed),
        target: row_target,
    })
}

/// A `musicTwoRowItemRenderer`: carousel and grid cards.
pub(crate) fn two_row(r: &Value) -> Option<Item> {
    to_item(Row {
        title: text(&r["title"]),
        sub: subtitle(runs(&r["subtitle"])),
        thumbnail: thumbnail(&r["thumbnailRenderer"]),
        explicit: explicit(r),
        duration: None,
        target: pick(&[
            target(&r["navigationEndpoint"]),
            target(&r["title"]["runs"][0]["navigationEndpoint"]),
        ]),
    })
}

/// The "Top result" card of a search. Its target is `onTap`.
pub(crate) fn card(c: &Value) -> Option<Item> {
    to_item(Row {
        title: text(&c["title"]),
        sub: subtitle(runs(&c["subtitle"])),
        thumbnail: thumbnail(&c["thumbnail"]),
        explicit: explicit(c),
        duration: None,
        target: pick(&[
            target(&c["onTap"]),
            target(&c["title"]["runs"][0]["navigationEndpoint"]),
            target(&c["navigationEndpoint"]),
        ]),
    })
}

/// A `playlistPanelVideoRenderer` row of the up-next queue. Only rows with a video are tracks.
pub(crate) fn panel_track(r: &Value) -> Option<Track> {
    let video_id = match target(&r["navigationEndpoint"]) {
        Some(Target::Watch(id)) => id,
        _ => str_of(&r["videoId"])?,
    };
    let byline = if arr(&r["longBylineText"]["runs"]).is_empty() {
        &r["shortBylineText"]
    } else {
        &r["longBylineText"]
    };
    let row = Row {
        title: text(&r["title"]),
        sub: subtitle(runs(byline)),
        thumbnail: thumbnail(&r["thumbnail"]),
        explicit: explicit(r),
        duration: parse_duration(&text(&r["lengthText"])),
        target: Some(Target::Watch(video_id.clone())),
    };
    Some(track(video_id, row))
}

fn to_item(row: Row) -> Option<Item> {
    match row.target.clone()? {
        Target::Watch(video_id) => Some(Item::Track(track(video_id, row))),
        Target::Playlist(playlist_id) => Some(Item::Playlist(playlist(
            format!("VL{playlist_id}"),
            playlist_id,
            row,
        ))),
        Target::Browse { id, page_type } => match classify(&id, page_type.as_deref())? {
            Kind::Album => Some(Item::Album(AlbumItem {
                browse_id: id,
                title: row.title,
                artists: artists_of(&row.sub),
                year: row.sub.year,
                thumbnail: row.thumbnail,
            })),
            Kind::Artist => Some(Item::Artist(ArtistItem {
                browse_id: id,
                name: row.title,
                thumbnail: row.thumbnail,
            })),
            Kind::Playlist => {
                let playlist_id = id.strip_prefix("VL").unwrap_or(&id).to_owned();
                Some(Item::Playlist(playlist(id, playlist_id, row)))
            }
        },
    }
}

fn track(video_id: String, row: Row) -> Track {
    Track {
        video_id,
        title: row.title,
        artists: artists_of(&row.sub),
        album: row.sub.album.clone(),
        duration_secs: row.duration.or(row.sub.duration),
        thumbnail: row.thumbnail,
        explicit: row.explicit,
    }
}

fn playlist(browse_id: String, playlist_id: String, row: Row) -> PlaylistItem {
    PlaylistItem {
        browse_id,
        playlist_id,
        title: row.title,
        author: author_of(&row.sub),
        count: row.sub.count,
        thumbnail: row.thumbnail,
    }
}

/// Linked artists, or the first plain subtitle text when nothing is linked.
pub(crate) fn artists_of(sub: &Sub) -> Vec<ArtistRef> {
    if !sub.artists.is_empty() {
        return sub.artists.clone();
    }
    sub.plain
        .first()
        .map(|name| {
            vec![ArtistRef {
                name: name.clone(),
                id: None,
            }]
        })
        .unwrap_or_default()
}

/// The first artist, or the first plain subtitle text.
pub(crate) fn author_of(sub: &Sub) -> Option<String> {
    sub.artists
        .first()
        .map(|a| a.name.clone())
        .or_else(|| sub.plain.first().cloned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn responsive_row_with_a_video_is_a_track() {
        let row = json!({
            "playlistItemData": {"videoId": "abc"},
            "flexColumns": [
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Song"}]}}},
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [
                    {"text": "Artist X", "navigationEndpoint": {"browseEndpoint": {"browseId": "UCx"}}},
                    {"text": " • "},
                    {"text": "Album Y", "navigationEndpoint": {"browseEndpoint": {"browseId": "MPREb_y"}}}
                ]}}}
            ],
            "fixedColumns": [{"musicResponsiveListItemFixedColumnRenderer": {"text": {"simpleText": "2:30"}}}],
            "badges": [{"musicInlineBadgeRenderer": {"icon": {"iconType": "MUSIC_EXPLICIT_BADGE"}}}]
        });
        match responsive(&row) {
            Some(Item::Track(t)) => {
                assert_eq!(t.video_id, "abc");
                assert_eq!(t.title, "Song");
                assert_eq!(t.artists[0].name, "Artist X");
                assert_eq!(t.album.unwrap().name, "Album Y");
                assert_eq!(t.duration_secs, Some(150));
                assert!(t.explicit);
            }
            other => panic!("expected a track, got {other:?}"),
        }
    }

    #[test]
    fn row_without_any_target_is_skipped() {
        let row = json!({
            "flexColumns": [
                {"musicResponsiveListItemFlexColumnRenderer": {"text": {"runs": [{"text": "Gone"}]}}}
            ]
        });
        assert_eq!(responsive(&row), None);
    }

    #[test]
    fn two_row_playlist_with_vl_prefix_strips_it_for_the_playlist_id() {
        let row = json!({
            "title": {"runs": [{"text": "Mix"}]},
            "subtitle": {"runs": [{"text": "Playlist"}, {"text": " • "}, {"text": "12 songs"}]},
            "navigationEndpoint": {"browseEndpoint": {"browseId": "VLPLabc"}}
        });
        match two_row(&row) {
            Some(Item::Playlist(p)) => {
                assert_eq!(p.browse_id, "VLPLabc");
                assert_eq!(p.playlist_id, "PLabc");
                assert_eq!(p.count, Some(12));
            }
            other => panic!("expected a playlist, got {other:?}"),
        }
    }
}
