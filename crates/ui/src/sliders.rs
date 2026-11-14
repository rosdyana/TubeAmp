//! Slider geometry for the main window: pointer position → value, and value → thumb position.
//!
//! Thumb widths and travel come from Webamp's CSS for the native range inputs. Pointer x values
//! are element-relative window pixels. The thumb is centred under the pointer while dragging.

/// Thumb width of the position bar.
pub const POSITION_THUMB: u32 = 29;
/// Thumb travel of the position bar (input width minus thumb width).
pub const POSITION_TRAVEL: u32 = 248 - POSITION_THUMB;
/// Thumb width of the volume bar.
pub const VOLUME_THUMB: u32 = 14;
/// Thumb travel of the volume bar.
pub const VOLUME_TRAVEL: u32 = 65 - VOLUME_THUMB;
/// Thumb width of the balance bar.
pub const BALANCE_THUMB: u32 = 14;
/// Thumb travel of the balance bar.
pub const BALANCE_TRAVEL: u32 = 38 - BALANCE_THUMB;
/// Balance values with `|b| < BALANCE_SNAP` snap to centre (Webamp `setBalance`).
pub const BALANCE_SNAP: i32 = 25;

/// `0.0..=1.0` for a thumb whose left edge is `left` pixels along a track of `travel` pixels.
fn fraction(left: f64, travel: u32) -> f64 {
    (left / f64::from(travel)).clamp(0.0, 1.0)
}

/// Position-bar fraction (`0.0..=1.0`) for an element-relative pointer x.
pub fn position_fraction(x: f64) -> f32 {
    fraction(x - f64::from(POSITION_THUMB) / 2.0, POSITION_TRAVEL) as f32
}

/// Whole percent (`0..=100`) of a position fraction, as shown in the seek marquee.
pub fn percent(fraction: f32) -> u32 {
    (fraction.clamp(0.0, 1.0) * 100.0).round() as u32
}

/// Thumb left edge (window pixels) of the position bar at `fraction`.
pub fn position_x(fraction: f32) -> i32 {
    (fraction.clamp(0.0, 1.0) * POSITION_TRAVEL as f32).round() as i32
}

/// Volume (`0..=100`) for an element-relative pointer x.
pub fn volume_from_x(x: f64) -> u8 {
    (fraction(x - f64::from(VOLUME_THUMB) / 2.0, VOLUME_TRAVEL) * 100.0).round() as u8
}

/// Thumb left edge (element-relative) of the volume bar. Matches how the main window draws it.
pub fn volume_x(volume: u8) -> i32 {
    ((u32::from(volume.min(100)) * VOLUME_TRAVEL + 50) / 100) as i32
}

/// Balance (`-100..=100`, snapped to 0 near the centre) for an element-relative pointer x.
pub fn balance_from_x(x: f64) -> i8 {
    let f = fraction(x - f64::from(BALANCE_THUMB) / 2.0, BALANCE_TRAVEL);
    let b = (f * 200.0 - 100.0).round() as i32;
    snap_balance(b.clamp(-100, 100))
}

/// Snaps a balance to centre inside the dead zone.
pub fn snap_balance(b: i32) -> i8 {
    if b.abs() < BALANCE_SNAP {
        0
    } else {
        b as i8
    }
}

/// Thumb left edge (element-relative) of the balance bar. Matches how the main window draws it.
pub fn balance_x(balance: i8) -> i32 {
    ((i32::from(balance.clamp(-100, 100)) + 100) * BALANCE_TRAVEL as i32 + 100) / 200
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn position_maps_the_track_ends_and_middle() {
        let half = f64::from(POSITION_THUMB) / 2.0;
        assert_eq!(position_fraction(0.0), 0.0);
        assert_eq!(position_fraction(half), 0.0);
        assert_eq!(position_fraction(half + f64::from(POSITION_TRAVEL)), 1.0);
        assert_eq!(percent(position_fraction(half + 109.5)), 50);
        assert_eq!(percent(position_fraction(10_000.0)), 100);
    }

    #[test]
    fn position_round_trips_to_a_whole_percent() {
        for p in 0..=100u32 {
            let f = p as f32 / 100.0;
            let x = f64::from(position_x(f)) + f64::from(POSITION_THUMB) / 2.0;
            let back = percent(position_fraction(x));
            assert!(back.abs_diff(p) <= 1, "{p}% came back as {back}%");
        }
    }

    #[test]
    fn volume_maps_pointer_to_value_and_back() {
        let half = f64::from(VOLUME_THUMB) / 2.0;
        assert_eq!(volume_from_x(-50.0), 0);
        assert_eq!(volume_from_x(half), 0);
        assert_eq!(volume_from_x(half + f64::from(VOLUME_TRAVEL)), 100);
        assert_eq!(volume_from_x(half + f64::from(VOLUME_TRAVEL) / 2.0), 50);
        for v in 0..=100u8 {
            let x = f64::from(volume_x(v)) + half;
            let back = volume_from_x(x);
            assert!(back.abs_diff(v) <= 1, "{v} came back as {back}");
        }
    }

    #[test]
    fn balance_maps_pointer_ends_and_centre() {
        let half = f64::from(BALANCE_THUMB) / 2.0;
        let travel = f64::from(BALANCE_TRAVEL);
        assert_eq!(balance_from_x(0.0), -100);
        assert_eq!(balance_from_x(half + travel), 100);
        assert_eq!(balance_from_x(half + travel / 2.0), 0);
    }

    #[test]
    fn balance_snaps_to_centre_inside_the_dead_zone() {
        assert_eq!(snap_balance(24), 0);
        assert_eq!(snap_balance(-24), 0);
        assert_eq!(snap_balance(25), 25);
        assert_eq!(snap_balance(-25), -25);
        let half = f64::from(BALANCE_THUMB) / 2.0;
        // Fraction 0.6 is balance +20 before snapping.
        assert_eq!(balance_from_x(half + 0.6 * f64::from(BALANCE_TRAVEL)), 0);
        // Fraction 0.65 is +30, which is outside the dead zone.
        assert_eq!(balance_from_x(half + 0.65 * f64::from(BALANCE_TRAVEL)), 30);
    }

    #[test]
    fn balance_round_trips_outside_the_dead_zone() {
        for b in (-100..=100i32).filter(|b| b.abs() >= 30) {
            let x = f64::from(balance_x(b as i8)) + f64::from(BALANCE_THUMB) / 2.0;
            let back = i32::from(balance_from_x(x));
            // 24 px span 200 units, so each pixel is about 8 units.
            assert!((back - b).abs() <= 5, "{b} came back as {back}");
        }
        assert_eq!(balance_x(-100), 0);
        assert_eq!(balance_x(100), BALANCE_TRAVEL as i32);
    }
}
