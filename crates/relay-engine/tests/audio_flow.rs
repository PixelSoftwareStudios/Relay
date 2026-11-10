#![cfg(unix)]
//! Audio really flows between two whole engines (real loopback sockets, real Noise links,
//! real Opus, RTP, jitter buffer, mixer; only the audio hardware is a deterministic fake that
//! captures a synthetic tone and records what the speaker plays), driven only through the
//! local API: the per-call transmit mode is the consent, a default mode applies when the call
//! connects, push-to-talk opens and closes the gate, and a stale mode never leaks audio.
//!
//! The devices met here the way a first call does: a manual dial, the unknown device pinned
//! (trust on first use), then a call. The discovery probe dial ends in the same untrusted
//! link and the same pin (`peers_discovered.rs` covers the probe itself).

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;

use std::time::Duration;

use relay_api::pb::{CallState, InputAction, InputControlMessage, TransmitMode};
use service_support::*;
use support_channels::*;
use support_media::{power_at, rms};
use tokio::sync::mpsc;
use tokio_stream::wrappers::ReceiverStream;

const TONE_A: f32 = 440.0;
const TONE_B: f32 = 1_700.0;

/// Waits until the speaker of `side` plays `tone` clearly and the other tone only faintly.
async fn hears(side: &Side, tone: f32, other: f32) {
	eventually!(heard_clearly(side, tone, other).then_some(()));
}

fn heard_clearly(side: &Side, tone: f32, other: f32) -> bool {
	let heard = tail(side, 300);
	rms(&heard) > 0.02 && power_at(&heard, tone) > 10.0 * power_at(&heard, other)
}

async fn silent(side: &Side) {
	eventually!((rms(&tail(side, 200)) < 1e-3).then_some(()));
}

/// Two engines, paired by trust on first use, with A's call to B connected.
async fn connected_pair() -> (Side, Side, Vec<u8>) {
	let mut a = side(TONE_A).await;
	let mut b = side(TONE_B).await;
	pair(&mut a, &mut b, 1).await;
	let call = ring(&mut a, &b, 30).await;
	answer(&mut b.cli, &call, 40).await;
	wait_state(&mut a.cli, &call, CallState::Connected).await;
	wait_state(&mut b.cli, &call, CallState::Connected).await;
	(a, b, call)
}

