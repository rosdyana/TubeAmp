//! The classic Winamp skin, offered on the first run and from the skin browser.
//!
//! TubeAmp never ships this file. It is the user's own copy, downloaded for personal use from a
//! public source into the skins folder, and checked against its SHA-256 before it is used. The
//! download runs on a background thread (see `window/session.rs`), and the answer is remembered in
//! the config, so the question is asked once.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::config::write_atomic;

/// The file name in the skins folder.
pub const FILE_NAME: &str = "Winamp Classic (base-2.91).wsz";
/// The SHA-256 of `base-2.91.wsz`, lowercase hex.
pub const SHA256: &str = "cb2e5361a5208878855ae2c6c4a842df6cb71590b2c4405fe8776258c82e59bb";
/// Tried first.
pub const PRIMARY_URL: &str = "https://r2.webampskins.org/skins/2f245e741b6bd4e1a122a28489f0c7b7.wsz";
/// Tried when the primary fails or has the wrong checksum.
pub const FALLBACK_URL: &str =
    "https://raw.githubusercontent.com/captbaritone/webamp/master/packages/webamp/assets/skins/base-2.91.wsz";
/// Largest download accepted, in bytes. The skin is a few tens of kilobytes.
const MAX_BYTES: u64 = 20 * 1024 * 1024;

/// The first-run question's title and text.
pub const PROMPT_TITLE: &str = "Use the classic Winamp skin?";
pub const PROMPT_TEXT: &str = "TubeAmp can download Winamp 2.91's original skin from the Winamp Skin Museum for your personal use. (TubeAmp does not include it.)";

/// The SHA-256 of `bytes`, as lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Whether `bytes` hash to `expected` (hex, any case).
pub fn matches_sha256(bytes: &[u8], expected: &str) -> bool {
    sha256_hex(bytes).eq_ignore_ascii_case(expected)
}

/// Downloads `url`. The request carries only a plain `User-Agent: TubeAmp`.
fn fetch(url: &str) -> Result<Vec<u8>, String> {
    let mut response = ureq::get(url)
        .header("User-Agent", "TubeAmp")
        .call()
        .map_err(|e| e.to_string())?;
    response
        .body_mut()
        .with_config()
        .limit(MAX_BYTES)
        .read_to_vec()
        .map_err(|e| e.to_string())
}

/// Puts the classic skin into `dir` and returns its path. A copy already there that checks out is
/// used as it is. Otherwise the primary source is tried, then the fallback. Each download must
/// match [`SHA256`] before it is written.
pub fn obtain(dir: &Path) -> Result<PathBuf, String> {
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
    let dest = dir.join(FILE_NAME);
    if let Ok(bytes) = std::fs::read(&dest) {
        if matches_sha256(&bytes, SHA256) {
            eprintln!("tubeamp: classic skin already in {}, sha256 ok", dest.display());
            return Ok(dest);
        }
    }
    for url in [PRIMARY_URL, FALLBACK_URL] {
        let bytes = match fetch(url) {
            Ok(bytes) => bytes,
            Err(e) => {
                eprintln!("tubeamp: classic skin download from {url} failed: {e}");
                continue;
            }
        };
        let digest = sha256_hex(&bytes);
        if !digest.eq_ignore_ascii_case(SHA256) {
            eprintln!(
                "tubeamp: classic skin from {url} has sha256 {digest}, expected {SHA256}; not used"
            );
            continue;
        }
        write_atomic(&dest, &bytes).map_err(|e| format!("could not save the classic skin: {e}"))?;
        eprintln!(
            "tubeamp: classic skin from {url}, {} bytes, sha256 ok; saved to {}",
            bytes.len(),
            dest.display()
        );
        return Ok(dest);
    }
    Err("could not download the classic skin".to_string())
}

/// What the first-run question came back with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Yes: download the classic skin.
    Yes,
    /// No: remembered, not asked again.
    No,
    /// Anything else (closed, cancelled, no answer): not remembered, so it is asked again next run.
    Unanswered,
}

/// Maps the dialog's result to an answer. Only an explicit Yes or No counts as an answer.
pub fn answer_from_result(result: &rfd::MessageDialogResult) -> Answer {
    match result {
        rfd::MessageDialogResult::Yes => Answer::Yes,
        rfd::MessageDialogResult::No => Answer::No,
        _ => Answer::Unanswered,
    }
}

/// Asks the first-run question.
pub fn ask() -> Answer {
    let result = rfd::MessageDialog::new()
        .set_title(PROMPT_TITLE)
        .set_description(PROMPT_TEXT)
        .set_buttons(rfd::MessageButtons::YesNo)
        .show();
    let answer = answer_from_result(&result);
    eprintln!("tubeamp: first-run dialog returned {result} ({answer:?})");
    answer
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sha256_matches_the_published_test_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn a_fake_payload_passes_only_with_its_own_digest() {
        let payload = b"abc";
        let expected = "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad";
        assert!(matches_sha256(payload, expected));
        assert!(matches_sha256(payload, &expected.to_uppercase()));
        // One byte different, or a different digest, fails.
        assert!(!matches_sha256(b"abd", expected));
        assert!(!matches_sha256(payload, SHA256));
        assert!(!matches_sha256(b"", expected));
    }

    #[test]
    fn only_an_explicit_yes_or_no_is_an_answer() {
        use rfd::MessageDialogResult as R;
        assert_eq!(answer_from_result(&R::Yes), Answer::Yes);
        assert_eq!(answer_from_result(&R::No), Answer::No);
        // A closed or cancelled dialog, or any other result, is not an answer: it is asked again.
        assert_eq!(answer_from_result(&R::Cancel), Answer::Unanswered);
        assert_eq!(answer_from_result(&R::Ok), Answer::Unanswered);
        assert_eq!(answer_from_result(&R::Custom("Later".into())), Answer::Unanswered);
        assert_eq!(answer_from_result(&R::default()), Answer::Unanswered);
    }

    #[test]
    fn the_classic_skin_constants_are_well_formed() {
        assert_eq!(SHA256.len(), 64);
        assert!(SHA256.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(PRIMARY_URL.starts_with("https://"));
        assert!(FALLBACK_URL.starts_with("https://"));
        assert!(FILE_NAME.ends_with(".wsz"));
    }
}
