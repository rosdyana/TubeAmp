//! Marquee text, as Winamp and Webamp show it (Webamp `js/marqueeUtils.tsx`, `getMarqueeText`
//! and `getMediaText` in `js/selectors.ts`).

/// Track number shown before the title. There is no playlist yet, so it is always 1.
pub const TRACK_NUMBER: u32 = 1;

/// Text shown when nothing is loaded.
pub const IDLE_TEXT: &str = "TubeAmp";

/// `1. Artist - Title (m:ss)`. The artist is omitted when unknown. `None` when there is no title.
pub fn now_playing(author: &str, title: &str, duration_secs: u32) -> Option<String> {
    let title = title.trim();
    if title.is_empty() {
        return None;
    }
    let author = author.trim();
    let name = if author.is_empty() {
        title.to_string()
    } else {
        format!("{author} - {title}")
    };
    Some(format!(
        "{TRACK_NUMBER}. {name} ({})",
        time_truncated(duration_secs)
    ))
}

/// While dragging the position bar: `Seek to: mm:ss/mm:ss (pct%)`.
pub fn seek_text(duration_secs: u32, percent: u32) -> String {
    let elapsed = (u64::from(duration_secs) * u64::from(percent) / 100) as u32;
    format!(
        "Seek to: {}/{} ({percent}%)",
        time_padded(elapsed),
        time_padded(duration_secs)
    )
}

/// While dragging the volume bar: `Volume: v%`.
pub fn volume_text(volume: u8) -> String {
    format!("Volume: {volume}%")
}

/// While dragging the balance bar: `Balance: Center`, or `Balance: p% Left` / `Right`.
pub fn balance_text(balance: i8) -> String {
    match balance {
        0 => "Balance: Center".to_string(),
        b if b > 0 => format!("Balance: {b}% Right"),
        b => format!("Balance: {}% Left", b.unsigned_abs()),
    }
}

/// `mm:ss` with zero-padded minutes (Webamp `getTimeStr(t, false)`).
pub fn time_padded(secs: u32) -> String {
    format!("{:02}:{:02}", secs / 60, secs % 60)
}

/// `m:ss` with no leading zero on the minutes (Webamp `getTimeStr(t)`).
pub fn time_truncated(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn now_playing_has_number_artist_title_and_length() {
        assert_eq!(
            now_playing("Luis Fonsi & Daddy Yankee", "Despacito", 229).as_deref(),
            Some("1. Luis Fonsi & Daddy Yankee - Despacito (3:49)")
        );
    }

    #[test]
    fn now_playing_without_artist_or_title() {
        assert_eq!(
            now_playing("  ", "Song", 65).as_deref(),
            Some("1. Song (1:05)")
        );
        assert_eq!(now_playing("Artist", "   ", 65), None);
        assert_eq!(now_playing("Artist", "", 65), None);
    }

    #[test]
    fn seek_text_shows_elapsed_duration_and_percent() {
        assert_eq!(seek_text(245, 50), "Seek to: 02:02/04:05 (50%)");
        assert_eq!(seek_text(3725, 0), "Seek to: 00:00/62:05 (0%)");
        assert_eq!(seek_text(60, 100), "Seek to: 01:00/01:00 (100%)");
    }

    #[test]
    fn volume_and_balance_text() {
        assert_eq!(volume_text(75), "Volume: 75%");
        assert_eq!(balance_text(0), "Balance: Center");
        assert_eq!(balance_text(-30), "Balance: 30% Left");
        assert_eq!(balance_text(45), "Balance: 45% Right");
        assert_eq!(balance_text(-100), "Balance: 100% Left");
    }

    #[test]
    fn time_formats() {
        assert_eq!(time_padded(65), "01:05");
        assert_eq!(time_truncated(65), "1:05");
        assert_eq!(time_truncated(0), "0:00");
        assert_eq!(time_truncated(600), "10:00");
    }
}
