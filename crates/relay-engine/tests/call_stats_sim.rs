//! The numbers behind `WatchCallStats`, proven on a simulated lossy and jittery link and a
//! fake clock: RFC 3550 jitter, loss over 5 s and in total, round trip min and max over 10 s.

use std::sync::atomic::Ordering::Relaxed;

use relay_core::CallId;
use relay_engine::media::{CallReceiver, Ingress, RemoteStream};
use relay_engine::service::call_stats::{CallRaw, StatsWindow, mouth_to_ear_ms, to_pb};
use relay_protocol::jitter::JitterConfig;
use relay_protocol::rtp::{RtpHeader, RtpProfile};

const SSRC: u32 = 0xfeed;

/// Small deterministic generator, so the run is repeatable.
struct Lcg(u64);

impl Lcg {
	fn next(&mut self) -> u64 {
		self.0 = self
			.0
			.wrapping_mul(6_364_136_223_846_793_005)
			.wrapping_add(1_442_695_040_888_963_407);
		self.0 >> 33
	}
}

fn rtp(sequence: u16, ts_step: u32) -> Vec<u8> {
	let header = RtpHeader {
		marker: false,
		payload_type: RtpProfile::V1.payload_type,
		sequence,
		timestamp: u32::from(sequence).wrapping_mul(ts_step),
		ssrc: SSRC,
	};
	let mut out = vec![0u8; 256];
	let n = RtpProfile::V1
		.write(&header, &[1, 2, 3, 4], &mut out)
		.unwrap();
	out.truncate(n);
	out
}

/// Packets every 60 ms (so a delay never reorders them), 5 % lost, each delayed uniformly in
/// `0..60 ms`: the mean change of delay between neighbours, which is what RFC 3550 jitter
/// converges to, is 60/3 = 20 ms.
#[test]
fn a_lossy_jittery_link_reports_its_jitter_and_loss() {
	const SPACING_US: u64 = 60_000;
	const TS_STEP: u32 = 2_880;
	let call = CallId::from_u128(9);
	let mut receiver = CallReceiver::new(
		RemoteStream {
			call,
			ssrc: SSRC,
			generation: 1,
			source: 1,
		},
		JitterConfig::BALANCED,
	)
	.unwrap();
	let stats = receiver.stats();
	let mut rng = Lcg(7);
	let mut frame = [0.0f32; relay_audio::FRAME_SAMPLES];
	for i in 0u32..2000 {
		if i % 20 == 7 {
			continue; // lost
		}
		let delay = rng.next() % 60_000;
		let arrival_us = u64::from(i) * SPACING_US + delay;
		let packet = Ingress::new(call.to_bytes(), 1, arrival_us, &rtp(i as u16, TS_STEP)).unwrap();
		receiver.ingest(1, &packet);
		receiver.render(&mut frame);
	}
	let jitter_ms = stats.jitter_us.load(Relaxed) as f64 / 1000.0;
	assert!((15.0..=25.0).contains(&jitter_ms), "jitter {jitter_ms} ms");
	let expected = stats.expected.load(Relaxed);
	let received = stats.accepted.load(Relaxed) + stats.refused.load(Relaxed);
	let loss = (expected - received) as f64 * 100.0 / expected as f64;
	assert!((4.5..=5.5).contains(&loss), "loss {loss} %");
}

#[test]
fn sequence_wrap_does_not_count_as_loss() {
	let call = CallId::from_u128(9);
	let mut receiver = CallReceiver::new(
		RemoteStream {
			call,
			ssrc: SSRC,
			generation: 1,
			source: 1,
		},
		JitterConfig::BALANCED,
	)
	.unwrap();
	let stats = receiver.stats();
	let start: u16 = 65_500;
	for i in 0u32..100 {
		let sequence = start.wrapping_add(i as u16);
		let packet = Ingress::new(
			call.to_bytes(),
			1,
			u64::from(i) * 10_000,
			&rtp(sequence, 480),
		)
		.unwrap();
		receiver.ingest(1, &packet);
	}
	assert_eq!(stats.expected.load(Relaxed), 100);
}

fn raw(expected: u64, received: u64, rtt: f64) -> CallRaw {
	CallRaw {
		packets_expected: expected,
		packets_received: received,
		rtt_ms: Some(rtt),
		..CallRaw::default()
	}
}

/// One reading per second on a fake clock: 100 packets a second, 5 % lost, a 40 ms round
/// trip that wanders by +-6 ms.
#[test]
fn windows_report_loss_and_round_trip_extremes() {
	let mut window = StatsWindow::default();
	let (mut expected, mut received) = (0u64, 0u64);
	let wander = [
		0.0, 4.0, -6.0, 2.0, 6.0, -3.0, 1.0, -5.0, 3.0, 0.0, 5.0, -2.0,
	];
	let mut last = None;
	for second in 0u64..12 {
		expected += 100;
		received += 95;
		let rtt = 40.0 + wander[second as usize];
		last = Some(window.push(second * 1000, &raw(expected, received, rtt)));
	}
	let last = last.unwrap();
	assert!((last.loss_5s - 5.0).abs() < 0.5, "{}", last.loss_5s);
	assert!((last.loss_total - 5.0).abs() < 0.01);
	// The last 10 s hold seconds 2..=11.
	assert_eq!(last.rtt_min_ms, Some(34.0));
	assert_eq!(last.rtt_max_ms, Some(46.0));
}

#[test]
fn a_burst_of_loss_shows_in_the_short_window_before_the_total() {
	let mut window = StatsWindow::default();
	let (mut expected, mut received) = (0u64, 0u64);
	for second in 0u64..20 {
		expected += 100;
		received += 100;
		window.push(second * 1000, &raw(expected, received, 40.0));
	}
	let mut last = None;
	for second in 20u64..25 {
		expected += 100;
		received += 50;
		last = Some(window.push(second * 1000, &raw(expected, received, 40.0)));
	}
	let last = last.unwrap();
	assert!(last.loss_5s > 40.0, "{}", last.loss_5s);
	assert!(last.loss_total < 25.0, "{}", last.loss_total);
}

#[test]
fn mouth_to_ear_adds_the_documented_parts() {
	let reading = CallRaw {
		rtt_ms: Some(40.0),
		jitter_buffer_depth_ms: 30.0,
		playout_delay_ms: 25.0,
		capture_frames: 480,
		capture_rate: 48_000,
		..CallRaw::default()
	};
	// 10 (capture) + 12.5 (encode) + 20 (rtt/2) + 30 + 5 (decode) + 25.
	assert_eq!(mouth_to_ear_ms(&reading), Some(102.5));
	let pb = to_pb(CallId::from_u128(1), &reading, &Default::default());
	assert_eq!(pb.mouth_to_ear_ms, Some(102.5));
	assert_eq!(mouth_to_ear_ms(&CallRaw::default()), None);
}

#[test]
fn sending_and_receiving_mean_a_packet_in_the_last_500_ms() {
	let fresh = CallRaw {
		since_sent_ms: Some(120),
		since_received_ms: Some(499),
		..CallRaw::default()
	};
	let pb = to_pb(CallId::from_u128(1), &fresh, &Default::default());
	assert!(pb.sending && pb.receiving);
	let stale = CallRaw {
		since_sent_ms: Some(500),
		since_received_ms: None,
		..CallRaw::default()
	};
	let pb = to_pb(CallId::from_u128(1), &stale, &Default::default());
	assert!(!pb.sending && !pb.receiving);
}
