//! Realtime equalizer.
//!
//! [`EqParams`] holds the lock-free shared state (settings + diagnostic meters) and
//! can live in a `static`. [`EqProcessor`] is owned by the audio thread and does no
//! allocation in [`EqProcessor::process`].
//!
//! Chain per channel: preamp gain -> 10 RBJ peaking biquads (Q 1.41, Winamp's
//! eq10dsp value) -> balance -> soft limiter.

use crate::model::{EqSettings, BANDS_HZ, MAX_VALUE, MIN_VALUE};
use std::f64::consts::PI;
use std::sync::atomic::{AtomicBool, AtomicI8, AtomicU32, AtomicU64, AtomicU8, Ordering};

/// Winamp's eq10dsp band Q.
const BAND_Q: f64 = 1.41;
/// Channels that get the biquad cascade. Extra output channels pass through unfiltered.
const MAX_FILTER_CH: usize = 8;
/// Linear soft-limiter knee.
const LIMIT_KNEE: f32 = 0.89;

/// Lock-free shared EQ parameters and meters. Usable as a `static`.
pub struct EqParams {
    on: AtomicBool,
    preamp: AtomicU8,
    bands: [AtomicU8; 10],
    balance: AtomicI8,
    version: AtomicU64,
    /// Mean squares over ~0.5 s windows, stored as f32 bits.
    pub in_sq: AtomicU32,
    pub out_sq: AtomicU32,
    pub low_sq: AtomicU32,
    pub mid_sq: AtomicU32,
}

impl EqParams {
    pub const fn new() -> Self {
        EqParams {
            on: AtomicBool::new(true),
            preamp: AtomicU8::new(33),
            bands: [const { AtomicU8::new(33) }; 10],
            balance: AtomicI8::new(0),
            version: AtomicU64::new(0),
            in_sq: AtomicU32::new(0),
            out_sq: AtomicU32::new(0),
            low_sq: AtomicU32::new(0),
            mid_sq: AtomicU32::new(0),
        }
    }

    fn bump(&self) {
        self.version.fetch_add(1, Ordering::Release);
    }

    /// Replaces on/preamp/bands. `auto` is ignored.
    pub fn set(&self, s: &EqSettings) {
        self.on.store(s.on, Ordering::Relaxed);
        self.preamp.store(s.preamp.clamp(MIN_VALUE, MAX_VALUE), Ordering::Relaxed);
        for (a, &v) in self.bands.iter().zip(s.bands.iter()) {
            a.store(v.clamp(MIN_VALUE, MAX_VALUE), Ordering::Relaxed);
        }
        self.bump();
    }

    /// Current settings (`auto` is always false).
    pub fn get(&self) -> EqSettings {
        let mut bands = [0u8; 10];
        for (b, a) in bands.iter_mut().zip(self.bands.iter()) {
            *b = a.load(Ordering::Relaxed);
        }
        EqSettings {
            on: self.on.load(Ordering::Relaxed),
            auto: false,
            preamp: self.preamp.load(Ordering::Relaxed),
            bands,
        }
    }

    /// Balance in -100..=100. Negative attenuates the right channel, positive the left.
    pub fn set_balance(&self, b: i8) {
        self.balance.store(b.clamp(-100, 100), Ordering::Relaxed);
        self.bump();
    }

    pub fn set_band(&self, i: usize, v: u8) {
        self.bands[i].store(v.clamp(MIN_VALUE, MAX_VALUE), Ordering::Relaxed);
        self.bump();
    }

    pub fn set_preamp(&self, v: u8) {
        self.preamp.store(v.clamp(MIN_VALUE, MAX_VALUE), Ordering::Relaxed);
        self.bump();
    }

    pub fn set_on(&self, on: bool) {
        self.on.store(on, Ordering::Relaxed);
        self.bump();
    }

    /// Change counter, bumped by every setter. A reader that sees a new value knows something changed.
    pub fn version(&self) -> u64 {
        self.version.load(Ordering::Acquire)
    }

    /// Balance in -100..=100, as last set.
    pub fn balance(&self) -> i8 {
        self.balance.load(Ordering::Relaxed)
    }
}

impl Default for EqParams {
    fn default() -> Self {
        Self::new()
    }
}

