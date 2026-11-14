//! The audio tap's start, run off the main thread.
//!
//! Creating the tap and starting its device can block while macOS shows the System Audio
//! Recording prompt, so the caller never runs them on the UI thread. [`Capture`] runs the start on
//! a worker thread and hands the result back through a channel; the UI polls it with
//! [`Capture::poll`] (or waits with [`Capture::wait`] in tests).
//!
//! State machine:
//!
//! ```text
//! Idle ──request_start──▶ Starting ──ok──▶ Running ──deny──▶ Denied ──retry──▶ Idle
//!  ▲                        │  │
//!  └──────retry (auto)── Failed ◀── err ─┘
//! ```
//!
//! - `request_start` is accepted from `Idle` and `Failed` only. Starting or running ignores it,
//!   so there is never more than one start in flight.
//! - `deny` is for a tap that is running but silent (see `guard`). It drops the engine, which stops
//!   the device, and parks the state at `Denied`. Only `retry` leaves `Denied`, so nothing starts
//!   again on its own.

use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::thread;
use std::time::{Duration, Instant};

use crate::AudioError;

/// Where the capture is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptureState {
    /// Nothing running and nothing pending. A start may be requested.
    Idle,
    /// A start is running on the worker thread.
    Starting,
    /// The engine is running.
    Running,
    /// The last start failed. A start may be requested again (the caller paces the retries).
    Failed,
    /// The engine was torn down as silent. Only `retry` (an explicit user action) leaves this state.
    Denied,
}

/// What changed in the last [`Capture::poll`] or [`Capture::wait`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CaptureEvent {
    /// The start succeeded. The engine is available through [`Capture::engine`].
    Started,
    /// The start failed with this error.
    Failed(AudioError),
}

/// The start in flight: the worker sends exactly one result.
type Pending<E> = Receiver<Result<E, AudioError>>;

/// The start's state, with the engine when it is running.
pub struct Capture<E> {
    state: CaptureState,
    engine: Option<E>,
    pending: Option<Pending<E>>,
    /// When the current start was requested.
    started_at: Option<Instant>,
    /// The error of the last failed start.
    last_error: Option<AudioError>,
}

impl<E: Send + 'static> Default for Capture<E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<E: Send + 'static> Capture<E> {
    pub fn new() -> Self {
        Capture {
            state: CaptureState::Idle,
            engine: None,
            pending: None,
            started_at: None,
            last_error: None,
        }
    }

    pub fn state(&self) -> CaptureState {
        self.state
    }

    /// Whether a start may be requested now: from `Idle` or `Failed`.
    pub fn can_start(&self) -> bool {
        matches!(self.state, CaptureState::Idle | CaptureState::Failed)
    }

    /// The running engine, if any.
    pub fn engine(&self) -> Option<&E> {
        self.engine.as_ref()
    }

    /// The running engine, mutably, if any.
    pub fn engine_mut(&mut self) -> Option<&mut E> {
        self.engine.as_mut()
    }

    /// The error of the last failed start, until the next start is requested.
    pub fn last_error(&self) -> Option<&AudioError> {
        self.last_error.as_ref()
    }

    /// How long the current start has been running, or `None` when none is.
    pub fn starting_for(&self, now: Instant) -> Option<Duration> {
        match (self.state, self.started_at) {
            (CaptureState::Starting, Some(at)) => Some(now.saturating_duration_since(at)),
            _ => None,
        }
    }

    /// Starts `start` on a worker thread. Returns `false` (and does nothing) unless the state is
    /// `Idle` or `Failed`.
    pub fn request_start<F>(&mut self, now: Instant, start: F) -> bool
    where
        F: FnOnce() -> Result<E, AudioError> + Send + 'static,
    {
        if !self.can_start() {
            return false;
        }
        self.last_error = None;
        match spawn_start(start) {
            Ok(rx) => {
                self.pending = Some(rx);
                self.state = CaptureState::Starting;
                self.started_at = Some(now);
            }
            Err(e) => {
                self.state = CaptureState::Failed;
                self.last_error = Some(e);
            }
        }
        true
    }

    /// Takes the start's result if it has arrived. Never blocks. Returns `None` while the start is
    /// still running, or when there is nothing pending.
    pub fn poll(&mut self) -> Option<CaptureEvent> {
        let rx = self.pending.as_ref()?;
        match rx.try_recv() {
            Ok(result) => Some(self.finish(result)),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(self.finish(Err(worker_died()))),
        }
    }

