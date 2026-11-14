//! Text-file parsing for `VISCOLOR.TXT`, `PLEDIT.TXT` and `REGION.TXT`.
//!
//! Rules follow Webamp (MIT, © Jordan Eldredge): `utils.ts` (`parseIni`),
//! `skinParserUtils.ts` (`getPlaylistStyle`), `utils.ts` (`parseViscolors`) and
//! `regionParser.ts`.

use crate::bitmap::argb;
use std::collections::BTreeMap;

/// Winamp's default visualiser colours (`0xFFRRGGBB`), as in Webamp's base skin.
pub const DEFAULT_VISCOLORS: [u32; 24] = [
    argb(0xFF, 0, 0, 0),
    argb(0xFF, 24, 33, 41),
    argb(0xFF, 239, 49, 16),
    argb(0xFF, 206, 41, 16),
    argb(0xFF, 214, 90, 0),
    argb(0xFF, 214, 102, 0),
    argb(0xFF, 214, 115, 0),
    argb(0xFF, 198, 123, 8),
    argb(0xFF, 222, 165, 24),
    argb(0xFF, 214, 181, 33),
    argb(0xFF, 189, 222, 41),
    argb(0xFF, 148, 222, 33),
    argb(0xFF, 41, 206, 16),
    argb(0xFF, 50, 190, 16),
    argb(0xFF, 57, 181, 16),
    argb(0xFF, 49, 156, 8),
    argb(0xFF, 41, 148, 0),
    argb(0xFF, 24, 132, 8),
    argb(0xFF, 255, 255, 255),
    argb(0xFF, 214, 214, 222),
    argb(0xFF, 181, 189, 189),
    argb(0xFF, 160, 170, 175),
    argb(0xFF, 148, 156, 165),
    argb(0xFF, 150, 150, 150),
];

/// Playlist editor colours from `PLEDIT.TXT` `[Text]` (`0xFFRRGGBB`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlEditColors {
    pub normal: u32,
    pub current: u32,
    pub normal_bg: u32,
    pub selected_bg: u32,
    pub font: String,
}

impl Default for PlEditColors {
    fn default() -> Self {
        PlEditColors {
            normal: argb(0xFF, 0x00, 0xFF, 0x00),
            current: argb(0xFF, 0xFF, 0xFF, 0xFF),
            normal_bg: argb(0xFF, 0x00, 0x00, 0x00),
            selected_bg: argb(0xFF, 0x00, 0x00, 0xFF),
            font: "Arial".to_string(),
        }
    }
}

/// Region polygons per section (lowercased section name), in window pixels.
pub type Regions = BTreeMap<String, Vec<Vec<(i32, i32)>>>;

/// Parsed INI: lowercased section → lowercased key → value.
pub type Ini = BTreeMap<String, BTreeMap<String, String>>;

/// Parses INI text the way Webamp does: lines before the first section are
/// ignored, keys and sections are lowercased, values are trimmed, anything
/// after a second `=` is dropped and one pair of surrounding quotes is removed.
pub fn parse_ini(text: &str) -> Ini {
    let mut out = Ini::new();
    let mut section: Option<String> = None;
    for raw in text.split(['\r', '\n']) {
        if raw.trim().is_empty() {
            continue;
        }
        if let Some((key, value)) = property(raw) {
            if let Some(name) = &section {
                out.entry(name.clone()).or_default().insert(key, value);
            }
            continue;
        }
        if let Some(name) = section_name(raw) {
            out.entry(name.clone()).or_default();
            section = Some(name);
        }
    }
    out
}

fn property(raw: &str) -> Option<(String, String)> {
    if raw.trim_start().starts_with(';') {
        return None;
    }
    let eq = raw.find('=')?;
    let key = raw[..eq].trim().to_lowercase();
    if key.is_empty() {
        return None;
    }
    let rest = raw[eq + 1..].trim_start();
    let rest = rest.split('=').next().unwrap_or("").trim();
    let rest = rest.strip_prefix(['"', '\'']).unwrap_or(rest);
    let rest = rest.strip_suffix(['"', '\'']).unwrap_or(rest);
    Some((key, rest.to_string()))
}

fn section_name(raw: &str) -> Option<String> {
    let t = raw.trim();
    let inner = t.strip_prefix('[')?.strip_suffix(']')?;
    if inner.is_empty() {
        return None;
    }
    Some(inner.trim().to_lowercase())
}

/// Parses `r,g,b` from the start of a line (`^\s*(\d+)\s*,?\s*(\d+)\s*,?\s*(\d+)`).
fn leading_triplet(line: &str) -> Option<(u8, u8, u8)> {
    let mut rest = line.trim_start();
    let mut vals = [0u32; 3];
    for (i, slot) in vals.iter_mut().enumerate() {
        let end = rest
            .find(|c: char| !c.is_ascii_digit())
            .unwrap_or(rest.len());
        if end == 0 {
            return None;
        }
        *slot = rest[..end].parse().ok()?;
        rest = rest[end..].trim_start();
        if i < 2 {
            rest = rest.strip_prefix(',').unwrap_or(rest).trim_start();
        }
    }
    let clamp = |v: u32| v.min(255) as u8;
    Some((clamp(vals[0]), clamp(vals[1]), clamp(vals[2])))
}

