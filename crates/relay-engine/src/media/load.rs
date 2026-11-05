//! Measured worker load and the admission rule built on it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

/// Exponentially weighted per-frame processing time of one worker, in
/// nanoseconds. Single writer (the worker), any number of readers.
#[derive(Debug, Default)]
pub struct LoadMeter(AtomicU64);

impl LoadMeter {
	/// Folds one observation of "time spent per 10 ms frame" into the average.
	pub fn record(&self, ns_per_frame: u64) {
		let old = self.0.load(Ordering::Relaxed);
		let new = if old == 0 {
			ns_per_frame
		} else {
			// 1/8 weight: reacts within about a second, ignores single spikes.
			old - old / 8 + ns_per_frame / 8
		};
		self.0.store(new, Ordering::Relaxed);
	}

	/// Forgets the average: the worker has nothing to serve, so a transient from
	/// the last call must not weigh on the next admission.
	pub fn reset(&self) {
		self.0.store(0, Ordering::Relaxed);
	}

	/// The current average, zero until something was recorded.
	pub fn ns_per_frame(&self) -> u64 {
		self.0.load(Ordering::Relaxed)
	}
}

/// Admission limits. The call ceiling and the frame budget are design
/// defaults, not measured capacities.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdmissionLimits {
	/// Hard cap on simultaneously prepared calls.
	pub max_calls: usize,
	/// Share of one 10 ms frame each worker may spend, in per-mille.
	pub budget_permille: u32,
	/// Cost assumed for one more call before any call has been measured.
	pub assumed_call_cost: Duration,
}

impl Default for AdmissionLimits {
	fn default() -> Self {
		Self {
			max_calls: relay_audio::MAX_CALLS,
			budget_permille: 500,
			assumed_call_cost: Duration::from_micros(400),
		}
	}
}

/// What the workers look like right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadSnapshot {
	/// Calls with prepared media.
	pub calls: usize,
	/// Measured capture-worker time per frame.
	pub capture_ns: u64,
	/// Measured render-worker time per frame.
	pub render_ns: u64,
}

/// Why a new call was refused. The two kinds are shown differently to users:
/// a policy cap versus a resource shortage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum Refusal {
	/// The configured number of calls is reached.
	#[error("call limit of {0} reached")]
	CallCap(usize),
	/// A worker has no budget left for another call.
	#[error("not enough {worker} budget for another call")]
	WorkerBudget {
		/// Which worker is out of budget.
		worker: &'static str,
	},
}

const FRAME_NS: u64 = 10_000_000;

impl AdmissionLimits {
	/// Decides whether one more call fits without taking anything from the
	/// calls already running: the projected load of each worker, with one
	/// more call's worth of work added, must stay inside the budget.
	pub fn admit(&self, load: LoadSnapshot) -> Result<(), Refusal> {
		if load.calls >= self.max_calls {
			return Err(Refusal::CallCap(self.max_calls));
		}
		let budget = FRAME_NS * u64::from(self.budget_permille) / 1000;
		let assumed = u64::try_from(self.assumed_call_cost.as_nanos()).unwrap_or(u64::MAX);
		for (worker, measured) in [("capture", load.capture_ns), ("render", load.render_ns)] {
			let per_call = match load.calls {
				0 => assumed,
				n => (measured / n as u64).max(assumed / 4),
			};
			if measured.saturating_add(per_call) > budget {
				return Err(Refusal::WorkerBudget { worker });
			}
		}
		Ok(())
	}
}
