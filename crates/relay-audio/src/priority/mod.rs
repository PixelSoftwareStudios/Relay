//! Scheduling priority for the media worker threads (never called from a device callback).
//!
//! Implemented from OS documentation (Apple `thread_policy_set` and `pthread_set_qos_class_self_np`,
//! Microsoft MMCSS and `SetThreadPriority`, Linux `sched_setscheduler(2)`, `setpriority(2)` and the
//! RealtimeKit D-Bus specification); behaviour informed by reviewing the `audio_thread_priority`
//! crate's approach. No code was taken from it, so no MPL obligations apply.
//!
//! The call happens once, on the thread being promoted, before its loop starts. It may make a few
//! syscalls (and, with the `rtkit` feature, one bounded D-Bus round trip) but is never made from
//! an audio callback. Every failure is reported, never fatal and never a panic: the thread keeps
//! running at its previous priority and the caller logs the outcome once, off the audio path.
//!
//! Tiers, tried in order by [`policy::promote_with`]:
//!
//! 1. real-time: macOS `THREAD_TIME_CONSTRAINT_POLICY`; Windows MMCSS "Pro Audio"; Linux
//!    `SCHED_RR` (bounded by `RLIMIT_RTPRIO`, else RealtimeKit when built with `rtkit`);
//! 2. elevated: macOS QoS user-interactive; Windows `THREAD_PRIORITY_TIME_CRITICAL`; Linux
//!    RealtimeKit high priority or a lower nice value;
//! 3. denied: reported as [`Promotion::Denied`] with the reason of the last tier tried.
//!
//! cpal's `realtime` feature is off (it pulls in the MPL-2.0 `audio_thread_priority`), so the
//! device callback threads are promoted by [`promote_callback_thread`] from their first callback;
//! a denial there is recorded in `StreamHealth::realtime_denied` (RT-15) and never pauses audio.

pub mod policy;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(all(target_os = "linux", feature = "rtkit"))]
mod rtkit;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
use linux::System;
#[cfg(target_os = "macos")]
use macos::System;
#[cfg(target_os = "windows")]
use windows::System;

use policy::OsPriority;

/// Which worker is being promoted; sizes the period hint given to the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerKind {
	Capture,
	Render,
	/// A thread created by the audio backend for device callbacks (see [`promote_callback_thread`]).
	Callback,
}

/// What a promotion attempt achieved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Promotion {
	/// The thread now runs under the named policy.
	Promoted(&'static str),
	/// The OS refused (missing privilege, no rtkit, resource limit). The thread is unchanged.
	Denied(String),
	/// This platform has no promotion support here.
	Unsupported,
}

impl Promotion {
	/// Whether the thread was actually promoted.
	pub const fn is_promoted(&self) -> bool {
		matches!(self, Self::Promoted(_))
	}
}

/// Why one tier of promotion failed.
#[derive(Debug, thiserror::Error)]
pub enum PriorityError {
	/// The OS call failed; the text names the call and its error code.
	#[error("{call} failed: {detail}")]
	Os { call: &'static str, detail: String },
	/// The OS or its configuration does not permit this tier (limit is zero, no broker).
	#[error("not permitted: {0}")]
	NotPermitted(String),
	/// The period or sample rate cannot describe a real-time constraint.
	#[error("invalid timing: {frames} frames at {rate} Hz")]
	InvalidTiming { frames: u32, rate: u32 },
	/// This tier does not exist on this platform.
	#[error("not available on this platform")]
	Unavailable,
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
type Undo = <System as OsPriority>::Undo;
#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
type Undo = ();

/// Keeps a promotion alive; dropping it restores the previous priority where the OS allows.
/// Drop it on the thread that was promoted: the restore applies to the calling thread.
#[must_use = "dropping the guard demotes the thread immediately"]
pub struct PriorityGuard {
	undo: Option<Undo>,
}

impl Drop for PriorityGuard {
	fn drop(&mut self) {
		#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
		if let Some(undo) = self.undo.take() {
			System.restore(undo);
		}
	}
}

/// Promotes the calling thread. Call once at the top of a worker loop.
pub fn promote_current_thread(
	kind: WorkerKind,
	period_frames: u32,
	sample_rate: u32,
) -> (Promotion, PriorityGuard) {
	#[cfg(any(target_os = "macos", target_os = "linux", target_os = "windows"))]
	{
		let (promotion, undo) = policy::promote_with(&mut System, kind, period_frames, sample_rate);
		(promotion, PriorityGuard { undo })
	}
	#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
	{
		let _ = (kind, period_frames, sample_rate);
		(Promotion::Unsupported, PriorityGuard { undo: None })
	}
}

/// Promotes the audio thread the backend created for device callbacks, from its first callback.
///
/// The backend (cpal) owns that thread and offers no hook before it starts, so this is called once
/// per stream from inside the first callback, behind a plain `bool` (one predictable branch per
/// callback afterwards). It makes a handful of syscalls and, on failure, builds one reason string;
/// it never uses D-Bus (RealtimeKit is skipped on this path) and never waits. The guard is
/// deliberately forgotten: the thread belongs to the backend and ends with the stream, and
/// restoring from another thread would change the wrong thread.
pub fn promote_callback_thread(period_frames: u32, sample_rate: u32) -> Promotion {
	let (promotion, guard) =
		promote_current_thread(WorkerKind::Callback, period_frames, sample_rate);
	std::mem::forget(guard);
	promotion
}

/// Logs a promotion outcome once. Honest about what happened: a denial is a warning, not an
/// error, because the worker still functions.
pub fn log_promotion(kind: WorkerKind, promotion: &Promotion) {
	match promotion {
		Promotion::Promoted(policy) => {
			tracing::info!(worker = ?kind, policy, "media worker promoted");
		}
		Promotion::Denied(reason) => {
			tracing::warn!(worker = ?kind, reason, "media worker priority promotion denied; running at default priority");
		}
		Promotion::Unsupported => {
			tracing::info!(worker = ?kind, "no priority promotion on this platform");
		}
	}
}
