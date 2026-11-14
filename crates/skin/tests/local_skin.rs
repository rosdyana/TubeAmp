//! Loads a real classic skin from disk. Ignored by default because the skin is
//! a local file, not part of the repository (`*.wsz` is gitignored).
//!
//! Run with `TUBEAMP_TEST_WSZ=/path/to/base-2.91.wsz cargo test -p tubeamp-skin -- --ignored`.

use std::path::PathBuf;
use tubeamp_skin::{Sheet, Skin};

#[test]
#[ignore = "needs a local .wsz skin (base-2.91.wsz); run with --ignored"]
fn loads_base_2_91_skin_when_present() {
    let Some(path) = std::env::var_os("TUBEAMP_TEST_WSZ").map(PathBuf::from) else {
        eprintln!("skipping: set TUBEAMP_TEST_WSZ to a classic .wsz skin");
        return;
    };
    if !path.exists() {
        eprintln!("skipping: {} does not exist", path.display());
        return;
    }

    let skin = Skin::load_path(&path).expect("base skin loads");

    let main = skin.sheet(Sheet::Main).expect("MAIN.BMP");
    assert_eq!((main.w, main.h), (275, 116));
    assert!(skin.sheet(Sheet::Titlebar).is_some());
    assert!(skin.sheet(Sheet::Cbuttons).is_some());
    assert!(skin.sheet(Sheet::Numbers).is_some());
    assert!(skin.sheet(Sheet::Text).is_some());

    // base-2.91 ships no NUMS_EX.BMP, so the digit lookup must fall back to NUMBERS.
    if !skin.has_nums_ex() {
        let digit = skin
            .sprite(Sheet::NumsEx, "DIGIT_8_EX")
            .expect("fallback digit");
        assert_eq!((digit.w, digit.h), (9, 13));
    }

    let play = skin
        .sprite(Sheet::Cbuttons, "MAIN_PLAY_BUTTON")
        .expect("play sprite");
    assert_eq!((play.x, play.y, play.w, play.h), (23, 0, 23, 18));

    // base-2.91's VISCOLOR.TXT lists the same 24 colours as Winamp's defaults,
    // with `//` comments after each value; all 24 must be read.
    assert_eq!(skin.viscolor, tubeamp_skin::parse::DEFAULT_VISCOLORS);
    // base-2.91's REGION.TXT is all `;` comments and defines no active sections.
    assert!(skin.regions.is_empty());
}