/// Normalized biquad coefficients (`a0` already divided out).
#[derive(Clone, Copy, Debug)]
struct Biquad {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
}

impl Biquad {
    const IDENTITY: Biquad = Biquad { b0: 1.0, b1: 0.0, b2: 0.0, a1: 0.0, a2: 0.0 };
}

/// RBJ Audio EQ Cookbook peaking filter.
fn peaking(sr: f64, f0: f64, q: f64, db: f64) -> Biquad {
    let a = 10f64.powf(db / 40.0);
    let f = f0.min(sr * 0.45);
    let w = 2.0 * PI * f / sr;
    let (cw, sw) = (w.cos(), w.sin());
    let alpha = sw / (2.0 * q);
    let a0 = 1.0 + alpha / a;
    Biquad {
        b0: ((1.0 + alpha * a) / a0) as f32,
        b1: ((-2.0 * cw) / a0) as f32,
        b2: ((1.0 - alpha * a) / a0) as f32,
        a1: ((-2.0 * cw) / a0) as f32,
        a2: ((1.0 - alpha / a) / a0) as f32,
    }
}

/// Soft limiter: identity below the knee, then smoothly approaches 1.0. Never exceeds 1.0.
#[inline]
fn soft_limit(x: f32) -> f32 {
    let a = x.abs();
    if a <= LIMIT_KNEE {
        return x;
    }
    let k = 1.0 - LIMIT_KNEE;
    let y = LIMIT_KNEE + k * ((a - LIMIT_KNEE) / k).tanh();
    y.min(1.0).copysign(x)
}

/// Analytic gain in dB of preamp + band cascade at `hz`. Returns 0 when the EQ is off
/// (processing is bypassed in that case).
pub fn response_db(settings: &EqSettings, sample_rate: f32, hz: f32) -> f32 {
    if !settings.on {
        return 0.0;
    }
    let sr = f64::from(sample_rate);
    let w = 2.0 * PI * f64::from(hz) / sr;
    let (c1, s1) = (w.cos(), w.sin());
    let (c2, s2) = ((2.0 * w).cos(), (2.0 * w).sin());
    let mut db = f64::from(settings.preamp_db());
    for (i, &f0) in BANDS_HZ.iter().enumerate() {
        let c = peaking(sr, f64::from(f0), BAND_Q, f64::from(settings.band_db(i)));
        let (b0, b1, b2) = (f64::from(c.b0), f64::from(c.b1), f64::from(c.b2));
        let (a1, a2) = (f64::from(c.a1), f64::from(c.a2));
        let nr = b0 + b1 * c1 + b2 * c2;
        let ni = -(b1 * s1 + b2 * s2);
        let dr = 1.0 + a1 * c1 + a2 * c2;
        let di = -(a1 * s1 + a2 * s2);
        let p = (nr * nr + ni * ni) / (dr * dr + di * di);
        db += 10.0 * p.log10();
    }
    db as f32
}

/// Audio-thread processor. Owns filter state and smoothing.
pub struct EqProcessor {
    sr: f32,
    seen: u64,
    coef: [Biquad; 10],
    /// Filter states, `[channel][band][DF2T state]`.
    z: [[[f32; 2]; 10]; MAX_FILTER_CH],
    active: bool,
    gain_cur: f32,
    gain_target: f32,
    k_lo: f32,
    k_hi: f32,
    lp_lo: f32,
    lp_hi: f32,
    acc: [f64; 4],
    n: u32,
}

impl EqProcessor {
    pub fn new(sample_rate: f32) -> Self {
        let k = |hz: f32| 1.0 - (-2.0 * std::f32::consts::PI * hz / sample_rate).exp();
        EqProcessor {
            sr: sample_rate,
            seen: u64::MAX,
            coef: [Biquad::IDENTITY; 10],
            z: [[[0.0; 2]; 10]; MAX_FILTER_CH],
            active: true,
            gain_cur: 1.0,
            gain_target: 1.0,
            k_lo: k(200.0),
            k_hi: k(4000.0),
            lp_lo: 0.0,
            lp_hi: 0.0,
            acc: [0.0; 4],
            n: 0,
        }
    }

