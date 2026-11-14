//! Winamp-2.x-accurate 10-band equalizer.
//!
//! - [`model`]: slider scale (1..=64, 33 = 0 dB), band frequencies, [`EqSettings`].
//! - [`presets`]: the 17 built-in Winamp presets.
//! - [`eqf`]: `.eqf` / `winamp.q1` file reader and writer.
//! - [`dsp`]: lock-free shared parameters and the realtime processor.

pub mod dsp;
pub mod eqf;
pub mod model;
pub mod presets;

pub use dsp::{response_db, EqParams, EqProcessor};
pub use eqf::{EqfEntry, EqfError};
pub use model::{db_to_value, value_to_db, EqSettings, BANDS_HZ};
pub use presets::{Preset, BUILTIN};
