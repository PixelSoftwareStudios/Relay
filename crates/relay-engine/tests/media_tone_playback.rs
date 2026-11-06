//! Tones reach the local speaker only, never a peer, and need no call.

mod support_e2e;
mod support_media;

use std::time::Duration;

use relay_audio::tones::Sound;
use relay_core::*;
use relay_engine::service::{MediaControl, ToneCommand};
use support_e2e::{Node, Pair, active_call, run_for};
use support_media::{power_at, rms};

const TONE_B: f32 = 1_700.0;

fn ring() -> ToneCommand {
	ToneCommand::Start {
		sound: Sound::Ring,
		looping: true,
		volume: 0.7,
	}
}

fn peak(samples: &[f32]) -> f32 {
	samples.iter().fold(0.0, |m, s| m.max(s.abs()))
}

fn tail(node: &Node, ms: usize) -> Vec<f32> {
	let heard = node.sim.heard();
	heard[heard.len().saturating_sub(48 * ms)..].to_vec()
}

async fn idle(node: &mut Node, time: Duration) {
	let end = std::time::Instant::now() + time;
	while std::time::Instant::now() < end {
		node.poll();
		tokio::time::sleep(Duration::from_millis(5)).await;
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tone_plays_with_no_call_and_only_while_it_rings() {
	let mut node = Node::new(1, 440.0).await;
	idle(&mut node, Duration::from_millis(300)).await;
	assert!(
		node.sim.heard().iter().all(|s| s.abs() < 1e-4),
		"silent first"
	);

	MediaControl::tone(&node.media, ring());
	idle(&mut node, Duration::from_millis(900)).await;
	let rang = tail(&node, 500);
	assert!(rms(&rang) > 0.02, "the ring reaches the speaker");

	MediaControl::tone(&node.media, ToneCommand::Stop);
	idle(&mut node, Duration::from_millis(500)).await;
	assert!(rms(&tail(&node, 200)) < 1e-4, "silent after the stop");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn deafen_mutes_a_playing_tone() {
	let mut node = Node::new(1, 440.0).await;
	MediaControl::tone(&node.media, ring());
	idle(&mut node, Duration::from_millis(600)).await;
	MediaControl::tone(&node.media, ToneCommand::Muted(true));
	idle(&mut node, Duration::from_millis(300)).await;
	assert!(rms(&tail(&node, 150)) < 1e-4);
	MediaControl::tone(&node.media, ToneCommand::Muted(false));
	idle(&mut node, Duration::from_millis(2_200)).await;
	assert!(peak(&tail(&node, 2_100)) > 0.02);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_local_ring_sends_nothing_and_the_peer_hears_only_the_microphone() {
	let Pair {
		mut a,
		mut b,
		call,
		lease_b,
		..
	} = active_call().await;
	b.command(Command::SetTransmitMode {
		call,
		lease: lease_b,
		route: TransmitRoute::unmuted(),
	})
	.unwrap();
	run_for(&mut a, &mut b, Duration::from_millis(800)).await;
	let at_a_before = rms(&tail(&a, 500));
	let before = b.media.report(call).unwrap().sent;
	assert!(before > 0, "B transmits its microphone");

	MediaControl::tone(&b.media, ring());
	run_for(&mut a, &mut b, Duration::from_millis(1_200)).await;
	assert!(peak(&tail(&b, 2_000)) > 0.02, "B hears its own ring");

	// A hears B's microphone tone and none of B's ring.
	let at_a = tail(&a, 500);
	assert!(rms(&at_a) > 0.05, "A still hears B's voice");
	assert!(
		(rms(&at_a) - at_a_before).abs() < 0.2 * at_a_before,
		"A's signal did not change"
	);
	assert!(power_at(&at_a, TONE_B) > 50.0 * power_at(&at_a, 660.0));
	assert!(power_at(&at_a, TONE_B) > 50.0 * power_at(&at_a, 880.0));

	// Packet count keeps its rate (about 100 per second): the ring adds none.
	let after = b.media.report(call).unwrap().sent;
	assert!(
		after - before < 140,
		"no extra packets: {before} -> {after}"
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ring_with_consent_withheld_sends_zero_packets() {
	let Pair {
		mut a, mut b, call, ..
	} = active_call().await;
	MediaControl::tone(&b.media, ring());
	run_for(&mut a, &mut b, Duration::from_millis(2_200)).await;
	assert_eq!(b.media.report(call).unwrap().sent, 0);
	assert_eq!(a.media.report(call).unwrap().sent, 0);
	assert!(rms(&tail(&a, 500)) < 1e-4, "A hears silence");
	assert!(peak(&tail(&b, 2_000)) > 0.02, "B hears its ring");
}

/// The ring must not stutter while the output device wakes up: the first 500 ms
/// after the output opens play with no buffer underrun, and the tone is audible.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_ring_on_a_slow_to_open_output_has_no_underruns_at_the_start() {
	let mut node = Node::with_slow_output(1, 440.0, Duration::from_millis(250)).await;
	MediaControl::tone(&node.media, ring());
	let started = std::time::Instant::now();
	// Wait for the first audible sample, then watch half a second.
	while started.elapsed() < Duration::from_secs(3) && peak(&node.sim.heard()) < 0.01 {
		node.poll();
		tokio::time::sleep(Duration::from_millis(2)).await;
	}
	let first = started.elapsed();
	println!("first ring sound {first:?} after the request, output open delay 250 ms");
	assert!(
		first < Duration::from_millis(250 + 150),
		"the ring starts soon after the output opens: {first:?}"
	);
	idle(&mut node, Duration::from_millis(500)).await;
	let heard = node.sim.heard();
	assert!(peak(&heard) > 0.02, "the ring is audible");
	let underruns = node.media.telemetry().render_underruns;
	assert_eq!(
		underruns, 0,
		"no underrun in the first 500 ms (first sound after {first:?})"
	);
}
