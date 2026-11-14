//! Winamp `.eqf` / `winamp.q1` preset library format.
//!
//! Layout: 27-byte header `Winamp EQ library file v1.1`, then `1A ! - -`, then
//! one or more entries of 257-byte name field (NUL-terminated, zero padded) followed
//! by 11 bytes: bands 60..16k, then preamp. Each stored byte is inverted:
//! `value = 64 - byte`.

use crate::model::{MAX_VALUE, MIN_VALUE};
use crate::presets::Preset;
use std::fmt;

const HEADER: &[u8; 27] = b"Winamp EQ library file v1.1";
const MAGIC_TAIL: [u8; 4] = [0x1A, b'!', b'-', b'-'];
const PREFIX_LEN: usize = HEADER.len() + MAGIC_TAIL.len();
const NAME_LEN: usize = 257;
const NAME_MAX_BYTES: usize = 256;
const VALUES_LEN: usize = 11;
const ENTRY_LEN: usize = NAME_LEN + VALUES_LEN;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EqfEntry {
    pub name: String,
    pub preamp: u8,
    pub bands: [u8; 10],
}

impl From<&Preset> for EqfEntry {
    fn from(p: &Preset) -> Self {
        EqfEntry { name: p.name.to_string(), preamp: p.preamp, bands: p.bands }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EqfError {
    /// The file does not start with the Winamp EQ library header.
    BadHeader,
    /// An entry is cut short.
    Truncated,
    /// The header is valid but the file holds no entries.
    Empty,
}

impl fmt::Display for EqfError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EqfError::BadHeader => write!(f, "not a Winamp EQ library file (bad header)"),
            EqfError::Truncated => write!(f, "truncated Winamp EQ library entry"),
            EqfError::Empty => write!(f, "Winamp EQ library contains no entries"),
        }
    }
}

impl std::error::Error for EqfError {}

fn clamp_value(v: i32) -> u8 {
    v.clamp(i32::from(MIN_VALUE), i32::from(MAX_VALUE)) as u8
}

/// Parses a `.eqf` file image.
pub fn parse(bytes: &[u8]) -> Result<Vec<EqfEntry>, EqfError> {
    if !bytes.starts_with(HEADER) || bytes.len() < PREFIX_LEN || bytes[HEADER.len()..PREFIX_LEN] != MAGIC_TAIL {
        return Err(EqfError::BadHeader);
    }
    let mut rest = &bytes[PREFIX_LEN..];
    let mut entries = Vec::new();
    while !rest.is_empty() {
        if rest.len() < ENTRY_LEN {
            return Err(EqfError::Truncated);
        }
        let (name_field, after_name) = rest.split_at(NAME_LEN);
        let (values, _) = after_name.split_at(VALUES_LEN);
        let name_end = name_field.iter().position(|&b| b == 0).unwrap_or(NAME_LEN);
        let name = String::from_utf8_lossy(&name_field[..name_end]).into_owned();
        let val = |i: usize| clamp_value(64 - i32::from(values[i]));
        let mut bands = [0u8; 10];
        for (i, b) in bands.iter_mut().enumerate() {
            *b = val(i);
        }
        entries.push(EqfEntry { name, preamp: val(10), bands });
        rest = &rest[ENTRY_LEN..];
    }
    if entries.is_empty() {
        return Err(EqfError::Empty);
    }
    Ok(entries)
}

/// Serializes entries into a `.eqf` file image. Names longer than 256 bytes are
/// truncated at a character boundary.
pub fn write(entries: &[EqfEntry]) -> Vec<u8> {
    let mut out = Vec::with_capacity(PREFIX_LEN + entries.len() * ENTRY_LEN);
    out.extend_from_slice(HEADER);
    out.extend_from_slice(&MAGIC_TAIL);
    for e in entries {
        let mut end = e.name.len().min(NAME_MAX_BYTES);
        while !e.name.is_char_boundary(end) {
            end -= 1;
        }
        let mut field = [0u8; NAME_LEN];
        field[..end].copy_from_slice(&e.name.as_bytes()[..end]);
        out.extend_from_slice(&field);
        for &v in e.bands.iter().chain(std::iter::once(&e.preamp)) {
            out.push((64 - i32::from(clamp_value(i32::from(v)))) as u8);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::presets::BUILTIN;

    fn single(name: &str, preamp_byte: u8, band_bytes: [u8; 10]) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend_from_slice(HEADER);
        v.extend_from_slice(&MAGIC_TAIL);
        let mut field = [0u8; NAME_LEN];
        field[..name.len()].copy_from_slice(name.as_bytes());
        v.extend_from_slice(&field);
        v.extend_from_slice(&band_bytes);
        v.push(preamp_byte);
        v
    }

    #[test]
    fn write_then_parse_round_trips_builtins() {
        let entries: Vec<EqfEntry> = BUILTIN.iter().map(EqfEntry::from).collect();
        let bytes = write(&entries);
        assert_eq!(parse(&bytes).unwrap(), entries);
    }

    #[test]
    fn single_entry_is_299_bytes() {
        let e = [EqfEntry { name: "Flat".into(), preamp: 33, bands: [33; 10] }];
        assert_eq!(write(&e).len(), 31 + 257 + 11);
        assert_eq!(write(&e).len(), 299);
    }

    #[test]
    fn raw_byte_31_is_33() {
        let bytes = single("Zero", 31, [31; 10]);
        let entries = parse(&bytes).unwrap();
        assert_eq!(entries[0].name, "Zero");
        assert_eq!(entries[0].preamp, 33);
        assert_eq!(entries[0].bands, [33; 10]);
    }

    #[test]
    fn raw_byte_0_is_plus_12_and_63_is_minus_12() {
        let bytes = single("Edge", 0, [63; 10]);
        let entries = parse(&bytes).unwrap();
        assert_eq!(entries[0].preamp, 64);
        assert_eq!(entries[0].bands, [1; 10]);
    }

    #[test]
    fn bad_header_errors() {
        let mut bytes = write(&[EqfEntry { name: "x".into(), preamp: 33, bands: [33; 10] }]);
        bytes[0] = b'X';
        assert_eq!(parse(&bytes), Err(EqfError::BadHeader));
        assert_eq!(parse(b"hello"), Err(EqfError::BadHeader));
    }

    #[test]
    fn truncated_errors() {
        let bytes = write(&[EqfEntry { name: "x".into(), preamp: 33, bands: [33; 10] }]);
        assert_eq!(parse(&bytes[..bytes.len() - 1]), Err(EqfError::Truncated));
    }

    #[test]
    fn header_only_is_empty_error() {
        let bytes = write(&[]);
        assert_eq!(parse(&bytes), Err(EqfError::Empty));
    }

    #[test]
    fn long_names_are_truncated_to_256_bytes() {
        let long = "a".repeat(300);
        let bytes = write(&[EqfEntry { name: long, preamp: 33, bands: [33; 10] }]);
        let back = parse(&bytes).unwrap();
        assert_eq!(back[0].name.len(), 256);
    }
}
