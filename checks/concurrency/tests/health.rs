//! Model the actual health-cell implementation with Loom's atomics and Arc.
#![cfg(loom)]

// Health recovery only needs the direction's type; it does not inspect its value.
#[allow(dead_code)]
enum Direction {
	Input,
	Output,
}

#[path = "../../../crates/relay-audio/src/device/health.rs"]
#[allow(dead_code)]
mod health;

use health::{StreamHealth, StreamState};

#[test]
fn concurrent_reports_never_lose_a_failure_or_an_xrun() {
	loom::model(|| {
		let health = StreamHealth::default();
		let other = health.clone();
		let reporter = loom::thread::spawn(move || {
			other.report(StreamState::DeviceLost);
			other.report_xrun();
			other.report_realtime_denied();
		});
		health.report(StreamState::Running);
		health.report_xrun();
		reporter.join().unwrap();
		assert_eq!(health.state(), StreamState::DeviceLost);
		assert_eq!(health.xruns(), 2);
		assert!(health.realtime_denied());
	});
}

#[test]
fn reports_publish_monotonically_across_observers() {
	loom::model(|| {
		let health = StreamHealth::default();
		let other = health.clone();
		let reporter = loom::thread::spawn(move || other.report(StreamState::Invalidated));
		let first = health.state();
		let second = health.state();
		if first == StreamState::Invalidated {
			assert_eq!(second, first);
		}
		reporter.join().unwrap();
		assert_eq!(health.state(), StreamState::Invalidated);
	});
}