    /// Waits up to `timeout` for the start's result. For tests and for code that must block.
    pub fn wait(&mut self, timeout: Duration) -> Option<CaptureEvent> {
        let rx = self.pending.as_ref()?;
        match rx.recv_timeout(timeout) {
            Ok(result) => Some(self.finish(result)),
            Err(mpsc::RecvTimeoutError::Timeout) => None,
            Err(mpsc::RecvTimeoutError::Disconnected) => Some(self.finish(Err(worker_died()))),
        }
    }

    /// Stops the engine after a denial, and parks the state at `Denied`. Only valid while running.
    /// Returns `false` in any other state.
    pub fn deny(&mut self) -> bool {
        if self.state != CaptureState::Running {
            return false;
        }
        // Dropping the engine stops the device; the original output plays again.
        self.engine = None;
        self.state = CaptureState::Denied;
        true
    }

    /// Leaves `Denied` for `Idle`, so the next tick may start again. Other states are unchanged.
    pub fn retry(&mut self) {
        if self.state == CaptureState::Denied {
            self.state = CaptureState::Idle;
        }
    }

    fn finish(&mut self, result: Result<E, AudioError>) -> CaptureEvent {
        self.pending = None;
        self.started_at = None;
        match result {
            Ok(engine) => {
                self.engine = Some(engine);
                self.state = CaptureState::Running;
                CaptureEvent::Started
            }
            Err(e) => {
                self.state = CaptureState::Failed;
                self.last_error = Some(e.clone());
                CaptureEvent::Failed(e)
            }
        }
    }
}

fn worker_died() -> AudioError {
    AudioError::Core {
        stage: "worker",
        code: -1,
    }
}

