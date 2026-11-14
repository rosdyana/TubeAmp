//! Winamp-style spectrum analyser and oscilloscope for the main window's vis area.
//!
//! The arithmetic follows Webamp's `js/components/VisPainter.ts` (MIT, © Jordan Eldredge) and its
//! `FFTNullsoft.ts` port of the Nullsoft FFT: the same envelope, equalise curve, 0.91 linear/log
//! blend, 4-column averaging ("wide" bars), Winamp falloff (12/16 rows per frame) and peak
//! falloff (1.1 per frame). Constants are per analysis step, and [`Vis::update`] is one step.
//!
//! Differences from Webamp:
//! - The FFT has 512 points (256 bins) instead of 1024 (512 bins), and every magnitude is doubled
//!   so a steady tone has the same height as in Webamp.
//! - The oscilloscope reads the latest 512 samples with a stride of 6, instead of 1024 bytes
//!   sliced to 576 with a stride of 7.

use std::f64::consts::{PI, TAU};
use std::sync::Arc;

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};

/// Columns in the vis area (Webamp paints 75 columns).
pub const COLUMNS: usize = 75;
/// Rows in the vis area. Heights and scope values are in `0..ROWS`.
pub const ROWS: u8 = 16;
/// Analysis window in samples.
pub const WINDOW: usize = 512;
/// FFT bins that are analysed.
const BINS: usize = WINDOW / 2;
/// Webamp's `Float32Array(76)`: one slot past the painted columns, which stays zero.
const SLOTS: usize = COLUMNS + 1;
/// Highest bar height (Webamp `maxHeight` for the main window).
const MAX_HEIGHT: i32 = ROWS as i32 - 1;
/// Webamp `saFalloff` "moderate" (12) divided by 16, per analysis step.
const FALLOFF_STEP: f32 = 12.0 / 16.0;
/// Webamp `saPeakFalloff` "slow".
const PEAK_FALLOFF: f32 = 1.1;
/// Peak drop when a peak is (re)set.
const PEAK_START_DROP: f32 = 3.0;
/// Cap on the growing peak drop. Beyond this the peak is already at zero, so the cap changes
/// nothing visible and it keeps the value finite.
const PEAK_DROP_MAX: f32 = 1.0e6;
/// Webamp's linear/log blend factor.
const SCALE: f32 = 0.91;
/// Compensates for the 512-point FFT (Webamp uses 1024 points).
const GAIN: f32 = 2.0;
/// Oscilloscope sample stride: the latest `WINDOW` samples are spread over `COLUMNS`.
const SCOPE_STRIDE: usize = WINDOW / COLUMNS;
/// Oscilloscope row for silence (byte 128, centred in the 16 rows).
const SILENT_SCOPE: u8 = 7;

/// One bar column of the spectrum.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Bar {
    /// Column height in rows, `0..=15`.
    pub height: u8,
    /// Peak marker row, `1..=15`, or `None` when the peak is hidden.
    pub peak: Option<u8>,
}

/// What the vis area shows for one frame.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VisFrame {
    /// Spectrum columns. Columns `4k+3` are the gaps between the 3-pixel bars.
    pub bars: [Bar; COLUMNS],
    /// Oscilloscope value per column, `0..=15`, where 0 is the top row.
    pub scope: [u8; COLUMNS],
}

impl Default for VisFrame {
    /// The silent frame: no bars, no peaks, scope centred.
    fn default() -> Self {
        VisFrame {
            bars: [Bar::default(); COLUMNS],
            scope: [SILENT_SCOPE; COLUMNS],
        }
    }
}

/// Spectrum and oscilloscope state. Feed it samples and call [`Vis::update`] once per frame.
pub struct Vis {
    /// Latest mono samples in `-1..=1`. `next` is the slot that will be overwritten, so it is
    /// also the oldest sample.
    ring: [f32; WINDOW],
    next: usize,
    fft: Arc<dyn Fft<f32>>,
    fft_buf: [Complex<f32>; WINDOW],
    envelope: [f32; WINDOW],
    equalize: [f32; BINS],
    sample: [f32; SLOTS],
    sa_data: [i32; SLOTS],
    sa_falloff: [f32; SLOTS],
    sa_peaks: [i32; SLOTS],
    sa_data2: [f32; SLOTS],
    bar_peak: [i32; SLOTS],
    frame: VisFrame,
}

