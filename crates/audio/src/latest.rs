//! The latest mono samples, for a second consumer next to the analyser's ring.
//!
//! The analyser drains an `rtrb` ring, which one consumer can read. The Milkdrop window also needs
//! recent samples, but a window of them rather than the ones not read yet. So the audio thread
//! also writes every mono sample into this fixed ring of atomics and publishes the running total
//! with a release store. A reader copies the newest samples without a lock.
//!
//! There must be one writer (the audio thread). A reader that copies while the writer is
//! overwriting a slot can get a sample from the lap before, so a copy is not always a clean
//! snapshot. For a visualiser that is harmless, and it keeps the audio thread wait-free.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

/// How many samples the ring keeps: about 43 ms at 48 kHz, more than any reader asks for.
pub const LATEST_CAPACITY: usize = 2048;

/// A fixed ring of the most recent samples, with one writer and any number of readers.
pub struct LatestSamples {
    /// The samples as `f32` bits. Atomics keep concurrent access defined.
    slots: Box<[AtomicU32]>,
    /// How many samples have been written so far, ever.
    written: AtomicU64,
}

impl LatestSamples {
    pub fn new() -> LatestSamples {
        LatestSamples {
            slots: (0..LATEST_CAPACITY).map(|_| AtomicU32::new(0)).collect(),
            written: AtomicU64::new(0),
        }
    }

    /// Appends one sample. Called from the audio thread: it never blocks or allocates.
    pub fn push(&self, sample: f32) {
        // Only the writer changes `written`, so a relaxed read of the old value is exact.
        let n = self.written.load(Ordering::Relaxed);
        self.slots[(n % LATEST_CAPACITY as u64) as usize].store(sample.to_bits(), Ordering::Relaxed);
        self.written.store(n + 1, Ordering::Release);
    }

    /// How many samples have been written so far.
    pub fn written(&self) -> u64 {
        self.written.load(Ordering::Acquire)
    }

    /// Replaces `out` with the newest `min(n, LATEST_CAPACITY, written)` samples, oldest first.
    /// Returns the total number of samples written when the copy started. Compare it with the
    /// previous return value to tell whether anything new arrived.
    pub fn copy_latest(&self, out: &mut Vec<f32>, n: usize) -> u64 {
        out.clear();
        let end = self.written.load(Ordering::Acquire);
        let count = (n.min(LATEST_CAPACITY) as u64).min(end);
        out.reserve(count as usize);
        for k in (end - count)..end {
            let bits = self.slots[(k % LATEST_CAPACITY as u64) as usize].load(Ordering::Relaxed);
            out.push(f32::from_bits(bits));
        }
        end
    }
}

impl Default for LatestSamples {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_ring_copies_nothing() {
        let ring = LatestSamples::new();
        let mut out = vec![9.0];
        assert_eq!(ring.copy_latest(&mut out, 16), 0);
        assert!(out.is_empty());
        assert_eq!(ring.written(), 0);
    }

    #[test]
    fn the_newest_samples_come_oldest_first() {
        let ring = LatestSamples::new();
        for k in 1..=5 {
            ring.push(k as f32);
        }
        let mut out = Vec::new();
        assert_eq!(ring.copy_latest(&mut out, 3), 5);
        assert_eq!(out, vec![3.0, 4.0, 5.0]);
        assert_eq!(ring.written(), 5);
    }

    #[test]
    fn a_short_ring_copies_what_it_has() {
        let ring = LatestSamples::new();
        ring.push(0.25);
        ring.push(-0.5);
        let mut out = Vec::new();
        assert_eq!(ring.copy_latest(&mut out, 1024), 2);
        assert_eq!(out, vec![0.25, -0.5]);
    }

    #[test]
    fn wrapping_keeps_only_the_last_capacity_samples_in_order() {
        let ring = LatestSamples::new();
        let total = LATEST_CAPACITY + 10;
        for k in 0..total {
            ring.push(k as f32);
        }
        let mut out = Vec::new();
        assert_eq!(ring.copy_latest(&mut out, LATEST_CAPACITY), total as u64);
        assert_eq!(out.len(), LATEST_CAPACITY);
        assert_eq!(out[0], 10.0);
        assert_eq!(*out.last().unwrap(), (total - 1) as f32);
        assert!(out.windows(2).all(|w| w[1] == w[0] + 1.0));
    }

    #[test]
    fn asking_for_more_than_the_capacity_is_capped() {
        let ring = LatestSamples::new();
        for k in 0..(LATEST_CAPACITY as u32 + 5) {
            ring.push(k as f32);
        }
        let mut out = Vec::new();
        ring.copy_latest(&mut out, usize::MAX);
        assert_eq!(out.len(), LATEST_CAPACITY);
    }

    #[test]
    fn the_reader_on_another_thread_sees_the_final_samples() {
        use std::sync::Arc;

        let ring = Arc::new(LatestSamples::new());
        let writer = {
            let ring = Arc::clone(&ring);
            std::thread::spawn(move || {
                for k in 0..200_000u32 {
                    ring.push((k % 1000) as f32 / 1000.0);
                }
            })
        };
        let mut out = Vec::new();
        // Copies taken while the writer runs must have the requested length and finite values.
        for _ in 0..200 {
            let n = ring.copy_latest(&mut out, 256);
            assert!(n <= 200_000);
            assert_eq!(out.len() as u64, n.min(256));
            assert!(out.iter().all(|s| s.is_finite() && (0.0..1.0).contains(s)));
        }
        writer.join().expect("writer thread");
        // After the writer stopped, the copy is exact.
        assert_eq!(ring.copy_latest(&mut out, 4), 200_000);
        let expected: Vec<f32> = (199_996u32..200_000)
            .map(|k| (k % 1000) as f32 / 1000.0)
            .collect();
        assert_eq!(out, expected);
    }
}
