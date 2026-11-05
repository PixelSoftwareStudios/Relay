//! RT-03 on macOS: promotion is visible through `thread_policy_get` and undone on drop.
#![cfg(target_os = "macos")]

use std::thread;

use relay_audio::priority::{WorkerKind, promote_current_thread};

/// Reads the calling thread's time-constraint policy; the flag is true when it is the default
/// (that is, no time-constraint policy is set).
fn time_constraint() -> (libc::thread_time_constraint_policy, bool) {
	let mut policy = libc::thread_time_constraint_policy {
		period: 0,
		computation: 0,
		constraint: 0,
		preemptible: 0,
	};
	let mut count = libc::THREAD_TIME_CONSTRAINT_POLICY_COUNT;
	let mut is_default: libc::boolean_t = 0;
	// SAFETY: the port is the calling thread's (borrowed, no reference taken); `policy`, `count`
	// and `is_default` are live and sized for the time-constraint flavor.
	let status = unsafe {
		libc::thread_policy_get(
			libc::pthread_mach_thread_np(libc::pthread_self()),
			libc::THREAD_TIME_CONSTRAINT_POLICY as libc::thread_policy_flavor_t,
			(&raw mut policy).cast(),
			&raw mut count,
			&raw mut is_default,
		)
	};
	assert_eq!(status, 0);
	(policy, is_default != 0)
}

#[test]
fn a_promoted_thread_has_a_time_constraint_and_drop_removes_it() {
	thread::spawn(|| {
		assert!(time_constraint().1, "a fresh thread has the default policy");
		let (promotion, guard) = promote_current_thread(WorkerKind::Capture, 480, 48_000);
		assert!(promotion.is_promoted(), "{promotion:?}");
		let (policy, is_default) = time_constraint();
		assert!(
			!is_default,
			"time-constraint policy should be set: {promotion:?}"
		);
		assert!(policy.period > 0 && policy.computation > 0);
		assert!(policy.computation <= policy.constraint && policy.constraint <= policy.period);
		drop(guard);
		assert!(
			time_constraint().1,
			"drop must return the thread to timeshare"
		);
	})
	.join()
	.unwrap();
}
