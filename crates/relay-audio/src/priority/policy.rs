//! Promotion policy, free of syscalls: timing math, tier order and fallbacks.
//!
//! The per-OS modules implement [`OsPriority`]; everything decided here is unit-tested with an
//! injected fake (`tests/priority_policy.rs`).

use super::{PriorityError, Promotion, WorkerKind};

/// Shortest period handed to an OS scheduler.
pub const MIN_PERIOD_NS: u64 = 500_000;
/// Longest period handed to an OS scheduler (a larger buffer is clamped, not refused).
pub const MAX_PERIOD_NS: u64 = 50_000_000;
/// Smallest computation budget, so a tiny period does not ask for an unschedulable slice.
pub const MIN_COMPUTATION_NS: u64 = 100_000;
/// Highest real-time priority we ever request on Linux (the user's limit may be higher).
pub const MAX_RT_PRIORITY: u32 = 10;

/// Real-time timing derived from the device buffer, in nanoseconds.
///
/// `computation <= constraint <= period` always holds. The computation budget is a quarter of the
/// period and the constraint half of it: the workers do light, bounded work per tick, and the
/// kernel is asked to run them early in each period rather than at its very end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RtTiming {
	pub period_ns: u64,
	pub computation_ns: u64,
	pub constraint_ns: u64,
}

impl RtTiming {
	/// Derives the timing, or `None` when frames or rate are zero.
	pub fn from_buffer(frames: u32, rate: u32) -> Option<Self> {
		if frames == 0 || rate == 0 {
			return None;
		}
		let raw = u128::from(frames) * 1_000_000_000 / u128::from(rate);
		let period_ns = u64::try_from(raw)
			.unwrap_or(u64::MAX)
			.clamp(MIN_PERIOD_NS, MAX_PERIOD_NS);
		let constraint_ns = period_ns / 2;
		let computation_ns = (period_ns / 4).max(MIN_COMPUTATION_NS).min(constraint_ns);
		Some(Self {
			period_ns,
			computation_ns,
			constraint_ns,
		})
	}
}

/// Clamps a wanted real-time priority to what the user's limit allows. `None` when the limit is
/// zero (no real-time scheduling permitted) or the wish is zero.
pub fn bounded_rt_priority(limit: u64, wanted: u32) -> Option<u32> {
	let bounded = u32::try_from(limit.min(u64::from(wanted))).unwrap_or(wanted);
	(bounded > 0).then_some(bounded)
}

/// What a platform can do for the calling thread. Every method acts on the calling thread only.
pub trait OsPriority {
	/// What is needed to put the thread back (a handle, the previous priority).
	type Undo;

	/// Real-time tier. Name of the policy on success.
	fn realtime(
		&mut self,
		kind: WorkerKind,
		timing: RtTiming,
	) -> Result<(&'static str, Self::Undo), PriorityError>;

	/// Weaker tier used when real-time is refused or the timing is invalid.
	fn elevated(&mut self, kind: WorkerKind) -> Result<(&'static str, Self::Undo), PriorityError>;

	/// Undoes a promotion. Best effort: a failure is ignored (the thread is usually ending).
	fn restore(&mut self, undo: Self::Undo);
}

/// Runs the tiers in order and reports the honest outcome.
pub fn promote_with<O: OsPriority>(
	os: &mut O,
	kind: WorkerKind,
	period_frames: u32,
	sample_rate: u32,
) -> (Promotion, Option<O::Undo>) {
	let realtime = match RtTiming::from_buffer(period_frames, sample_rate) {
		Some(timing) => os.realtime(kind, timing),
		None => Err(PriorityError::InvalidTiming {
			frames: period_frames,
			rate: sample_rate,
		}),
	};
	let first = match realtime {
		Ok((name, undo)) => return (Promotion::Promoted(name), Some(undo)),
		Err(PriorityError::Unavailable) => None,
		Err(error) => {
			tracing::debug!(worker = ?kind, %error, "real-time tier refused; trying the elevated tier");
			Some(error)
		}
	};
	match os.elevated(kind) {
		Ok((name, undo)) => (Promotion::Promoted(name), Some(undo)),
		Err(PriorityError::Unavailable) if first.is_none() => (Promotion::Unsupported, None),
		Err(error) => {
			let reason = match first {
				Some(first) => format!("real-time: {first}; elevated: {error}"),
				None => error.to_string(),
			};
			(Promotion::Denied(reason), None)
		}
	}
}
