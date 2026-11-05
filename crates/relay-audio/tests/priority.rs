//! RT-03: the promotion shim reports honestly and never panics or blocks.

use std::thread;

#[allow(unused_imports)]
use relay_audio::priority::{Promotion, WorkerKind, promote_current_thread};

#[test]
fn promotion_reports_an_outcome_on_a_worker_thread() {
	let outcome = thread::spawn(|| {
		let (promotion, guard) = promote_current_thread(WorkerKind::Capture, 480, 48_000);
		drop(guard);
		promotion
	})
	.join()
	.expect("promotion must not panic");
	#[cfg(target_os = "macos")]
	assert!(
		outcome.is_promoted(),
		"QoS should always be accepted: {outcome:?}"
	);
	#[cfg(not(target_os = "macos"))]
	assert!(matches!(
		outcome,
		Promotion::Promoted(_) | Promotion::Denied(_) | Promotion::Unsupported
	));
}

#[test]
fn a_zero_rate_is_a_denial_not_a_panic() {
	let outcome = thread::spawn(|| promote_current_thread(WorkerKind::Render, 480, 0).0)
		.join()
		.unwrap();
	#[cfg(any(target_os = "linux", target_os = "windows"))]
	assert!(matches!(outcome, Promotion::Denied(_)));
	let _ = outcome;
}
