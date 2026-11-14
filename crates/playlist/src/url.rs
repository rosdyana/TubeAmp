/// Length of a YouTube video id.
const ID_LEN: usize = 11;

/// Whether `s` has the shape of a YouTube video id: 11 characters from `[A-Za-z0-9_-]`.
pub fn is_video_id(s: &str) -> bool {
    s.len() == ID_LEN && is_id_text(s)
}

/// Whether `s` is a non-empty run of YouTube id characters, `[A-Za-z0-9_-]`.
fn is_id_text(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// What a YouTube or YouTube Music link points at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Link {
    /// One video, by its id.
    Video(String),
    /// A playlist, by its list id without the `VL` prefix (`PL…`, or `OLAK5uy…` for an album
    /// playlist).
    Playlist(String),
    /// An album, by its browse id (`MPREb_…`).
    Album(String),
    /// An artist's channel, by its `UC…` id.
    Artist(String),
}

/// The lowercase host, the path and the query of `input`, without the scheme and the fragment.
/// The path has no leading slash and the query has no `?`. A bare id has an empty path and query.
fn split_link(input: &str) -> (String, &str, &str) {
    let rest = match input.split_once("://") {
        Some((_, after)) => after,
        None => input,
    };
    let rest = rest.split('#').next().unwrap_or(rest);
    let (host_path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (host, path) = host_path.split_once('/').unwrap_or((host_path, ""));
    (host.to_ascii_lowercase(), path, query)
}

/// The value of `key` in a query string such as `v=ID&list=PL…`.
fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, value)| value)
}

/// The video id in `input`, which may be a bare id or a YouTube / YouTube Music link:
/// `youtu.be/ID`, `youtube.com/watch?v=ID` and `music.youtube.com/watch?v=ID` (any host ending in
/// `youtube.com` with a `v=` query parameter). `None` when no valid id is found.
pub fn parse_video_id(input: &str) -> Option<String> {
    let s = input.trim();
    if is_video_id(s) {
        return Some(s.to_string());
    }
    let (host, path, query) = split_link(s);
    let candidate = if host == "youtu.be" || host == "www.youtu.be" {
        path.split('/').next().unwrap_or("").to_string()
    } else if host == "youtube.com" || host.ends_with(".youtube.com") {
        query_value(query, "v")?.to_string()
    } else {
        return None;
    };
    is_video_id(&candidate).then_some(candidate)
}

/// The link `input` points at: a bare video id, or a video, playlist, album or artist link on
/// YouTube or YouTube Music. `None` for anything else, and for `@handle` links, which would need
/// a request to resolve.
///
/// * `…/playlist?list=ID` is a playlist. This covers album playlists (`OLAK5uy…`).
/// * `…/watch?v=X&list=ID` is the playlist, unless the list is a `RD…` radio mix, which is the
///   video alone.
/// * `…/browse/MPREb_…` is an album, `…/browse/VL…` a playlist, and `…/browse/UC…` or
///   `…/channel/UC…` an artist.
/// * Any other link with a video id (`youtu.be/ID`, `…/watch?v=ID`) is a video.
pub fn parse_link(input: &str) -> Option<Link> {
    let s = input.trim();
    if is_video_id(s) {
        return Some(Link::Video(s.to_string()));
    }
    let (host, path, query) = split_link(s);
    if host == "youtube.com" || host.ends_with(".youtube.com") {
        if let Some(link) = youtube_link(path, query) {
            return Some(link);
        }
    }
    parse_video_id(s).map(Link::Video)
}

/// The playlist, album or artist a YouTube path and query name, if they name one. `None` leaves
/// the link to [`parse_video_id`].
fn youtube_link(path: &str, query: &str) -> Option<Link> {
    let mut segments = path.split('/').filter(|p| !p.is_empty());
    let first = segments.next()?;
    let list = query_value(query, "list")
        .filter(|l| is_id_text(l) && !l.starts_with("RD"))
        .map(|l| Link::Playlist(l.to_string()));
    match first {
        "playlist" | "watch" => list,
        "browse" => browse_link(segments.next()?),
        "channel" => artist_link(segments.next()?),
        _ => None,
    }
}

