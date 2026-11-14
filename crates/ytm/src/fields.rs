//! Readers for the small JSON pieces that recur in InnerTube responses: text runs, navigation
//! endpoints, subtitles, thumbnails and durations. Everything tolerates missing or wrong-typed
//! values; `Value` indexing never panics.

use serde_json::Value;

use crate::model::{AlbumRef, ArtistRef};

pub(crate) fn arr(v: &Value) -> &[Value] {
    v.as_array().map(Vec::as_slice).unwrap_or(&[])
}

/// A non-empty string value.
pub(crate) fn str_of(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_owned)
}

/// The runs of a `{"runs": [...]}` text object.
pub(crate) fn runs(v: &Value) -> &[Value] {
    arr(&v["runs"])
}

/// The text of a `{"runs": [...]}` or `{"simpleText": "..."}` object, concatenated.
pub(crate) fn text(v: &Value) -> String {
    if let Some(s) = v["simpleText"].as_str() {
        return s.to_owned();
    }
    runs(v).iter().filter_map(|r| r["text"].as_str()).collect()
}

/// Where a navigation endpoint leads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Target {
    /// `watchEndpoint` with a video id.
    Watch(String),
    /// A playlist (`watchEndpoint` or `watchPlaylistEndpoint` with a playlist id, no video id).
    Playlist(String),
    /// `browseEndpoint`: an album, artist or playlist page.
    Browse {
        id: String,
        page_type: Option<String>,
    },
}

/// Reads a `navigationEndpoint`-shaped object.
pub(crate) fn target(ep: &Value) -> Option<Target> {
    if let Some(id) = str_of(&ep["watchEndpoint"]["videoId"]) {
        return Some(Target::Watch(id));
    }
    if let Some(id) = str_of(&ep["watchPlaylistEndpoint"]["playlistId"])
        .or_else(|| str_of(&ep["watchEndpoint"]["playlistId"]))
    {
        return Some(Target::Playlist(id));
    }
    let b = &ep["browseEndpoint"];
    let id = str_of(&b["browseId"])?;
    let page_type = str_of(
        &b["browseEndpointContextSupportedConfigs"]["browseEndpointContextMusicConfig"]["pageType"],
    );
    Some(Target::Browse { id, page_type })
}

/// Picks the first watch target, else the first other target. `None` entries are skipped.
pub(crate) fn pick(candidates: &[Option<Target>]) -> Option<Target> {
    let mut found = candidates.iter().flatten();
    let first_watch = found
        .clone()
        .find(|t| matches!(t, Target::Watch(_)))
        .cloned();
    first_watch.or_else(|| found.find(|t| !matches!(t, Target::Watch(_))).cloned())
}

/// What a browse id names.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Album,
    Artist,
    Playlist,
}

/// Classifies a browse id, preferring the page type when YouTube Music gives one.
pub(crate) fn classify(id: &str, page_type: Option<&str>) -> Option<Kind> {
    match page_type {
        Some("MUSIC_PAGE_TYPE_ALBUM") => return Some(Kind::Album),
        Some("MUSIC_PAGE_TYPE_ARTIST") | Some("MUSIC_PAGE_TYPE_USER_CHANNEL") => {
            return Some(Kind::Artist)
        }
        Some("MUSIC_PAGE_TYPE_PLAYLIST") => return Some(Kind::Playlist),
        _ => {}
    }
    if id.starts_with("MPRE") || id.starts_with("OLAK5uy") {
        Some(Kind::Album)
    } else if id.starts_with("UC") {
        Some(Kind::Artist)
    } else if id.starts_with("VL") {
        Some(Kind::Playlist)
    } else {
        None
    }
}

/// Words YouTube Music puts between the fields of a subtitle. They are never names.
const LABELS: &[&str] = &[
    "song", "songs", "video", "videos", "album", "albums", "single", "ep", "playlist",
    "playlists", "artist", "artists", "profile", "episode", "episodes", "podcast", "show",
    "track", "tracks", "mix", "radio",
];

/// The parts of a subtitle line ("Album • Daft Punk • 2001").
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub(crate) struct Sub {
    /// Runs that link to a channel.
    pub artists: Vec<ArtistRef>,
    /// The run that links to an album.
    pub album: Option<AlbumRef>,
    /// Unlinked text that is not a label, count, year or duration.
    pub plain: Vec<String>,
    pub year: Option<u32>,
    /// From "48 songs".
    pub count: Option<u32>,
    pub duration: Option<u32>,
}

/// Reads subtitle runs into a `Sub`.
pub(crate) fn subtitle<'a>(runs: impl IntoIterator<Item = &'a Value>) -> Sub {
    let mut sub = Sub::default();
    for run in runs {
        let t = run["text"].as_str().unwrap_or("").trim();
        if t.is_empty() || !t.chars().any(char::is_alphanumeric) {
            continue; // separators such as " • ", " & ", ","
        }
        if let Some(Target::Browse { id, page_type }) = target(&run["navigationEndpoint"]) {
            match classify(&id, page_type.as_deref()) {
                Some(Kind::Artist) => {
                    sub.artists.push(ArtistRef {
                        name: t.to_owned(),
                        id: Some(id),
                    });
                    continue;
                }
                Some(Kind::Album) => {
                    if sub.album.is_none() {
                        sub.album = Some(AlbumRef {
                            name: t.to_owned(),
                            id: Some(id),
                        });
                    }
                    continue;
                }
                _ => {}
            }
        }
        if t.len() == 4 && t.bytes().all(|b| b.is_ascii_digit()) {
            sub.year = sub.year.or_else(|| t.parse().ok());
        } else if let Some(d) = parse_duration(t) {
            sub.duration = sub.duration.or(Some(d));
        } else if let Some(n) = count_of(t) {
            sub.count = sub.count.or(Some(n));
        } else if !is_noise(t) {
            sub.plain.push(t.to_owned());
        }
    }
    sub
}