async fn mode_of(side: &mut Side, call: &[u8]) -> TransmitMode {
	let transmit = call_in(&mut side.cli, call)
		.await
		.expect("call")
		.transmit
		.expect("transmit");
	TransmitMode::try_from(transmit.mode).expect("mode")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn unmuted_is_one_request_and_audio_flows_both_ways() {
	let (mut a, mut b, call) = connected_pair().await;
	// The default mode is push-to-talk: connected, consenting, silent until a key is held.
	assert_eq!(mode_of(&mut a, &call).await, TransmitMode::PushToTalk);
	assert_eq!(mode_of(&mut b, &call).await, TransmitMode::PushToTalk);
	settle(600).await;
	silent(&a).await;
	silent(&b).await;

	// One request each: no separate consent step exists.
	route(&mut a.cli, 50, &call, TransmitMode::Unmuted).await;
	hears(&b, TONE_A, TONE_B).await;
	silent(&a).await;
	route(&mut b.cli, 51, &call, TransmitMode::Unmuted).await;
	hears(&a, TONE_B, TONE_A).await;
	hears(&b, TONE_A, TONE_B).await;

	// Muted withdraws the consent: the speaker of the other side goes quiet.
	route(&mut a.cli, 52, &call, TransmitMode::Muted).await;
	silent(&b).await;
	hears(&a, TONE_B, TONE_A).await;

	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

fn press(
	call: &[u8],
	cli: &relay_api::RelayClient,
	generation: u64,
	seq: u64,
	action: InputAction,
) -> InputControlMessage {
	InputControlMessage {
		client: cli.context(),
		source_id: vec![1; 16],
		input_sequence: seq,
		call_id: call.to_vec(),
		controller_generation: generation,
		transmit_route_generation: 0,
		action: action as i32,
		requested_lease_ms: 2_000,
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn push_to_talk_sends_while_held_and_stops_after_release() {
	let (mut a, b, call) = connected_pair().await;
	let generation = controller_of(&call_in(&mut a.cli, &call).await.expect("call")).generation;

	let (tx, rx) = mpsc::channel(8);
	let mut acks = a
		.cli
		.raw()
		.input_control(ReceiverStream::new(rx))
		.await
		.expect("input stream")
		.into_inner();
	settle(300).await;
	silent(&b).await;

	tx.send(press(&call, &a.cli, generation, 1, InputAction::Press))
		.await
		.expect("press");
	let ack = acks.message().await.expect("ack").expect("an ack");
	assert!(ack.accepted, "{ack:?}");

	// The key stays down while the far side is awaited: a held key is renewed before its
	// lease runs out (as a real client does), so a slow machine cannot expire the lease
	// and mute the very audio being waited for.
	let mut seq = 1;
	let mut renewed = tokio::time::Instant::now();
	let deadline = renewed + Duration::from_secs(30);
	while !heard_clearly(&b, TONE_A, TONE_B) {
		assert!(
			tokio::time::Instant::now() < deadline,
			"the far side never heard the held key"
		);
		if renewed.elapsed() >= Duration::from_millis(500) {
			seq += 1;
			tx.send(press(&call, &a.cli, generation, seq, InputAction::Renew))
				.await
				.expect("renew");
			let ack = acks.message().await.expect("ack").expect("an ack");
			assert!(ack.accepted, "{ack:?}");
			renewed = tokio::time::Instant::now();
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}

	tx.send(press(
		&call,
		&a.cli,
		generation,
		seq + 1,
		InputAction::Release,
	))
	.await
	.expect("release");
	assert!(acks.message().await.expect("ack").is_some());
	// The release delay holds the gate for a moment, then the far side falls silent.
	eventually!((rms(&tail(&b, 200)) < 1e-3).then_some(()));

	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_default_mode_is_applied_when_a_call_connects_and_follows_the_setting() {
	let mut a = side(TONE_A).await;
	let mut b = side(TONE_B).await;
	pair(&mut a, &mut b, 1).await;
	for side in [&mut a, &mut b] {
		let answer = side
			.cli
			.set_settings([("calls.default_transmit_mode", "unmuted")])
			.await
			.expect("set");
		assert_eq!(
			answer.results[0].outcome,
			relay_api::pb::SettingOutcome::AppliedLive as i32
		);
	}
	let call = ring(&mut a, &b, 30).await;
	answer(&mut b.cli, &call, 40).await;
	wait_state(&mut a.cli, &call, CallState::Connected).await;
	wait_state(&mut b.cli, &call, CallState::Connected).await;

	// Nobody asked for a mode: the default is the mode, and audio flows both ways at once.
	assert_eq!(mode_of(&mut a, &call).await, TransmitMode::Unmuted);
	hears(&a, TONE_B, TONE_A).await;
	hears(&b, TONE_A, TONE_B).await;

	// A default of muted leaves a new call silent, and an explicit choice still wins.
	for side in [&mut a, &mut b] {
		side.cli
			.set_settings([("calls.default_transmit_mode", "muted")])
			.await
			.expect("set");
	}
	route(&mut a.cli, 60, &call, TransmitMode::Muted).await;
	silent(&b).await;
	tokio::time::sleep(Duration::from_millis(200)).await;

	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_fresh_profile_records_history_and_starts_calls_in_push_to_talk() {
	let a = side(TONE_A).await;
	let mut cli = a.cli;
	let dump = cli.get_settings().await.expect("settings");
	let value = |key: &str| dump.get(key).map(String::as_str);
	assert_eq!(
		value("history.enabled"),
		Some("true"),
		"history is on by default"
	);
	assert_eq!(value("calls.default_transmit_mode"), Some("push_to_talk"));
	assert_eq!(
		value("ptt.enabled"),
		None,
		"push-to-talk has no on/off setting"
	);
	let settings = cli
		.snapshot()
		.await
		.expect("snapshot")
		.settings
		.expect("settings");
	assert!(settings.history_enabled);
	assert_eq!(
		settings.default_transmit_mode,
		TransmitMode::PushToTalk as i32
	);
	assert!(!cli.daemon_revision().build.is_empty() && cli.daemon_revision().api > 0);
	a.daemon.shutdown().await.expect("a stops");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn peers_carry_the_address_port_last_seen_and_trust_for_the_contact_lists() {
	let (mut a, b, call) = connected_pair().await;
	let peers = a.cli.snapshot().await.expect("snapshot").peers;
	let peer = peers
		.iter()
		.find(|p| p.peer_id == id_of(&b))
		.expect("B is listed");
	assert!(peer.trust != relay_api::pb::TrustState::Unknown as i32);
	assert!(peer.reachable);
	assert!(
		peer.local_addresses.iter().any(|a| a.contains(':')),
		"{:?}",
		peer.local_addresses
	);
	assert!(peer.port > 0, "the listener port is reported");
	assert!(peer.last_seen_unix_ms > 0, "last seen is stamped");

	// The call carries the instant it connected on both ends, from the engine's wall clock.
	let (at_a, at_b) = (
		call_in(&mut a.cli, &call)
			.await
			.expect("call")
			.connected_at_unix_ms,
		call_in(&mut { b.cli }, &call)
			.await
			.expect("call")
			.connected_at_unix_ms,
	);
	assert!(at_a > 0 && at_b > 0);
	assert!(at_a.abs_diff(at_b) < 5_000, "{at_a} {at_b}");
	a.daemon.shutdown().await.expect("a stops");
	b.daemon.shutdown().await.expect("b stops");
}
