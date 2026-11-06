//! Push-to-talk release delay on the real media send path: audio keeps flowing
//! for the hold after an ordinary key-up, then stops exactly with the gate; any
//! safety event stops it at once.

mod support_e2e;
mod support_media;

use std::time::{Duration, Instant};

use relay_core::*;
use support_e2e::{Node, Pair, active_call, run_for, until};

const HOLD: Duration = Duration::from_millis(200);

fn key(call: CallId, lease: ControllerLeaseId, sequence: u64, pressed: bool) -> Command {
	Command::Ptt(PttUpdate {
		call,
		lease,
		source: InputSourceId::new(1),
		sequence,
		pressed,
		ttl: None,
		immediate: false,
	})
}

fn sent(node: &Node, call: CallId) -> u64 {
	node.media.report(call).map_or(0, |r| r.sent)
}

/// A call whose side A has push-to-talk consent and a press that is held.
async fn talking() -> Pair {
	let mut pair = active_call().await;
	let (call, lease) = (pair.call, pair.lease_a);
	pair.a
		.command(Command::SetTransmitMode {
			call,
			lease,
			route: TransmitRoute::push_to_talk(),
		})
		.unwrap();
	pair.a.command(key(call, lease, 1, true)).unwrap();
	let Pair { a, b, .. } = &mut pair;
	until(a, b, "A is sending", |a, _| {
		a.gate_open(call) && sent(a, call) > 10
	})
	.await;
	pair
}

/// Asserts no packet leaves after `quiet` has passed since the gate closed.
async fn assert_silent(pair: &mut Pair) {
	let Pair { a, b, call, .. } = pair;
	run_for(a, b, Duration::from_millis(100)).await;
	let settled = sent(a, *call);
	run_for(a, b, Duration::from_millis(500)).await;
	assert_eq!(
		sent(a, *call),
		settled,
		"packets leaked after the gate closed"
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn audio_continues_for_the_delay_after_key_up_then_stops_with_the_gate() {
	let mut pair = talking().await;
	let (call, lease) = (pair.call, pair.lease_a);
	let before = sent(&pair.a, call);
	let released = Instant::now();
	pair.a.command(key(call, lease, 2, false)).unwrap();
	assert!(pair.a.gate_open(call), "the hold keeps the gate open");
	let view = pair.a.state.snapshot().calls[&call].clone();
	assert!(view.releasing && view.gate.open);

	let Pair { a, b, .. } = &mut pair;
	run_for(a, b, Duration::from_millis(100)).await;
	assert!(sent(a, call) > before, "still transmitting during the hold");
	until(a, b, "the hold ends", |a, _| !a.gate_open(call)).await;
	let held_for = released.elapsed();
	assert!(
		held_for >= HOLD - Duration::from_millis(10) && held_for < HOLD * 3,
		"gate closed after {held_for:?}"
	);
	assert_silent(&mut pair).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_press_during_the_hold_keeps_transmitting_past_the_old_deadline() {
	let mut pair = talking().await;
	let (call, lease) = (pair.call, pair.lease_a);
	pair.a.command(key(call, lease, 2, false)).unwrap();
	let Pair { a, b, .. } = &mut pair;
	run_for(a, b, Duration::from_millis(100)).await;
	a.command(key(call, lease, 3, true)).unwrap();
	run_for(a, b, HOLD).await;
	assert!(
		a.gate_open(call),
		"the cancelled hold must not close the gate"
	);
	assert!(!a.state.snapshot().calls[&call].releasing);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn safety_events_stop_sending_at_once_even_during_a_hold() {
	// Global mute during the hold.
	let mut pair = talking().await;
	let (call, lease) = (pair.call, pair.lease_a);
	pair.a.command(key(call, lease, 2, false)).unwrap();
	pair.a
		.command(Command::SetGlobalControls {
			muted: Some(true),
			deafened: None,
			output_gain: None,
			ptt_release_delay: None,
			expected: None,
		})
		.unwrap();
	assert!(!pair.a.gate_open(call), "mute closes the same instant");
	assert_silent(&mut pair).await;

	// Microphone unplugged during the hold.
	let mut pair = talking().await;
	let (call, lease) = (pair.call, pair.lease_a);
	pair.a.command(key(call, lease, 2, false)).unwrap();
	pair.a.sim.unplug_microphone();
	let Pair { a, b, .. } = &mut pair;
	until(a, b, "gate closes on device loss", |a, _| {
		!a.gate_open(call)
	})
	.await;
	assert_silent(&mut pair).await;

	// Consent withdrawn during the hold.
	let mut pair = talking().await;
	let (call, lease) = (pair.call, pair.lease_a);
	pair.a.command(key(call, lease, 2, false)).unwrap();
	pair.a
		.command(Command::SetTransmitMode {
			call,
			lease,
			route: TransmitRoute::MUTED,
		})
		.unwrap();
	assert!(!pair.a.gate_open(call));
	assert_silent(&mut pair).await;
}