impl Default for Vis {
    fn default() -> Self {
        Self::new()
    }
}

impl Vis {
    pub fn new() -> Self {
        let fft = FftPlanner::<f32>::new().plan_fft_forward(WINDOW);
        let mut envelope = [0.0f32; WINDOW];
        for (i, e) in envelope.iter_mut().enumerate() {
            // Webamp: 0.5 + 0.5 sin(2 pi i / N - pi / 2).
            *e = (0.5 + 0.5 * (i as f64 * TAU / WINDOW as f64 - PI / 2.0).sin()) as f32;
        }
        let mut equalize = [0.0f32; BINS];
        let mut bias = 0.04f64;
        for (i, eq) in equalize.iter_mut().enumerate() {
            let inv = (9.0 - bias) / BINS as f64;
            *eq = (1.0 + bias + (i as f64 + 1.0) * inv).log10() as f32;
            bias /= 1.0025;
        }
        Vis {
            ring: [0.0; WINDOW],
            next: 0,
            fft,
            fft_buf: [Complex::new(0.0, 0.0); WINDOW],
            envelope,
            equalize,
            sample: [0.0; SLOTS],
            sa_data: [0; SLOTS],
            sa_falloff: [0.0; SLOTS],
            sa_peaks: [0; SLOTS],
            sa_data2: [0.0; SLOTS],
            bar_peak: [0; SLOTS],
            frame: VisFrame::default(),
        }
    }

