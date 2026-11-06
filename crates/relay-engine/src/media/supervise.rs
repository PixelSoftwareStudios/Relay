//! Worker supervision: a panic in one step is contained, reported and
//! recovered from, a hang is detectable, and repeated failure is final.

use std::any::Any;
use std::collections::VecDeque;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, Instant};

use relay_audio::priority::{PriorityGuard, Promotion, WorkerKind, log_promotion};
use relay_core::CallId;

/// Panics inside this window count toward the restart limit.
const RESTART_WINDOW: Duration = Duration::from_secs(60);
/// Panics tolerated in the window; the next one is final.
const RESTARTS_MAX: usize = 3;

/// Which media thread an event is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerId {
	/// Microphone capture, suppression and encoding.
	Capture,
	/// Decoding, mixing and playback.
	Render,
	/// The device thread.
	Devices,
	/// A peer's media receive task.
	Network,
}

/// Why a worker is reported as failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureKind {
	/// A step panicked and was contained.
	Panic,
	/// No heartbeat for longer than the watchdog allows.
	Hang,
	/// The media socket kept failing and the task gave up.
	Socket,
}

/// A worker failure, delivered upward as `MediaEvent::WorkerFailed`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorkerFailure {
	/// The failed thread.
	pub worker: WorkerId,
	/// What happened.
	pub kind: FailureKind,
	/// The worker will not run again.
	pub permanent: bool,
	/// The call that was being processed, when known.
	pub call: Option<CallId>,
}

/// Liveness counter bumped once per worker loop pass, and whether the worker
/// is expected to be running (a call, a tone or the monitor needs it).
#[derive(Debug, Default)]
pub struct Heartbeat {
	beats: AtomicU64,
	active: AtomicBool,
}

impl Heartbeat {
	/// Records whether the worker has work that must not be late. The
	/// watchdog judges only an active worker.
	pub fn set_active(&self, active: bool) {
		self.active.store(active, Ordering::Relaxed);
	}

	/// Whether the worker is expected to be running right now.
	pub fn is_active(&self) -> bool {
		self.active.load(Ordering::Relaxed)
	}

	/// Records one pass of the loop.
	pub fn beat(&self) {
		self.beats.fetch_add(1, Ordering::Relaxed);
	}

	/// Passes completed so far.
	pub fn count(&self) -> u64 {
		self.beats.load(Ordering::Relaxed)
	}
}

/// What a supervised worker must provide.
pub trait Supervised {
	/// One pass of work; may panic.
	fn step(&mut self);
	/// How long to sleep before the next pass.
	fn wait(&self) -> Duration;
	/// Puts the worker back into a safe state after a panic and names the
	/// call that was being processed, if any. Gates must end up closed.
	fn recover(&mut self) -> Option<CallId>;
	/// Whether work is expected right now (a call or a tone to play, a
	/// microphone to read). An idle worker is never reported as hung.
	fn active(&self) -> bool;
	/// Called instead of `step` once the worker has failed for good.
	fn disable(&mut self);
}

/// Everything the supervisor shares with the rest of the system.
pub struct Supervision {
	/// Which thread this is.
	pub id: WorkerId,
	/// Liveness counter.
	pub heartbeat: Arc<Heartbeat>,
	/// Set once the worker stopped for good.
	pub failed: Arc<AtomicBool>,
	/// Where failures are reported.
	pub report: Arc<dyn Fn(WorkerFailure) + Send + Sync>,
	/// Cleared by the owner to ask the worker to exit.
	pub stop: Arc<AtomicBool>,
	/// Whether to ask the OS for a higher priority.
	pub promote: bool,
}

/// The text of a panic payload, for the log.
fn describe(payload: &(dyn Any + Send)) -> String {
	payload
		.downcast_ref::<&str>()
		.map(|s| (*s).to_owned())
		.or_else(|| payload.downcast_ref::<String>().cloned())
		.unwrap_or_else(|| "non-string panic payload".to_owned())
}