    fn refresh(&mut self, p: &EqParams) {
        let v = p.version.load(Ordering::Acquire);
        if v == self.seen {
            return;
        }
        self.seen = v;
        let s = p.get();
        for (i, c) in self.coef.iter_mut().enumerate() {
            *c = peaking(f64::from(self.sr), f64::from(BANDS_HZ[i]), BAND_Q, f64::from(s.band_db(i)));
        }
        if s.on && !self.active {
            // Re-enabling: start the filters from silence instead of stale state.
            self.z = [[[0.0; 2]; 10]; MAX_FILTER_CH];
        }
        self.active = s.on;
        self.gain_target = if s.on { 10f32.powf(s.preamp_db() / 20.0) } else { 1.0 };
    }

    #[inline]
    fn filter(&mut self, ch: usize, mut y: f32) -> f32 {
        for (c, z) in self.coef.iter().zip(self.z[ch].iter_mut()) {
            let o = c.b0 * y + z[0];
            z[0] = c.b1 * y - c.a1 * o + z[1];
            z[1] = c.b2 * y - c.a2 * o;
            y = o;
        }
        y
    }

    /// Interleaved processing. Output channel `oc` reads input channel `min(oc, in_ch - 1)`.
    /// `in_ch` must be at least 1. Buffers must hold `frames * channels` samples.
    pub fn process(&mut self, p: &EqParams, input: &[f32], in_ch: usize, out: &mut [f32], out_ch: usize, frames: usize) {
        self.refresh(p);
        if frames == 0 {
            return;
        }
        let bal = f32::from(p.balance.load(Ordering::Relaxed).clamp(-100, 100)) / 100.0;
        let gl = if bal > 0.0 { 1.0 - bal } else { 1.0 };
        let gr = if bal < 0.0 { 1.0 + bal } else { 1.0 };
        let g0 = self.gain_cur;
        let step = (self.gain_target - g0) / frames as f32;
        let filt = self.active;

        for f in 0..frames {
            let g = g0 + step * (f as f32 + 1.0);
            let mut mono_in = 0.0f32;
            let mut mono_out = 0.0f32;
            for oc in 0..out_ch {
                let ic = oc.min(in_ch - 1);
                let x = input[f * in_ch + ic];
                let mut y = x * g;
                if filt && oc < MAX_FILTER_CH {
                    y = self.filter(oc, y);
                }
                y *= match oc {
                    0 => gl,
                    1 => gr,
                    _ => 1.0,
                };
                y = soft_limit(y);
                out[f * out_ch + oc] = y;
                if oc < 2 {
                    mono_in += x * 0.5;
                    mono_out += y * 0.5;
                }
            }
            self.meter(p, mono_in, mono_out);
        }
        self.gain_cur = self.gain_target;
    }

