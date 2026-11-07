//! A custom ringtone is played like the built-in ring: on the local speaker only, never sent.

mod support_e2e;
mod support_media;

use std::sync::Arc;
use std::time::Duration;

use relay_audio::tones::Sound;
use relay_engine::service::{MediaControl, ToneCommand};
use support_e2e::{Pair, active_call, run_for};
use support_media::{power_at, rms};

fn custom_ring() -> Arc<[f32]> {
	let mut ring: Vec<f32> = (0..48_000)
		.map(|i| 0.2 * (std::f32::consts::TAU * 1_100.0 * i as f32 / 48_000.0).sin())
		.collect();
	ring.resize(72_000, 0.0);
	ring.into()
}

fn tail(node: &support_e2e::Node, ms: usize) -> Vec<f32> {
	let heard = node.sim.heard();
	heard[heard.len().saturating_sub(48 * ms)..].to_vec()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_custom_ring_is_heard_locally_and_sends_zero_packets() {
	let Pair {
		mut a, mut b, call, ..
	} = active_call().await;
	b.media.set_ringtone(Some(custom_ring()));
	MediaControl::tone(
		&b.media,
		ToneCommand::Start {
			sound: Sound::Ring,
			looping: true,
			volume: 1.0,
		},
	);
	run_for(&mut a, &mut b, Duration::from_millis(1_500)).await;

	let heard = tail(&b, 1_000);
	assert!(rms(&heard) > 0.02, "B hears the custom ring");
	assert!(
		power_at(&heard, 1_100.0) > 20.0 * power_at(&heard, 660.0),
		"it is the custom sound, not the chime"
	);
	assert_eq!(b.media.report(call).unwrap().sent, 0, "nothing is sent");
	assert_eq!(a.media.report(call).unwrap().sent, 0);
	assert!(rms(&tail(&a, 500)) < 1e-4, "A hears silence");

	// Resetting brings the chime back on the next cycle.
	b.media.set_ringtone(None);
	MediaControl::tone(&b.media, ToneCommand::Stop);
	run_for(&mut a, &mut b, Duration::from_millis(400)).await;
	assert!(rms(&tail(&b, 150)) < 1e-4);
}
