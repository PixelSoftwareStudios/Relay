//! Canary tests: each call carries its own tone, and the "network" is hostile
//! and delivers every packet to every receiver. A tone appearing in the wrong
//! call proves cross-forwarding.

mod support_media;

use relay_audio::FRAME_SAMPLES;
use relay_core::{AudioGeneration, CallId, RouteGeneration};
use relay_engine::media::{CallReceiver, FrameStamp, Ingress, RemoteStream, Rendered, SourceId};
use relay_protocol::jitter::JitterConfig;
use support_media::{Wire, open_gate, power_at, revision, rms, sender, tone};

const SOURCE: SourceId = 1;
const CALL_A: CallId = CallId::from_u128(0xA);
const CALL_B: CallId = CallId::from_u128(0xB);
const TONE_A: f32 = 440.0;
const TONE_B: f32 = 1_700.0;

fn stamp() -> FrameStamp {
	FrameStamp {
		capture: AudioGeneration::new(1),
		route: RouteGeneration::new(1),
		clean: true,
	}
}

fn receiver(call: CallId, ssrc: u32) -> CallReceiver {
	CallReceiver::new(
		RemoteStream {
			call,
			ssrc,
			generation: 1,
			source: SOURCE,
		},
		JitterConfig::BALANCED,
	)
	.unwrap()
}

/// Renders `frames` frames into one buffer, feeding `deliver` each tick.
fn run(
	rx: &mut CallReceiver,
	frames: usize,
	mut deliver: impl FnMut(usize, &mut CallReceiver),
) -> Vec<f32> {
	let mut out = Vec::new();
	for i in 0..frames {
		deliver(i, rx);
		let mut frame = [0.0; FRAME_SAMPLES];
		rx.render(&mut frame);
		out.extend_from_slice(&frame);
	}
	out
}

#[test]
fn each_call_hears_only_its_own_tone_on_a_hostile_network() {
	let (wire_a, wire_b) = (Wire::default(), Wire::default());
	let (mut tx_a, gate_a) = sender(CALL_A, 0xAAAA, &wire_a);
	let (mut tx_b, gate_b) = sender(CALL_B, 0xBBBB, &wire_b);
	gate_a.publish(open_gate(CALL_A, 1, 1), revision(1));
	gate_b.publish(open_gate(CALL_B, 1, 1), revision(1));
	let (mut rx_a, mut rx_b) = (receiver(CALL_A, 0xAAAA), receiver(CALL_B, 0xBBBB));

	let (mut out_a, mut out_b) = (Vec::new(), Vec::new());
	for i in 0..60u64 {
		tx_a.emit(&tone(TONE_A, i), stamp(), i * 480);
		tx_b.emit(&tone(TONE_B, i), stamp(), i * 480);
		let packets: Vec<Ingress> = wire_a.take().into_iter().chain(wire_b.take()).collect();
		// Every packet goes to both receivers.
		for p in &packets {
			rx_a.ingest(SOURCE, p);
			rx_b.ingest(SOURCE, p);
		}
		let (mut fa, mut fb) = ([0.0; FRAME_SAMPLES], [0.0; FRAME_SAMPLES]);
		rx_a.render(&mut fa);
		rx_b.render(&mut fb);
		out_a.extend_from_slice(&fa);
		out_b.extend_from_slice(&fb);
	}
	// Skip the prefill and the codec warm-up.
	let (heard_a, heard_b) = (&out_a[FRAME_SAMPLES * 10..], &out_b[FRAME_SAMPLES * 10..]);
	assert!(
		rms(heard_a) > 0.1 && rms(heard_b) > 0.1,
		"both calls carry audio"
	);
	assert!(power_at(heard_a, TONE_A) > 50.0 * power_at(heard_a, TONE_B));
	assert!(power_at(heard_b, TONE_B) > 50.0 * power_at(heard_b, TONE_A));
	let stats_a = rx_a.stats();
	// Half the offered packets belong to the other call.
	assert_eq!(
		stats_a.misbound.load(std::sync::atomic::Ordering::Relaxed),
		60
	);
}

#[test]
fn a_packet_relabelled_for_another_call_is_rejected() {
	let wire = Wire::default();
	let (mut tx, gate) = sender(CALL_A, 0xAAAA, &wire);
	gate.publish(open_gate(CALL_A, 1, 1), revision(1));
	let mut rx_b = receiver(CALL_B, 0xBBBB);
	for i in 0..20u64 {
		tx.emit(&tone(TONE_A, i), stamp(), i * 480);
	}
	let out = run(&mut rx_b, 20, |i, rx| {
		let p = wire.take().into_iter().next();
		let _ = i;
		if let Some(p) = p {
			// Sealed call id rewritten to B, but the RTP SSRC is still A's.
			let relabelled = Ingress::new(CALL_B.to_bytes(), 1, p.arrival_us, p.rtp()).unwrap();
			rx.ingest(SOURCE, &relabelled);
		}
	});
	assert!(rms(&out) < 1e-6, "nothing from call A may reach call B");
}

#[test]
fn wrong_generation_source_or_ssrc_is_dropped() {
	let wire = Wire::default();
	let (mut tx, gate) = sender(CALL_A, 0xAAAA, &wire);
	gate.publish(open_gate(CALL_A, 1, 1), revision(1));
	tx.emit(&tone(TONE_A, 0), stamp(), 0);
	let good = wire.take().remove(0);
	let mut rx = receiver(CALL_A, 0xAAAA);
	let bad_gen = Ingress::new(good.call, 2, 0, good.rtp()).unwrap();
	rx.ingest(SOURCE, &bad_gen);
	rx.ingest(SOURCE + 1, &good);
	let mut other_ssrc = receiver(CALL_A, 0x1234);
	other_ssrc.ingest(SOURCE, &good);
	let stats = rx.stats();
	assert_eq!(stats.misbound.load(std::sync::atomic::Ordering::Relaxed), 2);
	assert_eq!(stats.accepted.load(std::sync::atomic::Ordering::Relaxed), 0);
	assert_eq!(
		other_ssrc
			.stats()
			.misbound
			.load(std::sync::atomic::Ordering::Relaxed),
		1
	);
	rx.ingest(SOURCE, &good);
	assert_eq!(stats.accepted.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[test]
fn a_closed_call_puts_nothing_on_the_wire_while_the_other_keeps_sending() {
	let (wire_a, wire_b) = (Wire::default(), Wire::default());
	let (mut tx_a, gate_a) = sender(CALL_A, 0xAAAA, &wire_a);
	let (mut tx_b, gate_b) = sender(CALL_B, 0xBBBB, &wire_b);
	gate_b.publish(open_gate(CALL_B, 1, 1), revision(1));
	for i in 0..10u64 {
		// One shared microphone frame feeds both senders; only B has consent.
		let frame = tone(TONE_A, i);
		tx_a.emit(&frame, stamp(), i * 480);
		tx_b.emit(&frame, stamp(), i * 480);
	}
	assert_eq!(wire_a.len(), 0);
	assert_eq!(wire_b.len(), 10);
	let _ = gate_a;
}

#[test]
fn concealment_and_silence_never_invent_audio_for_an_idle_call() {
	let mut rx = receiver(CALL_A, 0xAAAA);
	let mut frame = [1.0; FRAME_SAMPLES];
	assert_eq!(rx.render(&mut frame), Rendered::Silence);
	assert!(frame.iter().all(|s| *s == 0.0));
}
