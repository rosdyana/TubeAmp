//! Text for native menu items. muda reads a single `&` as a mnemonic marker (`Full &Bass` shows as
//! `Full Bass`), so a label that comes from data (a preset name, a skin's name, a track title) has
//! to double its ampersands to show them as written.

/// `label` as a menu item's text: every `&` doubled, so that muda shows it unchanged.
pub fn escape(label: &str) -> String {
    label.replace('&', "&&")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_ampersand_is_doubled_so_it_shows_as_written() {
        assert_eq!(escape("Full Bass & Treble"), "Full Bass && Treble");
        assert_eq!(escape("R&B&&Soul"), "R&&B&&&&Soul");
    }

    #[test]
    fn text_without_an_ampersand_is_unchanged() {
        assert_eq!(escape("Rock"), "Rock");
        assert_eq!(escape(""), "");
    }

    #[test]
    fn escaped_text_reads_back_as_the_original_in_muda() {
        // muda's own rule: "&&" stands for one "&", and a lone "&" is a mnemonic marker that is
        // dropped. The escaped text must come back unchanged under that rule.
        fn strip(text: &str) -> String {
            let mut out = String::new();
            let mut chars = text.chars().peekable();
            while let Some(c) = chars.next() {
                if c == '&' {
                    if chars.peek() == Some(&'&') {
                        chars.next();
                        out.push('&');
                    }
                } else {
                    out.push(c);
                }
            }
            out
        }
        for label in ["Full Bass & Treble", "R&B&&Soul", "plain", "&"] {
            assert_eq!(strip(&escape(label)), label);
        }
    }
}