/// Runs `worker` until `supervision.stop`, containing panics.
pub fn run<W: Supervised>(mut worker: W, supervision: &Supervision, kind: WorkerKind) {
	let _guard = promote(supervision, kind);
	let mut recent: VecDeque<Instant> = VecDeque::new();
	while !supervision.stop.load(Ordering::Acquire) {
		if supervision.failed.load(Ordering::Acquire) {
			worker.disable();
		} else if let Err(payload) = {
			// A step that never returns is judged: it counts as active.
			supervision.heartbeat.set_active(true);
			catch_unwind(AssertUnwindSafe(|| worker.step()))
		} {
			let now = Instant::now();
			recent.retain(|t| now.duration_since(*t) < RESTART_WINDOW);
			recent.push_back(now);
			let permanent = recent.len() > RESTARTS_MAX;
			let call = catch_unwind(AssertUnwindSafe(|| worker.recover())).unwrap_or(None);
			tracing::error!(
				worker = ?supervision.id,
				panic = %describe(payload.as_ref()),
				permanent,
				"media worker panicked"
			);
			if permanent {
				supervision.failed.store(true, Ordering::Release);
			}
			(supervision.report)(WorkerFailure {
				worker: supervision.id,
				kind: FailureKind::Panic,
				permanent,
				call,
			});
		}
		supervision.heartbeat.set_active(worker.active());
		supervision.heartbeat.beat();
		thread::park_timeout(worker.wait());
	}
}

fn promote(supervision: &Supervision, kind: WorkerKind) -> Option<PriorityGuard> {
	if !supervision.promote {
		return None;
	}
	let (promotion, guard) = relay_audio::priority::promote_current_thread(
		kind,
		relay_audio::FRAME_SAMPLES as u32,
		relay_audio::INTERNAL_RATE,
	);
	log_promotion(kind, &promotion);
	matches!(promotion, Promotion::Promoted(_)).then_some(guard)
}

/// A worker the watchdog observes.
pub struct Watched {
	/// Which thread.
	pub id: WorkerId,
	/// Its liveness counter.
	pub heartbeat: Arc<Heartbeat>,
	/// Set when it already failed for good (nothing more to report).
	pub failed: Arc<AtomicBool>,
}

/// Longest a worker may go without a heartbeat while it has work. Idle workers
/// wake at least every 500 ms; an idle worker is not judged at all.
pub const HANG_AFTER: Duration = Duration::from_secs(3);

/// Reports a worker whose heartbeat stops advancing while it is expected to
/// run, once per hang. Runs until `stop`; the check period is `every`.
///
/// A check that comes much later than `every` means the whole process was
/// frozen (phone asleep, app cached by the system, laptop suspended), not that
/// a worker hung: every baseline restarts, so the workers get a fresh
/// `hang_after` to beat.
pub async fn watchdog(
	workers: Vec<Watched>,
	stop: Arc<AtomicBool>,
	report: Arc<dyn Fn(WorkerFailure) + Send + Sync>,
	every: Duration,
	hang_after: Duration,
) {
	let start = Instant::now();
	let mut last_check = start;
	let mut seen: Vec<(u64, Instant, bool)> = workers.iter().map(|_| (0, start, false)).collect();
	while !stop.load(Ordering::Acquire) {
		tokio::time::sleep(every).await;
		let now = Instant::now();
		let overslept = now.duration_since(last_check);
		last_check = now;
		let frozen = overslept >= every + hang_after / 2;
		if frozen {
			tracing::info!(
				overslept_ms = overslept.as_millis() as u64,
				"media watchdog woke late; the process was suspended, not a worker hung"
			);
		}
		for (worker, state) in workers.iter().zip(seen.iter_mut()) {
			let count = worker.heartbeat.count();
			if count != state.0 || frozen || !worker.heartbeat.is_active() {
				*state = (count, now, false);
			} else if !state.2
				&& now.duration_since(state.1) >= hang_after
				&& !worker.failed.load(Ordering::Acquire)
			{
				state.2 = true;
				tracing::error!(
					worker = ?worker.id,
					expected = "a heartbeat at least every 500 ms while active",
					silent_ms = now.duration_since(state.1).as_millis() as u64,
					beats = count,
					"media worker is not responding"
				);
				report(WorkerFailure {
					worker: worker.id,
					kind: FailureKind::Hang,
					permanent: false,
					call: None,
				});
			}
		}
	}
}
