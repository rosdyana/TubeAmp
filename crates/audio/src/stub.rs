//! Non-macOS stub. Windows audio (in-page Web Audio) comes in a later milestone.

use crate::AudioError;

/// Never running on this platform: `start` always fails, so the other methods are unreachable
/// in practice. They exist so the app compiles unchanged.
pub struct AudioEngine {
    sample_rate: u32,
}

impl AudioEngine {
    pub fn start() -> Result<AudioEngine, AudioError> {
        Err(AudioError::Unsupported)
    }

    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    pub fn last_sound_at(&self) -> std::time::Instant {
        std::time::Instant::now()
    }

    pub fn drain_samples(&mut self, out: &mut Vec<f32>) {
        out.clear();
    }

    pub fn copy_latest(&self, out: &mut Vec<f32>, _n: usize) -> u64 {
        out.clear();
        0
    }
}
