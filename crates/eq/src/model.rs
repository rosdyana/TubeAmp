//! Winamp slider model.
//!
//! Slider values are `u8` in `1..=64` (the scale used by `.eqf` files after
//! inversion). 33 is 0 dB. The mapping is piecewise linear: values at or below
//! 33 span -12..0 dB over 32 steps, values above 33 span 0..+12 dB over 31 steps.

/// Centre frequencies of the ten bands, in Hz.
pub const BANDS_HZ: [f32; 10] = [60., 170., 310., 600., 1000., 3000., 6000., 12000., 14000., 16000.];

/// Slider value that means 0 dB.
pub const ZERO_VALUE: u8 = 33;
/// Smallest slider value (-12 dB).
pub const MIN_VALUE: u8 = 1;
/// Largest slider value (+12 dB).
pub const MAX_VALUE: u8 = 64;

/// Maps a Winamp slider value (clamped to `1..=64`) to dB.
pub fn value_to_db(v: u8) -> f32 {
    let v = v.clamp(MIN_VALUE, MAX_VALUE);
    let d = f32::from(v) - f32::from(ZERO_VALUE);
    if v <= ZERO_VALUE {
        d * 12.0 / 32.0
    } else {
        d * 12.0 / 31.0
    }
}

/// Maps dB to the nearest Winamp slider value. `db` is clamped to +-12.
pub fn db_to_value(db: f32) -> u8 {
    if db.is_nan() {
        return ZERO_VALUE;
    }
    let db = db.clamp(-12.0, 12.0);
    let v = if db <= 0.0 {
        f32::from(ZERO_VALUE) + db * 32.0 / 12.0
    } else {
        f32::from(ZERO_VALUE) + db * 31.0 / 12.0
    };
    v.round().clamp(f32::from(MIN_VALUE), f32::from(MAX_VALUE)) as u8
}

/// Complete EQ state as seen by the user.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EqSettings {
    pub on: bool,
    pub auto: bool,
    pub preamp: u8,
    pub bands: [u8; 10],
}

impl Default for EqSettings {
    fn default() -> Self {
        EqSettings { on: true, auto: false, preamp: ZERO_VALUE, bands: [ZERO_VALUE; 10] }
    }
}

impl EqSettings {
    /// Gain of band `i` in dB.
    pub fn band_db(&self, i: usize) -> f32 {
        value_to_db(self.bands[i])
    }

    /// Preamp gain in dB.
    pub fn preamp_db(&self) -> f32 {
        value_to_db(self.preamp)
    }

    /// Settings for a built-in or loaded preset. Keeps `on = true`, `auto = false`.
    pub fn from_preset(p: &crate::presets::Preset) -> Self {
        EqSettings { on: true, auto: false, preamp: p.preamp, bands: p.bands }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_to_db_anchor_points() {
        assert_eq!(value_to_db(1), -12.0);
        assert_eq!(value_to_db(33), 0.0);
        assert_eq!(value_to_db(64), 12.0);
    }

    #[test]
    fn value_to_db_clamps_out_of_range() {
        assert_eq!(value_to_db(0), -12.0);
        assert_eq!(value_to_db(200), 12.0);
    }

    #[test]
    fn db_to_value_round_trips_every_value() {
        for v in 1..=64u8 {
            assert_eq!(db_to_value(value_to_db(v)), v, "value {v}");
        }
    }

    #[test]
    fn db_to_value_clamps() {
        assert_eq!(db_to_value(-100.0), 1);
        assert_eq!(db_to_value(100.0), 64);
        assert_eq!(db_to_value(f32::NAN), 33);
    }

    #[test]
    fn default_is_flat_and_on() {
        let s = EqSettings::default();
        assert!(s.on);
        assert!(!s.auto);
        assert_eq!(s.preamp, 33);
        assert_eq!(s.bands, [33; 10]);
        assert_eq!(s.band_db(4), 0.0);
        assert_eq!(s.preamp_db(), 0.0);
    }
}
