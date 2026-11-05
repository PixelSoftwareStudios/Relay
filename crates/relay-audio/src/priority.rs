//! Scheduling priority for the media worker threads (never called from a device callback).
//!
//! macOS gets the user-interactive QoS class. Linux, Windows and Android go through
//! `audio_thread_priority` (MMCSS or `THREAD_PRIORITY_TIME_CRITICAL` on Windows,
//! `SCHED_FIFO` via `RLIMIT_RTPRIO` on Linux; rtkit is not used because it needs D-Bus at
//! build time). Every failure is reported, never fatal: the thread keeps running at its
//! default priority and the caller logs the outcome once, off the audio path.

/// Which worker is being promoted; sizes the period hint given to the OS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkerKind {
	Capture,
	Render,
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

/// Keeps a promotion alive; dropping it restores the previous priority where the OS allows.
pub struct PriorityGuard {
	#[cfg(any(target_os = "linux", target_os = "windows", target_os = "android"))]
	handle: Option<audio_thread_priority::RtPriorityHandle>,
}

impl Drop for PriorityGuard {
	fn drop(&mut self) {
		#[cfg(any(target_os = "linux", target_os = "windows", target_os = "android"))]
		if let Some(handle) = self.handle.take() {
			let _ = audio_thread_priority::demote_current_thread_from_real_time(handle);
		}
	}
}

/// Promotes the calling thread. Call once at the top of a worker loop.
pub fn promote_current_thread(
	_kind: WorkerKind,
	period_frames: u32,
	sample_rate: u32,
) -> (Promotion, PriorityGuard) {
	imp(period_frames, sample_rate)
}

#[cfg(target_os = "macos")]
fn imp(_period_frames: u32, _sample_rate: u32) -> (Promotion, PriorityGuard) {
	// SAFETY: sets the QoS class of the calling thread only; no pointers are passed.
	let status = unsafe {
		libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)
	};
	let outcome = if status == 0 {
		Promotion::Promoted("QOS_CLASS_USER_INTERACTIVE")
	} else {
		Promotion::Denied(format!("pthread_set_qos_class_self_np returned {status}"))
	};
	(outcome, PriorityGuard {})
}

#[cfg(any(target_os = "linux", target_os = "windows", target_os = "android"))]
fn imp(period_frames: u32, sample_rate: u32) -> (Promotion, PriorityGuard) {
	match audio_thread_priority::promote_current_thread_to_real_time(period_frames, sample_rate) {
		Ok(handle) => (
			Promotion::Promoted(if cfg!(windows) {
				"MMCSS / time-critical"
			} else {
				"real-time scheduling"
			}),
			PriorityGuard {
				handle: Some(handle),
			},
		),
		Err(error) => (
			Promotion::Denied(error.to_string()),
			PriorityGuard { handle: None },
		),
	}
}

#[cfg(not(any(
	target_os = "macos",
	target_os = "linux",
	target_os = "windows",
	target_os = "android"
)))]
fn imp(_period_frames: u32, _sample_rate: u32) -> (Promotion, PriorityGuard) {
	(Promotion::Unsupported, PriorityGuard {})
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
