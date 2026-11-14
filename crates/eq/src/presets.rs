//! The 17 built-in Winamp 2.x presets. Band order: 60, 170, 310, 600, 1k, 3k, 6k, 12k, 14k, 16k.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Preset {
    pub name: &'static str,
    pub preamp: u8,
    pub bands: [u8; 10],
}

pub const BUILTIN: [Preset; 17] = [
    Preset { name: "Classical", preamp: 33, bands: [33, 33, 33, 33, 33, 33, 20, 20, 20, 16] },
    Preset { name: "Club", preamp: 33, bands: [33, 33, 38, 42, 42, 42, 38, 33, 33, 33] },
    Preset { name: "Dance", preamp: 33, bands: [48, 44, 36, 32, 32, 22, 20, 20, 32, 32] },
    Preset { name: "Laptop speakers/headphones", preamp: 33, bands: [40, 50, 41, 26, 28, 35, 40, 48, 53, 56] },
    Preset { name: "Large hall", preamp: 33, bands: [49, 49, 42, 42, 33, 24, 24, 24, 33, 33] },
    Preset { name: "Party", preamp: 33, bands: [44, 44, 33, 33, 33, 33, 33, 33, 44, 44] },
    Preset { name: "Pop", preamp: 33, bands: [29, 40, 44, 45, 41, 30, 28, 28, 29, 29] },
    Preset { name: "Reggae", preamp: 33, bands: [33, 33, 31, 22, 33, 43, 43, 33, 33, 33] },
    Preset { name: "Rock", preamp: 33, bands: [45, 40, 23, 19, 26, 39, 47, 50, 50, 50] },
    Preset { name: "Soft", preamp: 33, bands: [40, 35, 30, 28, 30, 39, 46, 48, 50, 52] },
    Preset { name: "Ska", preamp: 33, bands: [28, 24, 25, 31, 39, 42, 47, 48, 50, 48] },
    Preset { name: "Full Bass", preamp: 33, bands: [48, 48, 48, 42, 35, 25, 18, 15, 14, 14] },
    Preset { name: "Soft Rock", preamp: 33, bands: [39, 39, 36, 31, 25, 23, 26, 31, 37, 47] },
    Preset { name: "Full Treble", preamp: 33, bands: [16, 16, 16, 25, 37, 50, 58, 58, 58, 60] },
    Preset { name: "Full Bass & Treble", preamp: 33, bands: [44, 42, 33, 20, 24, 35, 46, 50, 52, 52] },
    Preset { name: "Live", preamp: 33, bands: [24, 33, 39, 41, 42, 42, 39, 37, 37, 36] },
    Preset { name: "Techno", preamp: 33, bands: [45, 42, 33, 23, 24, 33, 45, 48, 48, 47] },
];

/// Looks up a built-in preset by name (case-insensitive).
pub fn find(name: &str) -> Option<&'static Preset> {
    BUILTIN.iter().find(|p| p.name.eq_ignore_ascii_case(name))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn all_values_in_slider_range() {
        assert_eq!(BUILTIN.len(), 17);
        for p in &BUILTIN {
            assert!((1..=64).contains(&p.preamp), "{} preamp", p.name);
            for &v in &p.bands {
                assert!((1..=64).contains(&v), "{} band value {v}", p.name);
            }
        }
    }

    #[test]
    fn find_is_case_insensitive() {
        let p = find("full bass").expect("full bass");
        assert_eq!(p.name, "Full Bass");
        assert_eq!(find("FULL TREBLE").map(|p| p.name), Some("Full Treble"));
        assert!(find("nope").is_none());
    }
}