/// Parses `VISCOLOR.TXT`. Only lines that start with an `r,g,b` triple count
/// (comments are skipped); the first 24 such lines override the defaults.
pub fn parse_viscolor(text: &str) -> [u32; 24] {
    let mut out = DEFAULT_VISCOLORS;
    for (slot, (r, g, b)) in out.iter_mut().zip(text.lines().filter_map(leading_triplet)) {
        *slot = argb(0xFF, r, g, b);
    }
    out
}

fn parse_hex_color(value: &str) -> Option<u32> {
    let hex = value.strip_prefix('#').unwrap_or(value);
    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    let n = u32::from_str_radix(hex, 16).ok()?;
    Some(0xFF00_0000 | n)
}

/// Parses `PLEDIT.TXT`. Missing or malformed entries fall back to defaults.
pub fn parse_pledit(text: &str) -> PlEditColors {
    let ini = parse_ini(text);
    let mut out = PlEditColors::default();
    let Some(section) = ini.get("text") else {
        return out;
    };
    let color = |key: &str, slot: &mut u32| {
        if let Some(c) = section.get(key).and_then(|v| parse_hex_color(v)) {
            *slot = c;
        }
    };
    color("normal", &mut out.normal);
    color("current", &mut out.current);
    color("normalbg", &mut out.normal_bg);
    color("selectedbg", &mut out.selected_bg);
    if let Some(font) = section.get("font").filter(|f| !f.is_empty()) {
        out.font = font.clone();
    }
    out
}

/// Parses `REGION.TXT` (`NumPoints` / `PointList` per section) into polygons.
/// Polygons with fewer than three points are skipped, as in Webamp.
pub fn parse_region(text: &str) -> Regions {
    let mut out = Regions::new();
    for (name, keys) in parse_ini(text) {
        let (Some(counts), Some(list)) = (keys.get("numpoints"), keys.get("pointlist")) else {
            continue;
        };
        if counts.is_empty() || list.is_empty() {
            continue;
        }
        let nums: Vec<i32> = list
            .split(|c: char| c == ',' || c.is_whitespace())
            .filter(|s| !s.is_empty())
            .filter_map(|s| s.parse().ok())
            .collect();
        let points: Vec<(i32, i32)> = nums.chunks_exact(2).map(|p| (p[0], p[1])).collect();

        let mut idx = 0usize;
        let mut polygons = Vec::new();
        for count in counts.split(',').map(str::trim).filter(|s| !s.is_empty()) {
            let Ok(num) = count.parse::<usize>() else {
                continue;
            };
            if num < 3 {
                idx += num;
                continue;
            }
            let end = (idx + num).min(points.len());
            if idx >= end {
                continue;
            }
            polygons.push(points[idx..end].to_vec());
            idx += num;
        }
        if !polygons.is_empty() {
            out.insert(name, polygons);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn viscolor_skips_comments_and_keeps_defaults_for_the_rest() {
        let out = parse_viscolor("// header\r\n255,0,0\r\n 1 2 3 \r\nnot a colour\n");
        assert_eq!(out[0], argb(0xFF, 255, 0, 0));
        assert_eq!(out[1], argb(0xFF, 1, 2, 3));
        assert_eq!(out[2], DEFAULT_VISCOLORS[2]);
        assert_eq!(out[23], DEFAULT_VISCOLORS[23]);
    }

    #[test]
    fn viscolor_empty_text_is_all_defaults() {
        assert_eq!(parse_viscolor(""), DEFAULT_VISCOLORS);
    }

    #[test]
    fn pledit_parses_colours_and_font() {
        let p = parse_pledit("[Text]\nNormal=#112233\ncurrent=ABCDEF\nfont=Verdana\n");
        assert_eq!(p.normal, argb(0xFF, 0x11, 0x22, 0x33));
        assert_eq!(p.current, argb(0xFF, 0xAB, 0xCD, 0xEF));
        assert_eq!(p.font, "Verdana");
        assert_eq!(p.normal_bg, PlEditColors::default().normal_bg);
        assert_eq!(p.selected_bg, PlEditColors::default().selected_bg);
    }

    #[test]
    fn pledit_missing_or_bad_values_use_defaults() {
        assert_eq!(parse_pledit(""), PlEditColors::default());
        let p = parse_pledit("[text]\nnormal=zz\n");
        assert_eq!(p, PlEditColors::default());
    }

    #[test]
    fn ini_lowercases_and_ignores_text_before_first_section() {
        let ini = parse_ini("stray=1\n[Section One]\nKey = \"Value\" = junk\n");
        assert_eq!(ini["section one"]["key"], "Value");
        assert!(!ini.contains_key("stray"));
    }

    #[test]
    fn region_builds_polygons_and_skips_short_ones() {
        let text =
            "[Normal]\nNumPoints=4,2,3\nPointList=0,0 10,0 10,10 0,10 5,5 6,6 7,7 8,8 9,9 1,1\n";
        let regions = parse_region(text);
        let polys = &regions["normal"];
        assert_eq!(polys.len(), 2);
        assert_eq!(polys[0], vec![(0, 0), (10, 0), (10, 10), (0, 10)]);
        // The 2-point polygon consumes its two pairs (5,5),(6,6) without
        // emitting anything, so the 3-point polygon starts at pair index 6.
        assert_eq!(polys[1], vec![(7, 7), (8, 8), (9, 9)]);
    }

    #[test]
    fn region_section_without_points_is_absent() {
        assert!(parse_region("[Empty]\nNumPoints=3\n").is_empty());
    }
}
