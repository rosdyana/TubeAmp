//! macOS audio engine: Core Audio process tap (see `tap.m`) + EQ in the IOProc.

use std::ffi::{c_int, c_void};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use tubeamp_eq::EqProcessor;

use crate::guard;
use crate::{AudioError, LatestSamples, PARAMS};

type IoCallback = extern "C" fn(*const f32, u32, *mut f32, u32, u32, *mut c_void);

extern "C" {
    fn ta_create(owner: c_int, verbose: c_int, out_sr: *mut f64) -> c_int;
    fn ta_start(cb: IoCallback, user: *mut c_void) -> c_int;
    fn ta_stop();
}

/// Ring capacity in samples (about 0.7 s at 48 kHz). Overflow drops the newest samples.
const RING_CAPACITY: usize = 1 << 15;

/// Only one tap can exist at a time (the C side keeps global state).
static RUNNING: AtomicBool = AtomicBool::new(false);

/// `TapWatch::last_sound_ns` before the callback has heard any sound.
const NEVER: u64 = u64::MAX;

/// What the audio callback reports back to the engine, read from the main thread.
struct TapWatch {
    /// When the tap started. `last_sound_ns` counts from here.
    epoch: Instant,
    /// Nanoseconds after `epoch` of the last block with a non-zero input sample, or `NEVER`.
    last_sound_ns: AtomicU64,
    /// Dev hook (`TUBEAMP_FAKE_DENIED=1`): the callback treats its input as silent, so the denial
    /// path can be tested on a machine that has the permission.
    fake_denied: bool,
}

/// State owned by the audio callback. Allocated before the IOProc starts and freed after it stops.
struct Callback {
    eq: EqProcessor,
    tx: rtrb::Producer<f32>,
    /// The newest samples, for `AudioEngine::copy_latest`. The engine keeps its own handle.
    latest: Arc<LatestSamples>,
    watch: Arc<TapWatch>,
}

/// Runs on the Core Audio thread. Must not allocate or block.
extern "C" fn on_audio(
    inp: *const f32,
    in_ch: u32,
    out: *mut f32,
    out_ch: u32,
    frames: u32,
    user: *mut c_void,
) {
    let (ic, oc, n) = (
        in_ch.max(1) as usize,
        out_ch.max(1) as usize,
        frames as usize,
    );
    if n == 0 || inp.is_null() || out.is_null() || user.is_null() {
        return;
    }
    // SAFETY: `user` is the `Callback` boxed in `start_inner`. It stays alive until `ta_stop`
    // has returned, and `ta_stop` is the only thing that stops this callback.
    let cb = unsafe { &mut *(user as *mut Callback) };
    // SAFETY: the C side passes buffers of `frames * channels` interleaved floats (it clamps
    // `frames` to both buffers), and `ic`/`oc` are the channel counts it reported.
    let input = unsafe { std::slice::from_raw_parts(inp, n * ic) };
    let output = unsafe { std::slice::from_raw_parts_mut(out, n * oc) };
    if !cb.watch.fake_denied && !guard::is_silent(input) {
        let ns = cb.watch.epoch.elapsed().as_nanos() as u64;
        cb.watch.last_sound_ns.store(ns, Ordering::Relaxed);
    }
    cb.eq.process(&PARAMS, input, ic, output, oc, n);
    for frame in output.chunks_exact(oc) {
        let left = frame[0];
        let right = if oc > 1 { frame[1] } else { left };
        let mono = 0.5 * (left + right);
        // A full ring drops the sample; the UI only needs a recent window.
        let _ = cb.tx.push(mono);
        cb.latest.push(mono);
    }
}

fn map_error(code: c_int, stage: &'static str) -> AudioError {
    match code {
        -10 => AudioError::NoOwnedProcess,
        code => AudioError::Core { stage, code },
    }
}

/// The running tap. Dropping it stops the device.
pub struct AudioEngine {
    rx: rtrb::Consumer<f32>,
    latest: Arc<LatestSamples>,
    sample_rate: u32,
    callback: *mut Callback,
    watch: Arc<TapWatch>,
}

// SAFETY: the only non-`Send` field is `callback`, a raw pointer to the `Callback` that
// `start_inner` boxed. This engine is its sole owner. Before `ta_start` returns, nothing else
// touches it. From then until `ta_stop` returns, only the Core Audio thread uses it, through the
// IOProc. `Drop` calls `ta_stop` before freeing it. So moving the engine to another thread (the
// start runs on a worker, see `capture.rs`) does not create concurrent access. The other fields
// are `Send` on their own.
unsafe impl Send for AudioEngine {}

// Checked at compile time, so the `unsafe impl` above cannot silently stop applying.
const _: fn() = || {
    fn assert_send<T: Send>() {}
    assert_send::<AudioEngine>();
};

