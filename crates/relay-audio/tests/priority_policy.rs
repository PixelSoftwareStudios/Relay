//! RT-03: promotion policy, tested against an injected fake OS (no syscalls).

use relay_audio::priority::policy::{
	MAX_PERIOD_NS, MAX_RT_PRIORITY, MIN_PERIOD_NS, OsPriority, RtTiming, bounded_rt_priority,
	promote_with,
};
use relay_audio::priority::{PriorityError, Promotion, WorkerKind};

#[derive(Default)]
struct Fake {
	realtime: Option<PriorityError>,
	elevated: Option<PriorityError>,
	seen: Option<RtTiming>,
	restored: Vec<&'static str>,
}

impl OsPriority for Fake {
	type Undo = &'static str;

	fn realtime(
		&mut self,
		_: WorkerKind,
		timing: RtTiming,
	) -> Result<(&'static str, Self::Undo), PriorityError> {
		self.seen = Some(timing);
		self.realtime.take().map_or(Ok(("rt", "undo-rt")), Err)
	}

	fn elevated(&mut self, _: WorkerKind) -> Result<(&'static str, Self::Undo), PriorityError> {
		self.elevated.take().map_or(Ok(("hi", "undo-hi")), Err)
	}

	fn restore(&mut self, undo: Self::Undo) {
		self.restored.push(undo);
	}
}

fn denied() -> PriorityError {
	PriorityError::NotPermitted("no".into())
}

#[test]
fn ten_ms_at_48k_gives_a_quarter_and_half_budget() {
	let t = RtTiming::from_buffer(480, 48_000).unwrap();
	assert_eq!(t.period_ns, 10_000_000);
	assert_eq!(t.computation_ns, 2_500_000);
	assert_eq!(t.constraint_ns, 5_000_000);
}

#[test]
fn timing_is_clamped_and_ordered() {
	for (frames, rate) in [
		(1, 192_000),
		(1, 8_000),
		(100_000, 8_000),
		(u32::MAX, 1),
		(480, u32::MAX),
	] {
		let t = RtTiming::from_buffer(frames, rate).unwrap();
		assert!((MIN_PERIOD_NS..=MAX_PERIOD_NS).contains(&t.period_ns));
		assert!(t.computation_ns <= t.constraint_ns && t.constraint_ns <= t.period_ns);
	}
	assert!(RtTiming::from_buffer(0, 48_000).is_none());
	assert!(RtTiming::from_buffer(480, 0).is_none());
}

#[test]
fn rt_priority_is_bounded_by_the_limit() {
	assert_eq!(bounded_rt_priority(0, 10), None);
	assert_eq!(bounded_rt_priority(3, 10), Some(3));
	assert_eq!(
		bounded_rt_priority(99, MAX_RT_PRIORITY),
		Some(MAX_RT_PRIORITY)
	);
	assert_eq!(bounded_rt_priority(u64::MAX, 10), Some(10));
	assert_eq!(bounded_rt_priority(5, 0), None);
}

#[test]
fn realtime_success_is_promoted_with_an_undo() {
	let mut os = Fake::default();
	let (p, undo) = promote_with(&mut os, WorkerKind::Capture, 480, 48_000);
	assert_eq!(p, Promotion::Promoted("rt"));
	assert_eq!(undo, Some("undo-rt"));
}

#[test]
fn a_refused_realtime_tier_falls_back_to_elevated() {
	let mut os = Fake {
		realtime: Some(denied()),
		..Fake::default()
	};
	let (p, undo) = promote_with(&mut os, WorkerKind::Render, 480, 48_000);
	assert_eq!(p, Promotion::Promoted("hi"));
	assert_eq!(undo, Some("undo-hi"));
}

#[test]
fn invalid_timing_skips_realtime_without_calling_it() {
	let mut os = Fake::default();
	let (p, _) = promote_with(&mut os, WorkerKind::Render, 480, 0);
	assert_eq!(p, Promotion::Promoted("hi"));
	assert!(os.seen.is_none());
}

#[test]
fn both_tiers_refused_is_denied_with_both_reasons() {
	let mut os = Fake {
		realtime: Some(denied()),
		elevated: Some(denied()),
		..Fake::default()
	};
	let (p, undo) = promote_with(&mut os, WorkerKind::Capture, 480, 48_000);
	assert!(undo.is_none());
	let Promotion::Denied(reason) = p else {
		panic!("expected denial")
	};
	assert!(
		reason.contains("real-time") && reason.contains("elevated"),
		"{reason}"
	);
}

#[test]
fn a_platform_without_tiers_is_unsupported() {
	let mut os = Fake {
		realtime: Some(PriorityError::Unavailable),
		elevated: Some(PriorityError::Unavailable),
		..Fake::default()
	};
	let (p, _) = promote_with(&mut os, WorkerKind::Capture, 480, 48_000);
	assert_eq!(p, Promotion::Unsupported);
}