    /// Diagnostics: low = one-pole LP ~200 Hz of mono output, mid = LP 4 kHz minus LP 200 Hz.
    /// Publishes mean squares to `p` every ~0.5 s.
    #[inline]
    fn meter(&mut self, p: &EqParams, mono_in: f32, mono_out: f32) {
        self.lp_lo += self.k_lo * (mono_out - self.lp_lo);
        self.lp_hi += self.k_hi * (mono_out - self.lp_hi);
        let mid = self.lp_hi - self.lp_lo;
        self.acc[0] += f64::from(mono_in * mono_in);
        self.acc[1] += f64::from(mono_out * mono_out);
        self.acc[2] += f64::from(self.lp_lo * self.lp_lo);
        self.acc[3] += f64::from(mid * mid);
        self.n += 1;
        if self.n as f32 > self.sr * 0.5 {
            let n = f64::from(self.n);
            let publish = |a: &AtomicU32, v: f64| a.store(((v / n) as f32).to_bits(), Ordering::Relaxed);
            publish(&p.in_sq, self.acc[0]);
            publish(&p.out_sq, self.acc[1]);
            publish(&p.low_sq, self.acc[2]);
            publish(&p.mid_sq, self.acc[3]);
            self.acc = [0.0; 4];
            self.n = 0;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SR: f32 = 48000.0;

    fn sine(freq: f32, amp: f32, frames: usize, ch: usize) -> Vec<f32> {
        let mut v = Vec::with_capacity(frames * ch);
        for f in 0..frames {
            let s = amp * (2.0 * std::f32::consts::PI * freq * f as f32 / SR).sin();
            for _ in 0..ch {
                v.push(s);
            }
        }
        v
    }

    fn run(p: &EqParams, input: &[f32], in_ch: usize, out_ch: usize) -> Vec<f32> {
        let frames = input.len() / in_ch;
        let mut out = vec![0.0; frames * out_ch];
        let mut proc = EqProcessor::new(SR);
        proc.process(p, input, in_ch, &mut out, out_ch, frames);
        out
    }

    #[test]
    fn flat_is_near_transparent() {
        let p = EqParams::new();
        p.set(&EqSettings::default());
        let input = sine(1000.0, 0.25, 4800, 2);
        let out = run(&p, &input, 2, 2);
        let max = input
            .iter()
            .zip(out.iter())
            .skip(100 * 2)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(max < 1e-5, "max diff {max}");
    }

    #[test]
    fn single_band_at_1k_gives_about_12db() {
        let mut s = EqSettings::default();
        s.bands[4] = 64;
        let at_1k = response_db(&s, SR, 1000.0);
        assert!((at_1k - 12.0).abs() <= 0.3, "1 kHz gain {at_1k}");
        let at_60 = response_db(&s, SR, 60.0);
        assert!(at_60 < 1.0, "60 Hz gain {at_60}");
    }

    #[test]
    fn off_is_bypass_for_quiet_signal() {
        let p = EqParams::new();
        let mut s = EqSettings::default();
        s.on = false;
        s.preamp = 64;
        s.bands = [64; 10];
        p.set(&s);
        let input = sine(1000.0, 0.25, 4800, 2);
        let out = run(&p, &input, 2, 2);
        assert_eq!(out, input);
    }

    #[test]
    fn off_response_is_flat() {
        let mut s = EqSettings::default();
        s.on = false;
        s.bands = [64; 10];
        assert_eq!(response_db(&s, SR, 1000.0), 0.0);
    }

    #[test]
    fn limiter_caps_output_at_unity() {
        let p = EqParams::new();
        let mut s = EqSettings::default();
        s.preamp = 64;
        s.bands = [64; 10];
        p.set(&s);
        // Full scale + 6 dB = amplitude 2.0.
        let input = sine(1000.0, 2.0, 4800, 2);
        let out = run(&p, &input, 2, 2);
        assert!(out.iter().all(|y| y.abs() <= 1.0), "peak {}", out.iter().fold(0.0f32, |m, y| m.max(y.abs())));
    }

    #[test]
    fn limiter_is_monotonic_and_transparent_below_knee() {
        assert_eq!(soft_limit(0.5), 0.5);
        assert_eq!(soft_limit(-0.89), -0.89);
        let mut prev = 0.0;
        for i in 0..=400 {
            let x = i as f32 * 0.01;
            let y = soft_limit(x);
            assert!(y >= prev && y <= 1.0, "x={x} y={y}");
            prev = y;
        }
        assert_eq!(soft_limit(-5.0), -soft_limit(5.0));
    }

    #[test]
    fn balance_full_left_silences_right() {
        let p = EqParams::new();
        p.set(&EqSettings::default());
        p.set_balance(-100);
        let input = sine(1000.0, 0.25, 4800, 2);
        let out = run(&p, &input, 2, 2);
        assert!(out.iter().skip(1).step_by(2).all(|&y| y == 0.0));
        assert!(out.iter().step_by(2).any(|&y| y != 0.0));
    }

    #[test]
    fn balance_full_right_silences_left() {
        let p = EqParams::new();
        p.set(&EqSettings::default());
        p.set_balance(100);
        let input = sine(1000.0, 0.25, 4800, 2);
        let out = run(&p, &input, 2, 2);
        assert!(out.iter().step_by(2).all(|&y| y == 0.0));
    }

    #[test]
    fn get_round_trips_set() {
        let p = EqParams::new();
        let mut s = EqSettings::default();
        s.preamp = 40;
        s.bands[2] = 50;
        s.on = false;
        p.set(&s);
        assert_eq!(p.get(), s);
        p.set_band(0, 1);
        p.set_preamp(20);
        p.set_on(true);
        let g = p.get();
        assert_eq!(g.bands[0], 1);
        assert_eq!(g.preamp, 20);
        assert!(g.on);
    }
}
