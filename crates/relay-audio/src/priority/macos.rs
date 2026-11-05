//! macOS promotion: `THREAD_TIME_CONSTRAINT_POLICY` with QoS user-interactive as the fallback.
//! The only `unsafe` for this platform lives here.
//!
//! Apple documents the time-constraint policy (period, computation, constraint, preemptible in
//! mach absolute time units) in the Kernel Programming Guide, "Mach Scheduling and Thread
//! Interfaces". Restoring uses `THREAD_STANDARD_POLICY`, which returns the thread to timeshare.

use super::PriorityError;
use super::WorkerKind;
use super::policy::{OsPriority, RtTiming};

/// The macOS implementation of [`OsPriority`].
#[derive(Default)]
pub struct System;

/// What to restore: the QoS class the thread had and whether a time-constraint was set.
pub struct Undo {
	qos: libc::qos_class_t,
	relative: libc::c_int,
	time_constraint: bool,
}

/// Converts nanoseconds to mach absolute time units; `None` if the timebase is unusable.
fn ns_to_abs(ns: u64) -> Option<u32> {
	let mut base = mach2::mach_time::mach_timebase_info_data_t { numer: 0, denom: 0 };
	// SAFETY: `base` is a valid, writable `mach_timebase_info` for the duration of the call.
	let status = unsafe { mach2::mach_time::mach_timebase_info(&raw mut base) };
	if status != 0 || base.numer == 0 || base.denom == 0 {
		return None;
	}
	let abs = u128::from(ns) * u128::from(base.denom) / u128::from(base.numer);
	u32::try_from(abs).ok()
}

fn current_qos() -> (libc::qos_class_t, libc::c_int) {
	let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
	let mut relative: libc::c_int = 0;
	// SAFETY: both out pointers are valid for writes; the thread is the calling thread.
	let status = unsafe {
		libc::pthread_get_qos_class_np(libc::pthread_self(), &raw mut class, &raw mut relative)
	};
	if status == 0 {
		(class, relative)
	} else {
		(libc::qos_class_t::QOS_CLASS_DEFAULT, 0)
	}
}

fn set_qos(class: libc::qos_class_t, relative: libc::c_int) -> Result<(), PriorityError> {
	// SAFETY: sets the QoS class of the calling thread only; no pointers are passed.
	let status = unsafe { libc::pthread_set_qos_class_self_np(class, relative) };
	if status == 0 {
		Ok(())
	} else {
		Err(PriorityError::Os {
			call: "pthread_set_qos_class_self_np",
			detail: format!("error {status}"),
		})
	}
}

fn set_time_constraint(timing: RtTiming) -> Result<(), PriorityError> {
	let invalid = PriorityError::InvalidTiming { frames: 0, rate: 0 };
	let (Some(period), Some(computation), Some(constraint)) = (
		ns_to_abs(timing.period_ns),
		ns_to_abs(timing.computation_ns),
		ns_to_abs(timing.constraint_ns),
	) else {
		return Err(invalid);
	};
	let mut policy = libc::thread_time_constraint_policy {
		period,
		computation,
		constraint,
		preemptible: 1,
	};
	// SAFETY: `mach_thread_self`-style access through `pthread_mach_thread_np` borrows the
	// calling thread's port without taking a reference; `policy` is a live, correctly sized
	// `thread_time_constraint_policy` and the count matches its size in integers.
	let status = unsafe {
		libc::thread_policy_set(
			libc::pthread_mach_thread_np(libc::pthread_self()),
			libc::THREAD_TIME_CONSTRAINT_POLICY as libc::thread_policy_flavor_t,
			(&raw mut policy).cast(),
			libc::THREAD_TIME_CONSTRAINT_POLICY_COUNT,
		)
	};
	if status == 0 {
		Ok(())
	} else {
		Err(PriorityError::Os {
			call: "thread_policy_set(THREAD_TIME_CONSTRAINT_POLICY)",
			detail: format!("kern_return_t {status}"),
		})
	}
}

fn set_standard_policy() {
	let mut policy = libc::thread_standard_policy { no_data: 0 };
	// SAFETY: as in `set_time_constraint`; the standard policy carries no data (count 0).
	unsafe {
		libc::thread_policy_set(
			libc::pthread_mach_thread_np(libc::pthread_self()),
			libc::THREAD_STANDARD_POLICY as libc::thread_policy_flavor_t,
			(&raw mut policy).cast(),
			libc::THREAD_STANDARD_POLICY_COUNT as libc::mach_msg_type_number_t,
		);
	}
}

impl OsPriority for System {
	type Undo = Undo;

	fn realtime(
		&mut self,
		_kind: WorkerKind,
		timing: RtTiming,
	) -> Result<(&'static str, Undo), PriorityError> {
		let (qos, relative) = current_qos();
		// QoS first: setting it afterwards could replace the time-constraint policy.
		set_qos(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)?;
		if let Err(error) = set_time_constraint(timing) {
			let _ = set_qos(qos, relative);
			return Err(error);
		}
		Ok((
			"THREAD_TIME_CONSTRAINT_POLICY + QOS_CLASS_USER_INTERACTIVE",
			Undo {
				qos,
				relative,
				time_constraint: true,
			},
		))
	}

	fn elevated(&mut self, _kind: WorkerKind) -> Result<(&'static str, Undo), PriorityError> {
		let (qos, relative) = current_qos();
		set_qos(libc::qos_class_t::QOS_CLASS_USER_INTERACTIVE, 0)?;
		Ok((
			"QOS_CLASS_USER_INTERACTIVE (time-constraint policy refused)",
			Undo {
				qos,
				relative,
				time_constraint: false,
			},
		))
	}

	fn restore(&mut self, undo: Undo) {
		if undo.time_constraint {
			set_standard_policy();
		}
		let _ = set_qos(undo.qos, undo.relative);
	}
}
