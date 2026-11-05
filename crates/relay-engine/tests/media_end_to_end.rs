//! Two complete in-process engines: real loopback sockets and Noise links,
//! fake audio hardware, the real domain state machine deciding every gate.

mod support_e2e;
mod support_media;

use std::time::Duration;

use relay_core::*;
use support_e2e::{Node, Pair, active_call, run_for, until};
use support_media::{power_at, rms};

const TONE_A: f32 = 440.0;
const TONE_B: f32 = 1_700.0;

/// The newest `ms` milliseconds of what a node's speaker played.
fn tail(node: &Node, ms: usize) -> Vec<f32> {
	let heard = node.sim.heard();
	heard[heard.len().saturating_sub(48 * ms)..].to_vec()
}

fn route(node: &mut Node, call: CallId, lease: ControllerLeaseId, route: TransmitRoute) {
	node.command(Command::SetTransmitRoute { call, lease, route })
		.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_carries_audio_both_ways_only_after_consent_and_stops_on_withdrawal() {
	let Pair {
		mut a,
		mut b,
		call,
		lease_a,
		lease_b,
	} = active_call().await;

	// Accepted and active, but nobody consented: nothing is sent either way.
	run_for(&mut a, &mut b, Duration::from_millis(600)).await;
	assert_eq!(a.media.report(call).unwrap().sent, 0);
	assert_eq!(b.media.report(call).unwrap().sent, 0);
	assert!(rms(&tail(&b, 300)) < 1e-4, "silence before consent");

	// A consents: B hears A's tone, A hears nothing.
	route(&mut a, call, lease_a, TransmitRoute::open_mic());
	until(&mut a, &mut b, "A's gate opens", |a, _| a.gate_open(call)).await;
	run_for(&mut a, &mut b, Duration::from_millis(1_200)).await;
	let at_b = tail(&b, 500);
	assert!(rms(&at_b) > 0.05, "B hears A");
	assert!(power_at(&at_b, TONE_A) > 20.0 * power_at(&at_b, TONE_B));
	assert!(rms(&tail(&a, 300)) < 1e-4, "B never consented");
	assert_eq!(b.media.report(call).unwrap().sent, 0);

	// B consents too: audio flows both ways, and each call carries its own tone.
	route(&mut b, call, lease_b, TransmitRoute::open_mic());
	until(&mut a, &mut b, "B's gate opens", |_, b| b.gate_open(call)).await;
	run_for(&mut a, &mut b, Duration::from_millis(1_200)).await;
	let at_a = tail(&a, 500);
	assert!(rms(&at_a) > 0.05, "A hears B");
	assert!(power_at(&at_a, TONE_B) > 20.0 * power_at(&at_a, TONE_A));

	// Per-call mute closes A's gate: sending stops within a packet or two.
	a.command(Command::SetCallControls {
		call,
		lease: lease_a,
		muted: Some(true),
		deafened: None,
		gain: None,
	})
	.unwrap();
	assert!(!a.gate_open(call));
	run_for(&mut a, &mut b, Duration::from_millis(100)).await;
	let sent = a.media.report(call).unwrap().sent;
	run_for(&mut a, &mut b, Duration::from_millis(500)).await;
	assert_eq!(
		a.media.report(call).unwrap().sent,
		sent,
		"mute stops sending"
	);
	a.command(Command::SetCallControls {
		call,
		lease: lease_a,
		muted: Some(false),
		deafened: None,
		gain: None,
	})
	.unwrap();

	// Consent withdrawal on B closes the gate and the microphone.
	route(&mut b, call, lease_b, TransmitRoute::RECEIVE_ONLY);
	assert!(!b.gate_open(call));
	run_for(&mut a, &mut b, Duration::from_millis(100)).await;
	let sent = b.media.report(call).unwrap().sent;
	run_for(&mut a, &mut b, Duration::from_millis(500)).await;
	assert_eq!(b.media.report(call).unwrap().sent, sent);
	assert!(
		rms(&tail(&a, 200)) < 1e-3,
		"A hears silence after withdrawal"
	);

	// Hangup retires media on both sides.
	a.command(Command::EndCall {
		call,
		lease: lease_a,
	})
	.unwrap();
	until(&mut a, &mut b, "both ended", |a, b| {
		a.call_state(call) == Some(CallState::Ended) && b.call_state(call) == Some(CallState::Ended)
	})
	.await;
	assert!(a.media.report(call).is_none() && b.media.report(call).is_none());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn losing_the_microphone_pauses_sending_and_waits_for_the_user() {
	let Pair {
		mut a,
		mut b,
		call,
		lease_a,
		..
	} = active_call().await;
	route(&mut a, call, lease_a, TransmitRoute::open_mic());
	until(&mut a, &mut b, "A transmitting", |a, _| {
		a.gate_open(call) && a.media.report(call).is_some_and(|r| r.sent > 20)
	})
	.await;

	a.sim.unplug_microphone();
	until(&mut a, &mut b, "gate closes on device loss", |a, _| {
		!a.gate_open(call)
	})
	.await;
	run_for(&mut a, &mut b, Duration::from_millis(150)).await;
	let sent = a.media.report(call).unwrap().sent;
	let opens = |n: &Node| {
		n.effects
			.iter()
			.filter(|e| matches!(e, Effect::OpenCapture { .. }))
			.count()
	};
	let opens_before = opens(&a);
	run_for(&mut a, &mut b, Duration::from_millis(600)).await;
	assert_eq!(a.media.report(call).unwrap().sent, sent, "nothing is sent");
	assert_eq!(opens(&a), opens_before, "no device is opened automatically");
	assert!(!a.gate_open(call));

	// Only an explicit resume reopens capture.
	a.command(Command::ResumeCapture).unwrap();
	assert_eq!(opens(&a), opens_before + 1);
}