/// Runs `start` on a new thread and returns the receiver for its result. If the result cannot be
/// delivered (the capture was dropped meanwhile), the engine is dropped on the worker thread.
fn spawn_start<E, F>(start: F) -> Result<Pending<E>, AudioError>
where
    E: Send + 'static,
    F: FnOnce() -> Result<E, AudioError> + Send + 'static,
{
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("tubeamp-audio-start".into())
        .spawn(move || {
            let _ = tx.send(start());
        })
        .map_err(|_| AudioError::Core {
            stage: "spawn",
            code: -1,
        })?;
    Ok(rx)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    const WAIT: Duration = Duration::from_secs(5);

    /// A start that waits for `gate` before it returns `result`. The gate lets a test hold the
    /// start in `Starting` for as long as it needs.
    fn gated(
        gate: Receiver<()>,
        result: Result<u32, AudioError>,
    ) -> impl FnOnce() -> Result<u32, AudioError> + Send + 'static {
        move || {
            let _ = gate.recv();
            result
        }
    }

    #[test]
    fn idle_to_starting_to_running() {
        let mut c: Capture<u32> = Capture::new();
        let (open, gate) = mpsc::channel();
        let now = Instant::now();
        assert!(c.request_start(now, gated(gate, Ok(7))));
        assert_eq!(c.state(), CaptureState::Starting);
        assert!(c.poll().is_none(), "the start is still gated");
        open.send(()).unwrap();
        assert_eq!(c.wait(WAIT), Some(CaptureEvent::Started));
        assert_eq!(c.state(), CaptureState::Running);
        assert_eq!(c.engine(), Some(&7));
        assert!(c.poll().is_none(), "nothing pending after the result");
    }

    #[test]
    fn a_second_request_while_starting_is_refused_and_spawns_nothing() {
        let mut c: Capture<u32> = Capture::new();
        let spawned = Arc::new(AtomicUsize::new(0));
        let (open, gate) = mpsc::channel();
        let counter = Arc::clone(&spawned);
        let now = Instant::now();
        assert!(c.request_start(now, move || {
            counter.fetch_add(1, Ordering::SeqCst);
            let _ = gate.recv();
            Ok(1)
        }));
        let again = c.request_start(now, || panic!("must not run while a start is pending"));
        assert!(!again, "a start is already in flight");
        open.send(()).unwrap();
        assert_eq!(c.wait(WAIT), Some(CaptureEvent::Started));
        assert_eq!(spawned.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn a_second_request_while_running_is_refused() {
        let mut c: Capture<u32> = Capture::new();
        assert!(c.request_start(Instant::now(), || Ok(3)));
        c.wait(WAIT);
        assert_eq!(c.state(), CaptureState::Running);
        assert!(!c.request_start(Instant::now(), || panic!("already running")));
        assert_eq!(c.state(), CaptureState::Running);
    }

    #[test]
    fn a_failed_start_can_be_retried() {
        let mut c: Capture<u32> = Capture::new();
        let now = Instant::now();
        assert!(c.request_start(now, || Err(AudioError::NoOwnedProcess)));
        assert_eq!(
            c.wait(WAIT),
            Some(CaptureEvent::Failed(AudioError::NoOwnedProcess))
        );
        assert_eq!(c.state(), CaptureState::Failed);
        assert!(c.can_start());
        assert_eq!(c.last_error(), Some(&AudioError::NoOwnedProcess));
        assert!(c.request_start(now, || Ok(9)));
        assert_eq!(c.state(), CaptureState::Starting);
        assert!(c.last_error().is_none(), "a new start clears the old error");
        assert_eq!(c.wait(WAIT), Some(CaptureEvent::Started));
    }

    #[test]
    fn unsupported_is_reported_as_a_failure() {
        let mut c: Capture<u32> = Capture::new();
        assert!(c.request_start(Instant::now(), || Err(AudioError::Unsupported)));
        assert_eq!(
            c.wait(WAIT),
            Some(CaptureEvent::Failed(AudioError::Unsupported))
        );
    }

    #[test]
    fn a_worker_that_dies_without_a_result_is_a_failure() {
        let mut c: Capture<u32> = Capture::new();
        assert!(c.request_start(Instant::now(), || panic!("worker dies")));
        match c.wait(WAIT) {
            Some(CaptureEvent::Failed(AudioError::Core { stage, .. })) => {
                assert_eq!(stage, "worker")
            }
            other => panic!("expected a worker failure, got {other:?}"),
        }
        assert_eq!(c.state(), CaptureState::Failed);
    }

    #[test]
    fn deny_drops_the_engine_and_only_retry_leaves_denied() {
        let dropped = Arc::new(AtomicUsize::new(0));

        struct Probe(Arc<AtomicUsize>);
        impl Drop for Probe {
            fn drop(&mut self) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let mut c: Capture<Probe> = Capture::new();
        let counter = Arc::clone(&dropped);
        assert!(c.request_start(Instant::now(), move || Ok(Probe(counter))));
        assert_eq!(c.wait(WAIT), Some(CaptureEvent::Started));
        assert!(c.deny());
        assert_eq!(dropped.load(Ordering::SeqCst), 1, "deny stops the engine");
        assert_eq!(c.state(), CaptureState::Denied);
        assert!(c.engine().is_none());
        assert!(!c.can_start(), "denied does not start on its own");
        assert!(!c.request_start(Instant::now(), || Ok(Probe(Arc::new(AtomicUsize::new(0))))));
        c.retry();
        assert_eq!(c.state(), CaptureState::Idle);
        assert!(c.can_start());
    }

    #[test]
    fn deny_is_only_valid_while_running() {
        let mut c: Capture<u32> = Capture::new();
        assert!(!c.deny(), "idle");
        let (open, gate) = mpsc::channel();
        assert!(c.request_start(Instant::now(), gated(gate, Ok(1))));
        assert!(!c.deny(), "starting");
        open.send(()).unwrap();
        c.wait(WAIT);
        assert!(c.deny(), "running");
    }

    #[test]
    fn retry_does_not_touch_a_start_in_flight() {
        let mut c: Capture<u32> = Capture::new();
        let (open, gate) = mpsc::channel();
        assert!(c.request_start(Instant::now(), gated(gate, Ok(2))));
        c.retry();
        assert_eq!(c.state(), CaptureState::Starting);
        open.send(()).unwrap();
        assert_eq!(c.wait(WAIT), Some(CaptureEvent::Started));
    }

    #[test]
    fn starting_for_measures_from_the_request() {
        let mut c: Capture<u32> = Capture::new();
        let (open, gate) = mpsc::channel();
        let t0 = Instant::now();
        assert!(c.request_start(t0, gated(gate, Ok(1))));
        assert_eq!(c.starting_for(t0), Some(Duration::ZERO));
        assert_eq!(
            c.starting_for(t0 + Duration::from_millis(1500)),
            Some(Duration::from_millis(1500))
        );
        open.send(()).unwrap();
        c.wait(WAIT);
        assert_eq!(c.starting_for(t0 + Duration::from_secs(9)), None);
    }

    #[test]
    fn the_engine_is_moved_across_threads() {
        // The real engine crosses from the worker to the UI thread: the state machine needs a
        // `Send` payload, and the result must arrive on the thread that polls.
        let seen = Arc::new(Mutex::new(None));
        let mut c: Capture<String> = Capture::new();
        assert!(c.request_start(Instant::now(), || Ok("engine".to_string())));
        assert_eq!(c.wait(WAIT), Some(CaptureEvent::Started));
        *seen.lock().unwrap() = c.engine().cloned();
        assert_eq!(seen.lock().unwrap().as_deref(), Some("engine"));
    }
}
