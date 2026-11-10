//! A route change disconnects the speaker stream; the media layer reopens it
//! and the user hears no gap worth a warning. Only an output that stays gone
//! is reported, once, and withdrawn when it returns.

mod support_e2e;
mod support_media;

use std::time::{Duration, Instant};

use relay_audio::tones::Sound;
use relay_engine::service::{MediaControl, ToneCommand};
use support_e2e::Node;
use support_media::rms;

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
	let end = Instant::now() + time;
	while Instant::now() < end {
		node.poll();
		tokio::time::sleep(Duration::from_millis(5)).await;
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_disconnect_while_audio_plays_reopens_and_audio_continues_with_no_warning() {
	let mut node = Node::new(1, 440.0).await;
	MediaControl::tone(&node.media, ring());
	idle(&mut node, Duration::from_millis(900)).await;
	assert!(rms(&tail(&node, 400)) > 0.02, "the ring is audible first");

	// The reopened stream is a new device with a new recording. The ring is a decaying bell that
	// is audible for about its first 1.7 s of a four second period and pauses while no stream
	// renders it, so the audio is looked for right after the reopen, however long that takes.
	let before = node.sim.heard().len();
	node.sim.disconnect_speaker();
	let deadline = Instant::now() + Duration::from_secs(10);
	while node.sim.heard().len() >= before && Instant::now() < deadline {
		idle(&mut node, Duration::from_millis(10)).await;
	}
	assert!(
		node.sim.heard().len() < before,
		"the speaker is reopened (a new stream plays)"
	);
	while peak(&node.sim.heard()) <= 0.02 && Instant::now() < deadline {
		idle(&mut node, Duration::from_millis(10)).await;
	}
	assert!(
		peak(&node.sim.heard()) > 0.02,
		"the ring is audible again on the reopened stream"
	);
	idle(&mut node, Duration::from_millis(300)).await;
	assert_eq!(node.output_lost, 0, "a route change is not a warning");
	assert_eq!(node.output_restored, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_output_that_stays_gone_warns_once_and_resumes_when_it_returns() {
	let mut node = Node::new(1, 440.0).await;
	MediaControl::tone(&node.media, ring());
	idle(&mut node, Duration::from_millis(900)).await;

	node.sim.remove_speaker();
	node.sim.disconnect_speaker();
	idle(&mut node, Duration::from_millis(6_000)).await;
	assert_eq!(node.output_lost, 1, "exactly one warning");
	assert_eq!(node.output_restored, 0);

	node.sim.add_speaker();
	idle(&mut node, Duration::from_millis(4_500)).await;
	assert_eq!(node.output_lost, 1);
	assert_eq!(node.output_restored, 1, "the warning is withdrawn");
	assert!(
		peak(&tail(&node, 3_000)) > 0.02,
		"audio resumes without anyone choosing an output"
	);
}