impl AudioEngine {
    /// Starts the tap on our WebKit audio process. Fails with
    /// [`AudioError::NoOwnedProcess`] until YouTube Music has started playing, so callers
    /// should retry.
    ///
    /// This can block while macOS shows the System Audio Recording prompt. Call it from a worker
    /// thread, never the UI thread (see [`crate::Capture`]).
    pub fn start() -> Result<AudioEngine, AudioError> {
        // Dev hook (`TUBEAMP_FAKE_START_DELAY_MS=N`): holds the start for N ms, to test that the UI
        // keeps running while a slow start is in flight.
        if let Some(ms) = std::env::var("TUBEAMP_FAKE_START_DELAY_MS")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
        {
            std::thread::sleep(Duration::from_millis(ms));
        }
        if RUNNING.swap(true, Ordering::SeqCst) {
            return Err(AudioError::AlreadyRunning);
        }
        let result = start_inner();
        if result.is_err() {
            RUNNING.store(false, Ordering::SeqCst);
        }
        result
    }

    /// Device sample rate in Hz, as read before the IOProc started.
    pub fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    /// When the tap last delivered a non-zero sample, or when it started if it never has. A tap
    /// that is denied, or whose IOProc never runs, keeps this at its start. See `guard`.
    pub fn last_sound_at(&self) -> Instant {
        match self.watch.last_sound_ns.load(Ordering::Relaxed) {
            NEVER => self.watch.epoch,
            ns => self.watch.epoch + Duration::from_nanos(ns),
        }
    }

    /// Replaces `out` with the mono post-EQ samples produced since the last call.
    pub fn drain_samples(&mut self, out: &mut Vec<f32>) {
        out.clear();
        let available = self.rx.slots();
        if available == 0 {
            return;
        }
        if let Ok(chunk) = self.rx.read_chunk(available) {
            let (first, second) = chunk.as_slices();
            out.extend_from_slice(first);
            out.extend_from_slice(second);
            chunk.commit_all();
        }
    }

    /// Replaces `out` with the newest `n` mono post-EQ samples (at most [`LATEST_CAPACITY`]),
    /// oldest first, without touching the samples `drain_samples` reads. Returns the total number
    /// of samples written so far: when it has not changed since the last call, no new audio came.
    pub fn copy_latest(&self, out: &mut Vec<f32>, n: usize) -> u64 {
        self.latest.copy_latest(out, n)
    }
}

fn start_inner() -> Result<AudioEngine, AudioError> {
    let verbose = c_int::from(std::env::var_os("TUBEAMP_LOG").is_some());
    let owner = c_int::try_from(std::process::id()).unwrap_or(0);
    let fake_denied = std::env::var_os("TUBEAMP_FAKE_DENIED").is_some_and(|v| v == "1");
    if fake_denied {
        eprintln!("tubeamp: TUBEAMP_FAKE_DENIED=1: the tap's input is treated as silent (dev test)");
    }
    let watch = Arc::new(TapWatch {
        epoch: Instant::now(),
        last_sound_ns: AtomicU64::new(NEVER),
        fake_denied,
    });

    let mut sr = 0.0f64;
    // SAFETY: `sr` is a valid out-pointer for the duration of the call.
    let rc = unsafe { ta_create(owner, verbose, &mut sr) };
    if rc != 0 {
        // SAFETY: `ta_stop` is idempotent and frees anything a failed `ta_create` left behind.
        unsafe { ta_stop() };
        return Err(map_error(rc, "create"));
    }
    let sample_rate = sr.round() as u32;

    // Build the DSP for the real device rate before any callback can run.
    let (tx, rx) = rtrb::RingBuffer::<f32>::new(RING_CAPACITY);
    let latest = Arc::new(LatestSamples::new());
    let callback = Box::into_raw(Box::new(Callback {
        eq: EqProcessor::new(sr as f32),
        tx,
        latest: Arc::clone(&latest),
        watch: Arc::clone(&watch),
    }));

    // SAFETY: `callback` stays valid until `AudioEngine::drop` (or the failure branch below)
    // frees it, and both call `ta_stop` first.
    let rc = unsafe { ta_start(on_audio, callback as *mut c_void) };
    if rc != 0 {
        // SAFETY: `ta_stop` returns before the callback is freed, so nothing still uses it.
        unsafe {
            ta_stop();
            drop(Box::from_raw(callback));
        }
        return Err(map_error(rc, "start"));
    }
    Ok(AudioEngine {
        rx,
        latest,
        sample_rate,
        callback,
        watch,
    })
}

impl Drop for AudioEngine {
    fn drop(&mut self) {
        // SAFETY: `ta_stop` returns only after the IOProc has stopped, so the callback state
        // can be freed afterwards and is never used again.
        unsafe {
            ta_stop();
            drop(Box::from_raw(self.callback));
        }
        RUNNING.store(false, Ordering::SeqCst);
    }
}
