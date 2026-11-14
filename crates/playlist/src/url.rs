//! Reads a YouTube video id out of a pasted link or a bare id.

/// Length of a YouTube video id.
const ID_LEN: usize = 11;

/// Whether `s` has the shape of a YouTube video id: 11 characters from `[A-Za-z0-9_-]`.
pub fn is_video_id(s: &str) -> bool {
    s.len() == ID_LEN
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The video id in `input`, which may be a bare id or a YouTube / YouTube Music link:
/// `youtu.be/ID`, `youtube.com/watch?v=ID` and `music.youtube.com/watch?v=ID` (any host ending in
/// `youtube.com` with a `v=` query parameter). `None` when no valid id is found.
pub fn parse_video_id(input: &str) -> Option<String> {
    let s = input.trim();
    if is_video_id(s) {
        return Some(s.to_string());
    }
    let rest = match s.split_once("://") {
        Some((_, after)) => after,
        None => s,
    };
    // Drop the fragment first, then split the path from the query.
    let rest = rest.split('#').next().unwrap_or(rest);
    let (host_path, query) = rest.split_once('?').unwrap_or((rest, ""));
    let (host, path) = host_path.split_once('/').unwrap_or((host_path, ""));
    let host = host.to_ascii_lowercase();

    let candidate = if host == "youtu.be" || host == "www.youtu.be" {
        path.split('/').next().unwrap_or("").to_string()
    } else if host == "youtube.com" || host.ends_with(".youtube.com") {
        query
            .split('&')
            .filter_map(|pair| pair.split_once('='))
            .find(|(key, _)| *key == "v")
            .map(|(_, value)| value.to_string())?
    } else {
        return None;
    };
    is_video_id(&candidate).then_some(candidate)
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
}
