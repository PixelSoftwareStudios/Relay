use proptest::prelude::*;
use relay_protocol::PacketNumberError;
use relay_protocol::packet_number::{PacketNumber, PacketNumberSpace, PacketReplay};
use relay_protocol::seq::{ReplayVerdict, ReplayWindow, SeqExtender, TimestampExtender};

#[test]
fn sequence_wraps_monotonically() {
	let mut ext = SeqExtender::new();
	let mut last = 0;
	for n in 0u32..200_000 {
		let e = ext.extend(n as u16);
		if n > 0 {
			assert_eq!(e, last + 1);
		}
		last = e;
	}
}

#[test]
fn timestamp_wraps_monotonically() {
	let mut ext = TimestampExtender::new();
	let start = u32::MAX - 480 * 5;
	let mut last = ext.extend(start);
	for i in 1..40u32 {
		let e = ext.extend(start.wrapping_add(i * 480));
		assert_eq!(e, last + 480);
		last = e;
	}
}

#[test]
fn reorder_across_wrap_resolves_backwards() {
	let mut ext = SeqExtender::new();
	let before = ext.extend(65_534);
	let after = ext.extend(1);
	assert_eq!(after, before + 3);
	assert_eq!(ext.extend(65_535), before + 1);
	// A reordered packet from before the first one does not underflow.
	let mut fresh = SeqExtender::new();
	let first = fresh.extend(0);
	assert_eq!(fresh.resolve(65_535), first - 1);
}

#[test]
fn replay_window_semantics() {
	let mut w = ReplayWindow::<2>::new();
	assert_eq!(w.check(100), ReplayVerdict::Fresh);
	w.accept(100);
	assert_eq!(w.check(100), ReplayVerdict::Duplicate);
	w.accept(90);
	assert_eq!(w.check(90), ReplayVerdict::Duplicate);
	assert_eq!(w.check(91), ReplayVerdict::Fresh);
	w.accept(300);
	assert_eq!(w.check(100), ReplayVerdict::TooOld);
	assert_eq!(w.check(300 - 127), ReplayVerdict::Fresh);
	assert_eq!(w.check(300 - 128), ReplayVerdict::TooOld);
}

proptest! {
	#[test]
	fn extension_recovers_any_jitter_within_half_range(
		start in 0u64..1_000_000, steps in proptest::collection::vec(-300i64..600, 1..200)
	) {
		let mut ext = SeqExtender::new();
		let mut truth = start + 70_000;
		let first = ext.extend(truth as u16);
		let origin = truth;
		for step in steps {
			truth = (truth as i64 + step).max(70_000) as u64;
			prop_assert_eq!(ext.extend(truth as u16) as i64 - first as i64, truth as i64 - origin as i64);
		}
	}

	#[test]
	fn replay_window_matches_set_model(ops in proptest::collection::vec(0u64..400, 1..300)) {
		let mut w = ReplayWindow::<2>::new();
		let mut seen = std::collections::BTreeSet::new();
		let mut highest: Option<u64> = None;
		for n in ops {
			let verdict = w.check(n);
			let expected = match highest {
				Some(h) if n <= h && h - n >= 128 => ReplayVerdict::TooOld,
				_ if seen.contains(&n) => ReplayVerdict::Duplicate,
				_ => ReplayVerdict::Fresh,
			};
			prop_assert_eq!(verdict, expected);
			if verdict == ReplayVerdict::Fresh {
				w.accept(n);
				seen.insert(n);
				highest = Some(highest.map_or(n, |h| h.max(n)));
			}
		}
	}
}

#[test]
fn packet_numbers_are_unique_and_exhaust() {
	let mut space = PacketNumberSpace::with_limit(3);
	let a = space.allocate().unwrap();
	let b = space.allocate().unwrap();
	assert!(b > a);
	assert_eq!(space.remaining(), 1);
	space.allocate().unwrap();
	assert_eq!(space.allocate(), Err(PacketNumberError::Exhausted));
}

#[test]
fn nonce_roundtrips_and_separates_lanes() {
	let n = PacketNumber(0x0102_0304_0506_0708);
	let nonce = n.nonce(7);
	assert_eq!(PacketNumber::from_nonce(&nonce), (7, n));
	assert_ne!(n.nonce(1), n.nonce(2));
	assert_ne!(PacketNumber(1).nonce(1), PacketNumber(2).nonce(1));
}

#[test]
fn packet_replay_rejects_duplicates_only_after_accept() {
	let mut r = PacketReplay::new();
	let n = PacketNumber(5);
	assert_eq!(r.check(n), ReplayVerdict::Fresh);
	assert_eq!(r.check(n), ReplayVerdict::Fresh);
	r.accept(n);
	assert_eq!(r.check(n), ReplayVerdict::Duplicate);
	r.accept(PacketNumber(5 + 2000));
	assert_eq!(r.check(n), ReplayVerdict::TooOld);
}