    /// Appends `samples` (mono, newest last) and performs one analysis step. Call once per
    /// displayed frame, even with no new samples, so that the bars keep falling.
    pub fn update(&mut self, samples: &[f32]) -> &VisFrame {
        for &s in samples {
            self.ring[self.next] = if s.is_finite() {
                s.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            self.next = (self.next + 1) % WINDOW;
        }
        self.analyse();
        &self.frame
    }

    /// The frame produced by the last [`Vis::update`].
    pub fn frame(&self) -> &VisFrame {
        &self.frame
    }

    fn analyse(&mut self) {
        self.spectrum();
        self.bars();
        self.scope();
    }

    /// Webamp's `processFFT` followed by its log-ish resampling into 75 columns.
    fn spectrum(&mut self) {
        for i in 0..WINDOW {
            let x = self.ring[(self.next + i) % WINDOW];
            // Webamp feeds (byte - 128) / 24 with the Hann-like envelope applied.
            let v = (to_byte(x) - 128.0) / 24.0 * self.envelope[i] * GAIN;
            self.fft_buf[i] = Complex::new(v, 0.0);
        }
        self.fft.process(&mut self.fft_buf);

        let mut mags = [0.0f32; BINS];
        for (i, m) in mags.iter_mut().enumerate() {
            *m = self.fft_buf[i].norm() * self.equalize[i];
        }

        let log_max = (BINS as f32).log10();
        let span = (COLUMNS - 1) as f32;
        for x in 0..COLUMNS {
            let linear = x as f32 / span * (BINS - 1) as f32;
            let logged = 10f32.powf(log_max * x as f32 / span);
            let scaled = (1.0 - SCALE) * linear + SCALE * logged;
            let i1 = (scaled.floor() as usize).min(BINS - 1);
            let i2 = (scaled.ceil() as usize).min(BINS - 1);
            self.sample[x] = if i1 == i2 {
                mags[i1]
            } else {
                let f2 = scaled - i1 as f32;
                (1.0 - f2) * mags[i1] + f2 * mags[i2]
            };
        }
    }

    /// Webamp's per-column bar, falloff and peak state machine (one step).
    fn bars(&mut self) {
        for x in 0..COLUMNS {
            // "Wide" bars: each group of 4 columns shares the average of its 4 samples.
            let chunk = x & !3;
            let avg = (self.sample[chunk]
                + self.sample[chunk + 1]
                + self.sample[chunk + 2]
                + self.sample[chunk + 3])
                / 4.0;
            // Webamp stores this in an Int16Array.
            let mut data = (avg as i32) as i16 as i32;
            if data >= MAX_HEIGHT {
                data = MAX_HEIGHT;
            }
            self.sa_data[x] = data;

            if self.sa_peaks[x] >= MAX_HEIGHT * 256 {
                self.sa_peaks[x] = MAX_HEIGHT * 256;
            }

            self.sa_falloff[x] -= FALLOFF_STEP;
            if self.sa_falloff[x] <= data as f32 {
                self.sa_falloff[x] = data as f32;
            }

            if self.sa_peaks[x] <= js_round(self.sa_falloff[x] * 256.0) as i32 {
                self.sa_peaks[x] = (self.sa_falloff[x] * 256.0) as i32;
                self.sa_data2[x] = PEAK_START_DROP;
            }

            self.bar_peak[x] = self.sa_peaks[x] / 256;

            self.sa_peaks[x] -= js_round(self.sa_data2[x]) as i32;
            self.sa_data2[x] = (self.sa_data2[x] * PEAK_FALLOFF).min(PEAK_DROP_MAX);
            if self.sa_peaks[x] <= 0 {
                self.sa_peaks[x] = 0;
            }

            let height = js_round(self.sa_falloff[x]).clamp(0.0, MAX_HEIGHT as f32) as u8;
            let peak_row = self.bar_peak[x];
            let peak = (peak_row >= 1).then(|| peak_row.min(MAX_HEIGHT) as u8);
            self.frame.bars[x] = Bar { height, peak };
        }
    }

    /// Webamp's `paintOscilloscope` value: `round(byte / 8) - 9`, clamped to the 16 rows.
    fn scope(&mut self) {
        for j in 0..COLUMNS {
            let byte = to_byte(self.ring[(self.next + j * SCOPE_STRIDE) % WINDOW]);
            let y = js_round(byte / 8.0) - 9.0;
            self.frame.scope[j] = y.clamp(0.0, (ROWS - 1) as f32) as u8;
        }
    }
}

/// Web Audio's byte time-domain value for a sample in `-1..=1`.
fn to_byte(x: f32) -> f32 {
    (128.0 * x + 128.0).floor().clamp(0.0, 255.0)
}

/// JavaScript's `Math.round`: rounds halves towards +infinity.
fn js_round(x: f32) -> f32 {
    (x + 0.5).floor()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48_000.0;

    fn sine(freq: f32, amp: f32, n: usize) -> Vec<f32> {
        (0..n)
            .map(|i| amp * (2.0 * std::f32::consts::PI * freq * i as f32 / SR).sin())
            .collect()
    }

    fn peak_value(b: Bar) -> u8 {
        b.peak.unwrap_or(0)
    }

    #[test]
    fn silence_is_all_zero() {
        let mut vis = Vis::new();
        for _ in 0..10 {
            vis.update(&[0.0; WINDOW]);
        }
        let f = vis.frame();
        assert!(f.bars.iter().all(|b| b.height == 0 && b.peak.is_none()));
        assert!(f.scope.iter().all(|&y| y == SILENT_SCOPE));
        assert_eq!(*f, VisFrame::default());
    }

    #[test]
    fn one_kilohertz_tone_is_tallest_in_the_1k_bars() {
        let mut vis = Vis::new();
        vis.update(&sine(1000.0, 0.05, WINDOW));
        let f = vis.frame();
        // Groups of 4 columns; 1 kHz is bin ~10.7 of 256 at 48 kHz, which the log blend puts in
        // columns 20..=23 (group 5).
        let groups: Vec<u8> = (0..COLUMNS / 4).map(|g| f.bars[4 * g].height).collect();
        let (tallest, height) = groups
            .iter()
            .enumerate()
            .max_by_key(|(_, h)| **h)
            .map(|(g, h)| (g, *h))
            .expect("groups");
        assert!(height > 0, "{groups:?}");
        assert!(
            (4..=6).contains(&tallest),
            "tallest group {tallest}: {groups:?}"
        );
    }

    #[test]
    fn full_scale_tone_saturates_the_low_bars() {
        let mut vis = Vis::new();
        vis.update(&sine(1000.0, 1.0, WINDOW));
        assert_eq!(vis.frame().bars[20].height, MAX_HEIGHT as u8);
    }

    #[test]
    fn falloff_decreases_monotonically_per_frame() {
        let mut vis = Vis::new();
        vis.update(&sine(1000.0, 0.5, WINDOW));
        let mut prev = vis.frame().clone();
        assert!(prev.bars[20].height > 0);
        for _ in 0..60 {
            let cur = vis.update(&[0.0; WINDOW]).clone();
            for x in 0..COLUMNS {
                assert!(cur.bars[x].height <= prev.bars[x].height, "column {x} rose");
                assert!(
                    peak_value(cur.bars[x]) <= peak_value(prev.bars[x]),
                    "peak {x} rose"
                );
            }
            prev = cur;
        }
        assert!(prev.bars.iter().all(|b| b.height == 0 && b.peak.is_none()));
    }

    #[test]
    fn peaks_hang_above_falling_bars() {
        let mut vis = Vis::new();
        vis.update(&sine(1000.0, 1.0, WINDOW));
        let loud = vis.frame().bars[20];
        assert_eq!(loud.peak, Some(MAX_HEIGHT as u8));
        vis.update(&[0.0; WINDOW]);
        let after = vis.frame().bars[20];
        assert!(after.height < loud.height);
        assert!(after.peak.is_some_and(|p| p >= after.height));
    }

    #[test]
    fn oscilloscope_follows_a_full_scale_tone() {
        let mut vis = Vis::new();
        vis.update(&sine(1000.0, 1.0, WINDOW));
        let scope = vis.frame().scope;
        assert_eq!(*scope.iter().min().unwrap(), 0);
        assert_eq!(*scope.iter().max().unwrap(), ROWS - 1);
    }

    #[test]
    fn js_round_matches_math_round_for_halves() {
        assert_eq!(js_round(2.5), 3.0);
        assert_eq!(js_round(-0.5), 0.0);
        assert_eq!(js_round(-2.6), -3.0);
    }
}

#[cfg(test)]
mod level_tests {
    use super::*;

