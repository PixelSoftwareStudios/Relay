mod support_media;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;

use relay_core::{AudioGeneration, CallId, RouteGeneration};
use relay_engine::media::{Emit, FrameStamp, GateBoard};
use support_media::{Wire, closed_gate, open_gate, revision, sender, tone};

const CALL: CallId = CallId::from_u128(1);

fn stamp(capture: u64, route: u64) -> FrameStamp {
	FrameStamp {
		capture: AudioGeneration::new(capture),
		route: RouteGeneration::new(route),
		clean: true,
	}
}

#[test]
fn a_new_sender_is_closed_and_sends_nothing() {
	let wire = Wire::default();
	let (mut sender, _gate) = sender(CALL, 7, &wire);
	assert_eq!(sender.emit(&tone(440.0, 0), stamp(0, 0), 0), Emit::Closed);
	assert_eq!(wire.len(), 0);
}

#[test]
fn open_gate_sends_and_matching_generations_are_required() {
	let wire = Wire::default();
	let (mut sender, gate) = sender(CALL, 7, &wire);
	gate.publish(open_gate(CALL, 2, 3), revision(1));
	assert_eq!(sender.emit(&tone(440.0, 0), stamp(3, 2), 0), Emit::Sent);
	// A frame stamped before the route or capture changed is stale.
	assert_eq!(sender.emit(&tone(440.0, 1), stamp(3, 1), 480), Emit::Stale);
	assert_eq!(sender.emit(&tone(440.0, 2), stamp(2, 2), 960), Emit::Stale);
	assert_eq!(wire.len(), 1);
}

#[test]
fn a_route_bump_invalidates_frames_stamped_under_the_old_route() {
	let wire = Wire::default();
	let (mut sender, gate) = sender(CALL, 7, &wire);
	gate.publish(open_gate(CALL, 1, 1), revision(1));
	let _ = sender.stamp(AudioGeneration::new(1), false);
	let old = sender.stamp(AudioGeneration::new(1), false);
	// Consent withdrawn and re-granted under a new route generation.
	gate.publish(open_gate(CALL, 2, 1), revision(3));
	assert_eq!(sender.emit(&tone(440.0, 0), old, 0), Emit::Stale);
	let _ = sender.stamp(AudioGeneration::new(1), false);
	let fresh = sender.stamp(AudioGeneration::new(1), false);
	assert_eq!(sender.emit(&tone(440.0, 1), fresh, 480), Emit::Sent);
}

#[test]
fn older_revisions_never_override_newer_ones() {
	let wire = Wire::default();
	let (mut sender, gate) = sender(CALL, 7, &wire);
	gate.publish(closed_gate(CALL, 1, 1), revision(5));
	gate.publish(open_gate(CALL, 1, 1), revision(4));
	assert_eq!(sender.emit(&tone(440.0, 0), stamp(1, 1), 0), Emit::Closed);
}

#[test]
fn rtp_sequence_is_contiguous_and_marker_marks_each_spurt() {
	let wire = Wire::default();
	let (mut sender, gate) = sender(CALL, 7, &wire);
	gate.publish(open_gate(CALL, 1, 1), revision(1));
	for i in 0..3u64 {
		sender.emit(&tone(440.0, i), stamp(1, 1), i * 480);
	}
	gate.publish(closed_gate(CALL, 1, 1), revision(2));
	sender.emit(&tone(440.0, 3), stamp(1, 1), 1440);
	gate.publish(open_gate(CALL, 1, 1), revision(3));
	sender.emit(&tone(440.0, 4), stamp(1, 1), 1920);
	let packets = wire.take();
	let headers: Vec<_> = packets
		.iter()
		.map(|p| {
			relay_protocol::rtp::RtpProfile::V1
				.parse(p.rtp())
				.unwrap()
				.header
		})
		.collect();
	let seqs: Vec<u16> = headers.iter().map(|h| h.sequence).collect();
	let markers: Vec<bool> = headers.iter().map(|h| h.marker).collect();
	assert_eq!(seqs, [0, 1, 2, 3]);
	assert_eq!(markers, [true, false, false, true]);
	assert!(headers.iter().all(|h| h.ssrc == 7));
}

#[test]
fn closing_the_gate_stops_a_concurrently_running_sender_within_one_packet() {
	let wire = Wire::default();
	let (mut sender, gate) = sender(CALL, 7, &wire);
	gate.publish(open_gate(CALL, 1, 1), revision(1));
	let stop = Arc::new(AtomicBool::new(false));
	let worker = {
		let stop = Arc::clone(&stop);
		thread::spawn(move || {
			let mut i = 0u64;
			while !stop.load(Ordering::Acquire) {
				sender.emit(&tone(440.0, i), stamp(1, 1), i * 480);
				i += 1;
			}
		})
	};
	while wire.len() < 5 {
		thread::yield_now();
	}
	gate.publish(closed_gate(CALL, 1, 1), revision(2));
	let at_close = wire.len();
	thread::sleep(std::time::Duration::from_millis(50));
	stop.store(true, Ordering::Release);
	worker.join().unwrap();
	// At most the packet already past its final gate check may follow.
	assert!(
		wire.len() <= at_close + 1,
		"{} > {}",
		wire.len(),
		at_close + 1
	);
}

#[test]
fn board_applies_updates_that_arrived_before_the_call_was_prepared() {
	let board = GateBoard::new();
	board.publish(open_gate(CALL, 4, 9), revision(2));
	let cell = board.cell(CALL);
	assert!(cell.permits(stamp(9, 4)));
	board.remove(CALL);
	assert!(
		!cell.permits(stamp(9, 4)),
		"a removed call's gate is closed"
	);
	// A fresh cell after removal starts closed.
	assert!(!board.cell(CALL).permits(stamp(9, 4)));
}
