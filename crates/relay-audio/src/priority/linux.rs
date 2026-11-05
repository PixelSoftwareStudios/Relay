//! Linux and Android promotion: `SCHED_RR`, then RealtimeKit (feature `rtkit`), then nice.
//!
//! `rustix` (safe) covers the resource limit, the thread id and nice values. It does not expose
//! `sched_setscheduler`, so the scheduler change is the one `unsafe` call here, through
//! `pthread_setschedparam` on the calling thread (see `sched_setscheduler(2)`, `sched(7)`).
//!
//! Real-time priority is `min(RLIMIT_RTPRIO, MAX_RT_PRIORITY)`; a zero limit means the kernel
//! would refuse, so the tier is skipped without a syscall. `SCHED_RESET_ON_FORK` keeps a child
//! process from inheriting real-time scheduling.

use rustix::process::{Resource, getpriority_process, getrlimit, setpriority_process};
use rustix::thread::gettid;

use super::PriorityError;
use super::WorkerKind;
use super::policy::{MAX_RT_PRIORITY, OsPriority, RtTiming, bounded_rt_priority};

/// Nice value requested by the last-resort tier (lower is more favourable; -11 is what the
/// RealtimeKit documentation uses for its high-priority class).
pub const WANTED_NICE: i32 = -11;

/// The Linux implementation of [`OsPriority`].
#[derive(Default)]
pub struct System;

/// What to restore.
pub enum Undo {
	/// Back to `SCHED_OTHER` (always permitted).
	Scheduler,
	/// Nice value before the change (raising it back may be refused; best effort).
	Nice(i32),
	/// Granted by RealtimeKit; demoted through the scheduler like `Scheduler`.
	#[cfg(feature = "rtkit")]
	Broker,
}

fn set_scheduler(policy: libc::c_int, priority: libc::c_int) -> Result<(), libc::c_int> {
	let param = libc::sched_param {
		sched_priority: priority,
	};
	// SAFETY: `param` is a live, initialised `sched_param`; `pthread_self` is the calling thread.
	let status =
		unsafe { libc::pthread_setschedparam(libc::pthread_self(), policy, &raw const param) };
	if status == 0 { Ok(()) } else { Err(status) }
}

fn rt_limit() -> u64 {
	// An unlimited limit is `None`; it still means "as high as the kernel allows", which we cap.
	getrlimit(Resource::Rtprio)
		.current
		.unwrap_or(u64::from(MAX_RT_PRIORITY))
}

fn try_sched_rr() -> Result<(), PriorityError> {
	let limit = rt_limit();
	let Some(priority) = bounded_rt_priority(limit, MAX_RT_PRIORITY) else {
		return Err(PriorityError::NotPermitted("RLIMIT_RTPRIO is 0".into()));
	};
	let policy = libc::SCHED_RR | libc::SCHED_RESET_ON_FORK;
	set_scheduler(policy, priority as libc::c_int).map_err(|code| PriorityError::Os {
		call: "pthread_setschedparam(SCHED_RR)",
		detail: format!("error {code}"),
	})
}

impl OsPriority for System {
	type Undo = Undo;

	fn realtime(
		&mut self,
		kind: WorkerKind,
		_timing: RtTiming,
	) -> Result<(&'static str, Undo), PriorityError> {
		match try_sched_rr() {
			Ok(()) => Ok(("SCHED_RR", Undo::Scheduler)),
			Err(direct) => {
				// Never wait on D-Bus from a device callback.
				#[cfg(feature = "rtkit")]
				if kind != WorkerKind::Callback {
					match super::rtkit::make_realtime(gettid().as_raw_nonzero().get() as u64) {
						Ok(()) => return Ok(("SCHED_RR via RealtimeKit", Undo::Broker)),
						Err(broker) => {
							return Err(PriorityError::NotPermitted(format!(
								"{direct}; RealtimeKit: {broker}"
							)));
						}
					}
				}
				#[cfg(not(feature = "rtkit"))]
				let _ = kind;
				Err(direct)
			}
		}
	}

	fn elevated(&mut self, _kind: WorkerKind) -> Result<(&'static str, Undo), PriorityError> {
		let tid = Some(gettid());
		let previous = getpriority_process(tid).map_err(|e| PriorityError::Os {
			call: "getpriority",
			detail: e.to_string(),
		})?;
		setpriority_process(tid, WANTED_NICE).map_err(|e| PriorityError::Os {
			call: "setpriority",
			detail: e.to_string(),
		})?;
		Ok((
			"nice -11 (real-time scheduling refused)",
			Undo::Nice(previous),
		))
	}

	fn restore(&mut self, undo: Undo) {
		match undo {
			Undo::Scheduler => {
				let _ = set_scheduler(libc::SCHED_OTHER, 0);
			}
			#[cfg(feature = "rtkit")]
			Undo::Broker => {
				let _ = set_scheduler(libc::SCHED_OTHER, 0);
			}
			Undo::Nice(value) => {
				let _ = setpriority_process(Some(gettid()), value);
			}
		}
	}
}