/// The album, playlist or artist with browse id `id`.
fn browse_link(id: &str) -> Option<Link> {
    if !is_id_text(id) {
        None
    } else if id.starts_with("MPRE") || id.starts_with("OLAK5uy") {
        Some(Link::Album(id.to_string()))
    } else if let Some(list) = id.strip_prefix("VL").filter(|l| !l.is_empty()) {
        Some(Link::Playlist(list.to_string()))
    } else {
        artist_link(id)
    }
}

/// An artist link for a channel id `UC…`.
fn artist_link(id: &str) -> Option<Link> {
    (id.starts_with("UC") && is_id_text(id)).then(|| Link::Artist(id.to_string()))
}

/// Every link in `text`, in order. The text is read line by line, and each line is split at its
/// commas, so one line, one link, or a mix of both, all work. Blank lines and `#` comment lines
/// are skipped, and so is anything that is not a link.
pub fn parse_links(text: &str) -> Vec<Link> {
    text.lines()
        .map(str::trim)
        .filter(|line| !line.starts_with('#'))
        .flat_map(|line| line.split(','))
        .filter_map(parse_link)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bare_ids_are_accepted_when_well_formed() {
        assert_eq!(
            parse_video_id("kJQP7kiw5Fk").as_deref(),
            Some("kJQP7kiw5Fk")
        );
        assert_eq!(
            parse_video_id("  fJ9rUzIMcZQ \n").as_deref(),
            Some("fJ9rUzIMcZQ")
        );
        assert_eq!(
            parse_video_id("a-b_c-d_e-f"),
            Some("a-b_c-d_e-f".to_string())
        );
        assert_eq!(
            parse_video_id("a-b_c-d_e-"),
            None,
            "10 characters is too short"
        );
        assert_eq!(parse_video_id("short"), None);
        assert_eq!(parse_video_id("kJQP7kiw5F!"), None);
        assert_eq!(parse_video_id(""), None);
    }

    #[test]
    fn youtu_be_short_links() {
        assert_eq!(
            parse_video_id("https://youtu.be/kJQP7kiw5Fk?si=abc").as_deref(),
            Some("kJQP7kiw5Fk")
        );
        assert_eq!(
            parse_video_id("youtu.be/kJQP7kiw5Fk").as_deref(),
            Some("kJQP7kiw5Fk")
        );
    }

    #[test]
    fn watch_links_on_youtube_and_youtube_music() {
        assert_eq!(
            parse_video_id("https://www.youtube.com/watch?v=kJQP7kiw5Fk&list=PL1&index=2")
                .as_deref(),
            Some("kJQP7kiw5Fk")
        );
        assert_eq!(
            parse_video_id("https://music.youtube.com/watch?v=fJ9rUzIMcZQ").as_deref(),
            Some("fJ9rUzIMcZQ")
        );
        assert_eq!(
            parse_video_id("https://m.youtube.com/watch?feature=share&v=fJ9rUzIMcZQ#t=10")
                .as_deref(),
            Some("fJ9rUzIMcZQ")
        );
    }

    #[test]
    fn other_hosts_and_broken_links_are_rejected() {
        assert_eq!(
            parse_video_id("https://example.com/watch?v=kJQP7kiw5Fk"),
            None
        );
        assert_eq!(
            parse_video_id("https://www.youtube.com/watch?v=short"),
            None
        );
        assert_eq!(
            parse_video_id("https://www.youtube.com/results?search_query=x"),
            None
        );
        assert_eq!(parse_video_id("https://youtu.be/"), None);
        assert_eq!(parse_video_id("hello world"), None);
    }

    #[test]
    fn a_playlist_link_is_the_whole_list() {
        assert_eq!(
            parse_link("https://music.youtube.com/playlist?list=PLabc123-_x"),
            Some(Link::Playlist("PLabc123-_x".into()))
        );
        assert_eq!(
            parse_link("https://www.youtube.com/playlist?list=OLAK5uy_albumplaylist"),
            Some(Link::Playlist("OLAK5uy_albumplaylist".into()))
        );
    }

    #[test]
    fn a_watch_link_with_a_list_is_the_playlist_unless_it_is_a_radio_mix() {
        assert_eq!(
            parse_link("https://www.youtube.com/watch?v=kJQP7kiw5Fk&list=PL123"),
            Some(Link::Playlist("PL123".into()))
        );
        assert_eq!(
            parse_link("https://www.youtube.com/watch?list=PL123"),
            Some(Link::Playlist("PL123".into())),
            "a list without a video is still the list"
        );
        assert_eq!(
            parse_link("https://music.youtube.com/watch?v=kJQP7kiw5Fk&list=RDAMVMkJQP7kiw5Fk"),
            Some(Link::Video("kJQP7kiw5Fk".into())),
            "a radio mix adds only the song"
        );
        assert_eq!(
            parse_link("https://music.youtube.com/watch?v=kJQP7kiw5Fk&list=RDkJQP7kiw5Fk&index=3"),
            Some(Link::Video("kJQP7kiw5Fk".into()))
        );
    }

    #[test]
    fn browse_links_are_albums_playlists_or_artists() {
        assert_eq!(
            parse_link("https://music.youtube.com/browse/MPREb_7ltM34kr0mH"),
            Some(Link::Album("MPREb_7ltM34kr0mH".into()))
        );
        assert_eq!(
            parse_link("https://music.youtube.com/browse/VLPLabc"),
            Some(Link::Playlist("PLabc".into()))
        );
        assert_eq!(
            parse_link("https://music.youtube.com/browse/UC_kRDKYrUlrbtrSiyu5Tflg"),
            Some(Link::Artist("UC_kRDKYrUlrbtrSiyu5Tflg".into()))
        );
    }

    #[test]
    fn channel_links_are_artists_and_handles_are_rejected() {
        assert_eq!(
            parse_link("https://music.youtube.com/channel/UC_kRDKYrUlrbtrSiyu5Tflg?si=x"),
            Some(Link::Artist("UC_kRDKYrUlrbtrSiyu5Tflg".into()))
        );
        assert_eq!(
            parse_link("https://www.youtube.com/channel/UC_kRDKYrUlrbtrSiyu5Tflg/videos"),
            Some(Link::Artist("UC_kRDKYrUlrbtrSiyu5Tflg".into()))
        );
        assert_eq!(
            parse_link("https://www.youtube.com/@SomeArtist"),
            None,
            "handles need a request to resolve"
        );
        assert_eq!(
            parse_link("https://www.youtube.com/channel/notachannel"),
            None
        );
    }

    #[test]
    fn bare_ids_and_short_links_are_videos() {
        assert_eq!(
            parse_link("kJQP7kiw5Fk"),
            Some(Link::Video("kJQP7kiw5Fk".into()))
        );
        assert_eq!(
            parse_link("https://youtu.be/kJQP7kiw5Fk?si=abc"),
            Some(Link::Video("kJQP7kiw5Fk".into()))
        );
        assert_eq!(
            parse_link("https://m.youtube.com/watch?feature=share&v=fJ9rUzIMcZQ#t=10"),
            Some(Link::Video("fJ9rUzIMcZQ".into()))
        );
        assert_eq!(parse_link("https://example.com/playlist?list=PLabc"), None);
        assert_eq!(parse_link("hello world"), None);
    }

    #[test]
    fn text_with_mixed_separators_yields_every_link_in_order() {
        let text = "# my list\r\n\
            kJQP7kiw5Fk, https://youtu.be/fJ9rUzIMcZQ\n\
            \n\
            https://music.youtube.com/playlist?list=PLabc,https://www.youtube.com/@Nobody\n\
            https://music.youtube.com/browse/MPREb_7ltM34kr0mH,  not a link\n\
            #https://youtu.be/aaaaaaaaaaa\n";
        assert_eq!(
            parse_links(text),
            vec![
                Link::Video("kJQP7kiw5Fk".into()),
                Link::Video("fJ9rUzIMcZQ".into()),
                Link::Playlist("PLabc".into()),
                Link::Album("MPREb_7ltM34kr0mH".into()),
            ]
        );
        assert!(parse_links("\n  \n# only comments\n").is_empty());
    }
}