    /// A sine at `dbfs` (relative to full scale) at 1 kHz, one analysis window long.
    fn sine_dbfs(dbfs: f32) -> Vec<f32> {
        let amp = 10f32.powf(dbfs / 20.0);
        (0..WINDOW)
            .map(|i| amp * (2.0 * std::f32::consts::PI * 1000.0 * i as f32 / 48_000.0).sin())
            .collect()
    }

    #[test]
    fn a_minus_20_dbfs_sine_fills_most_of_the_column() {
        let mut vis = Vis::new();
        let frame = vis.update(&sine_dbfs(-20.0)).clone();
        let tallest = frame.bars.iter().map(|b| b.height).max().unwrap();
        assert!(tallest >= 12, "a -20 dBFS tone reached only row {tallest} of {MAX_HEIGHT}");
    }

    #[test]
    fn the_falloff_takes_about_a_third_of_a_second_at_sixty_steps_a_second() {
        // Webamp's "moderate" falloff at 60 Hz: 12/16 of a row per step, so a full column
        // (15 rows) empties in 20 steps.
        let mut vis = Vis::new();
        vis.update(&sine_dbfs(0.0));
        let mut steps = 0;
        while vis.frame().bars[20].height > 0 && steps < 200 {
            vis.update(&[0.0; WINDOW]);
            steps += 1;
        }
        assert!((18..=22).contains(&steps), "emptied in {steps} steps");
    }
}