fn is_noise(t: &str) -> bool {
    let lower = t.to_lowercase();
    t.starts_with(|c: char| c.is_ascii_digit())
        || lower.contains("subscriber")
        || LABELS.contains(&lower.as_str())
}

/// "48 songs" → 48, "1,234 songs" → 1234.
fn count_of(t: &str) -> Option<u32> {
    let mut words = t.split_whitespace();
    let num = words.next()?;
    let word = words.next()?.to_lowercase();
    let is_count_word = ["song", "track", "video", "title"]
        .iter()
        .any(|w| word.starts_with(w));
    if !is_count_word || num.is_empty() {
        return None;
    }
    if !num.chars().all(|c| c.is_ascii_digit() || c == ',' || c == '.') {
        return None;
    }
    let digits: String = num.chars().filter(char::is_ascii_digit).collect();
    digits.parse().ok()
}

/// "3:45" → 225, "1:02:03" → 3723. `None` for anything else.
pub fn parse_duration(text: &str) -> Option<u32> {
    let parts: Vec<&str> = text.trim().split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }
    let mut total: u32 = 0;
    for part in parts {
        if part.is_empty() || !part.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let n: u32 = part.parse().ok()?;
        total = total.checked_mul(60)?.checked_add(n)?;
    }
    Some(total)
}

/// The largest thumbnail URL under `v`, looking at most a few levels down.
pub(crate) fn thumbnail(v: &Value) -> Option<String> {
    let thumbs = find_thumbnails(v, 5)?;
    str_of(&arr(thumbs).last()?["url"])
}

fn find_thumbnails(v: &Value, depth: u8) -> Option<&Value> {
    if v["thumbnails"].is_array() {
        return Some(&v["thumbnails"]);
    }
    if depth == 0 {
        return None;
    }
    v.as_object()?
        .values()
        .find_map(|child| find_thumbnails(child, depth - 1))
}

/// Whether a row carries the explicit badge.
pub(crate) fn explicit(row: &Value) -> bool {
    arr(&row["badges"])
        .iter()
        .chain(arr(&row["subtitleBadges"]))
        .any(|b| {
            let badge = &b["musicInlineBadgeRenderer"];
            badge["icon"]["iconType"] == "MUSIC_EXPLICIT_BADGE"
                || badge["accessibilityData"]["accessibilityData"]["label"] == "Explicit"
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durations() {
        assert_eq!(parse_duration("3:45"), Some(225));
        assert_eq!(parse_duration("0:09"), Some(9));
        assert_eq!(parse_duration("1:02:03"), Some(3723));
        assert_eq!(parse_duration("12:00:00"), Some(43200));
        assert_eq!(parse_duration("45"), None);
        assert_eq!(parse_duration(""), None);
        assert_eq!(parse_duration("3:"), None);
        assert_eq!(parse_duration(":45"), None);
        assert_eq!(parse_duration("3:4a"), None);
        assert_eq!(parse_duration("1:2:3:4"), None);
        assert_eq!(parse_duration("Song"), None);
    }

    #[test]
    fn counts_and_years_in_subtitles() {
        let runs = [
            json_run("48 songs", None),
            json_run("1,234 songs", None),
        ];
        assert_eq!(subtitle(runs.iter()).count, Some(48));
        let runs = [json_run("2001", None)];
        assert_eq!(subtitle(runs.iter()).year, Some(2001));
        let runs = [json_run("Playlist", None), json_run(" • ", None), json_run("Mix Crew", None)];
        assert_eq!(subtitle(runs.iter()).plain, vec!["Mix Crew".to_string()]);
    }

    #[test]
    fn classify_by_prefix_and_page_type() {
        assert_eq!(classify("MPREb_abc", None), Some(Kind::Album));
        assert_eq!(classify("OLAK5uy_x", None), Some(Kind::Album));
        assert_eq!(classify("UCabc", None), Some(Kind::Artist));
        assert_eq!(classify("VLPLabc", None), Some(Kind::Playlist));
        assert_eq!(classify("FEmusic_home", None), None);
        assert_eq!(
            classify("UCabc", Some("MUSIC_PAGE_TYPE_ALBUM")),
            Some(Kind::Album)
        );
    }

    #[test]
    fn thumbnail_takes_the_largest_entry() {
        let v = json_thumb(&[
            "https://example.test/a=w60-h60",
            "https://example.test/a=w226-h226",
        ]);
        assert_eq!(
            thumbnail(&v).as_deref(),
            Some("https://example.test/a=w226-h226")
        );
        assert_eq!(thumbnail(&Value::Null), None);
    }

    fn json_run(text: &str, nav: Option<Value>) -> Value {
        let mut run = serde_json::json!({ "text": text });
        if let Some(nav) = nav {
            run["navigationEndpoint"] = nav;
        }
        run
    }

    fn json_thumb(urls: &[&str]) -> Value {
        let thumbs: Vec<Value> = urls.iter().map(|u| serde_json::json!({ "url": u })).collect();
        serde_json::json!({ "thumbnail": { "musicThumbnailRenderer": { "thumbnail": { "thumbnails": thumbs } } } })
    }
}
