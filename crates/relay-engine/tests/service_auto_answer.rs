#![cfg(unix)]
//! Auto-answer on a real daemon with fake adapters: who is answered, when, and what the
//! answered call looks like.

#[macro_use]
mod service_support;

use std::time::{Duration, Instant};

use relay_api::pb::{AcquireControllerRequest, CallState, TransmitMode, state_event::Event};
use relay_api::{RelayClient, StateWatch, WatchItem, WatchOptions};
use relay_audio::tones::Sound;
use relay_core::{CallId, PeerEvent, PeerMessage, SessionId, SystemEvent};
use relay_engine::service::fakes::MediaCall;
use relay_engine::service::{PeerInfo, PeerTrust, ToneCommand};
use service_support::*;

const WINDOW: Duration = Duration::from_millis(2_300);

async fn attach(rig: &Rig) -> (RelayClient, StateWatch) {
	let mut cli = rig.full_client("ui").await;
	let mut watch = cli.watch(WatchOptions::default()).await.expect("watch");
	let _ = watch.next().await;
	(cli, watch)
}

/// Makes `PEER` a known contact with `trust` and, when `flag`, switches auto-answer on.
async fn contact(rig: &Rig, cli: &mut RelayClient, trust: PeerTrust, flag: bool) {
	let info = PeerInfo {
		trust,
		reachable: true,
		display_name: "Kitchen".to_owned(),
		..PeerInfo::bare(peer_id())
	};
	rig.daemon
		.control()
		.peers_changed(vec![info])
		.await
		.expect("peers");
	if flag {
		let done = cli
			.set_peer_auto_answer(&[PEER; 32], true)
			.await
			.expect("flag");
		assert_applied(&done);
	}
}

async fn invite(rig: &Rig, n: u128) -> (CallId, SessionId) {
	let control = rig.daemon.control();
	let (call, session) = (CallId::from_u128(n), SessionId::from_u128(n + 100));
	control
		.peer(PeerEvent::LinkUp(peer_id()))
		.await
		.expect("link");
	control
		.peer(PeerEvent::Invite {
			call,
			session,
			peer: peer_id(),
			allow_parallel_same_peer: false,
		})
		.await
		.expect("invite");
	(call, session)
}

fn accepted(rig: &Rig) -> bool {
	rig.fakes
		.peers
		.sent()
		.iter()
		.any(|(_, m)| matches!(m, PeerMessage::Accept { .. }))
}

async fn rings_unanswered(rig: &Rig, cli: &mut RelayClient, n: u128) {
	let (call, _) = invite(rig, n).await;
	let id = call.to_bytes().to_vec();
	wait_state(cli, &id, CallState::IncomingRinging).await;
	tokio::time::sleep(WINDOW).await;
	let still = call_in(cli, &id).await.expect("call");
	assert_eq!(still.state, CallState::IncomingRinging as i32);
	assert!(!still.auto_answered);
	assert!(!accepted(rig));
}

fn tones(rig: &Rig) -> Vec<ToneCommand> {
	rig.fakes
		.media
		.calls()
		.into_iter()
		.filter_map(|c| match c {
			MediaCall::Tone(t) => Some(t),
			_ => None,
		})
		.collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_verified_flagged_contact_is_answered_after_the_delay_with_an_event() {
	let rig = Rig::start().await;
	let (mut cli, mut watch) = attach(&rig).await;
	contact(&rig, &mut cli, PeerTrust::Verified, true).await;
	let (call, _) = invite(&rig, 1).await;
	let id = call.to_bytes().to_vec();
	wait_state(&mut cli, &id, CallState::IncomingRinging).await;
	let rang = Instant::now();
	assert!(!accepted(&rig), "it rings first");
	let answered = wait_state(&mut cli, &id, CallState::Negotiating).await;
	assert!(
		rang.elapsed() >= Duration::from_millis(1_300),
		"{:?}",
		rang.elapsed()
	);
	assert!(answered.auto_answered);
	assert!(accepted(&rig));

	let note = eventually!(
		match tokio::time::timeout(Duration::from_millis(50), watch.next()).await {
			Ok(Ok(Some(WatchItem::Event { event, .. }))) => match event.event {
				Some(Event::AutoAnswered(n)) => Some(n),
				_ => None,
			},
			_ => None,
		}
	);
	assert_eq!(note.call_id, id);
	assert_eq!(note.peer_id, vec![PEER; 32]);
	assert_eq!(note.peer_name, "Kitchen");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_answered_call_opens_in_the_default_mode_and_chimes_with_call_sounds_off() {
	let rig = Rig::start().await;
	let (mut cli, _watch) = attach(&rig).await;
	contact(&rig, &mut cli, PeerTrust::Verified, true).await;
	let set = cli
		.set_settings([
			("calls.default_transmit_mode", "push_to_talk"),
			("audio.call_sounds_enabled", "false"),
		])
		.await
		.expect("settings");
	assert!(!set.results.is_empty());
	let (call, session) = invite(&rig, 2).await;
	let id = call.to_bytes().to_vec();
	wait_state(&mut cli, &id, CallState::Negotiating).await;
	let control = rig.daemon.control();
	control
		.system(SystemEvent::MediaPrepared { call, session })
		.await
		.expect("prepared");
	control
		.peer(PeerEvent::PathValidated { call, session })
		.await
		.expect("path");
	let live = wait_state(&mut cli, &id, CallState::Connected).await;
	let mode = live.transmit.expect("transmit");
	assert_eq!(
		mode.mode,
		TransmitMode::PushToTalk as i32,
		"the mic stays closed"
	);
	assert!(!mode.effective_transmitting);
	eventually!(
		tones(&rig)
			.iter()
			.any(|t| matches!(
				t,
				ToneCommand::Start {
					sound: Sound::Connected,
					..
				}
			))
			.then_some(())
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_answered_call_follows_a_changed_default_call_mode() {
	let rig = Rig::start().await;
	let (mut cli, _watch) = attach(&rig).await;
	contact(&rig, &mut cli, PeerTrust::Verified, true).await;
	cli.set_settings([("calls.default_transmit_mode", "unmuted")])
		.await
		.expect("settings");
	let (call, session) = invite(&rig, 3).await;
	let id = call.to_bytes().to_vec();
	wait_state(&mut cli, &id, CallState::Negotiating).await;
	let control = rig.daemon.control();
	control
		.system(SystemEvent::MediaPrepared { call, session })
		.await
		.expect("prepared");
	control
		.peer(PeerEvent::PathValidated { call, session })
		.await
		.expect("path");
	let live = wait_state(&mut cli, &id, CallState::Connected).await;
	assert_eq!(live.transmit.unwrap().mode, TransmitMode::Unmuted as i32);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_muted_default_keeps_the_answered_call_muted() {
	let rig = Rig::start().await;
	let (mut cli, _watch) = attach(&rig).await;
	contact(&rig, &mut cli, PeerTrust::Verified, true).await;
	cli.set_settings([("calls.default_transmit_mode", "muted")])
		.await
		.expect("settings");
	let (call, _) = invite(&rig, 4).await;
	let id = call.to_bytes().to_vec();
	let live = wait_state(&mut cli, &id, CallState::Negotiating).await;
	assert_eq!(live.transmit.unwrap().mode, TransmitMode::Muted as i32);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unverified_or_unflagged_contact_only_rings() {
	let rig = Rig::start().await;
	let (mut cli, _watch) = attach(&rig).await;
	contact(&rig, &mut cli, PeerTrust::TofuUnverified, true).await;
	rings_unanswered(&rig, &mut cli, 5).await;

	let rig = Rig::start().await;
	let (mut cli, _watch) = attach(&rig).await;
	contact(&rig, &mut cli, PeerTrust::Verified, false).await;
	rings_unanswered(&rig, &mut cli, 6).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn key_changed_and_blocked_labels_are_never_answered() {
	for trust in [
		PeerTrust::KeyChanged,
		PeerTrust::Blocked,
		PeerTrust::Unknown,
	] {
		let rig = Rig::start().await;
		let (mut cli, _watch) = attach(&rig).await;
		contact(&rig, &mut cli, trust, true).await;
		rings_unanswered(&rig, &mut cli, 7).await;
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_kill_switch_ignores_every_flag_and_applies_live() {
	let rig = Rig::start().await;
	let (mut cli, _watch) = attach(&rig).await;
	contact(&rig, &mut cli, PeerTrust::Verified, true).await;
	let snapshot = cli.snapshot().await.unwrap();
	assert!(snapshot.settings.unwrap().auto_answer_enabled);
	let (call, _) = invite(&rig, 8).await;
	let id = call.to_bytes().to_vec();
	wait_state(&mut cli, &id, CallState::IncomingRinging).await;
	// Switched off inside the window: the pending answer is dropped.
	cli.set_settings([("calls.auto_answer_enabled", "false")])
		.await
		.expect("settings");
	tokio::time::sleep(WINDOW).await;
	let still = call_in(&mut cli, &id).await.unwrap();
	assert_eq!(still.state, CallState::IncomingRinging as i32);
	assert!(!accepted(&rig));
	assert!(
		!cli.snapshot()
			.await
			.unwrap()
			.settings
			.unwrap()
			.auto_answer_enabled
	);
	// The flag itself survives the switch.
	let peer = cli.snapshot().await.unwrap().peers.remove(0);
	assert!(peer.auto_answer);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn declining_inside_the_window_wins() {
	let rig = Rig::start().await;
	let (mut cli, _watch) = attach(&rig).await;
	contact(&rig, &mut cli, PeerTrust::Verified, true).await;
	let (call, _) = invite(&rig, 9).await;
	let id = call.to_bytes().to_vec();
	wait_state(&mut cli, &id, CallState::IncomingRinging).await;
	let context = ctx(&cli, 40);
	let lease = cli
		.raw()
		.acquire_controller(AcquireControllerRequest {
			context: Some(context),
			call_id: id.clone(),
			request_explicit_takeover: false,
		})
		.await
		.expect("lease")
		.into_inner();
	let decline = mutation(&cli, 41, &id, lease.generation);
	assert_applied(
		&cli.raw()
			.decline_call(decline)
			.await
			.expect("decline")
			.into_inner(),
	);
	tokio::time::sleep(WINDOW).await;
	let ended = call_in(&mut cli, &id).await.expect("call");
	assert_eq!(ended.state, CallState::Declined as i32);
	assert!(!accepted(&rig));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_busy_device_does_not_answer_a_second_call() {
	let rig = Rig::start().await;
	let (mut cli, _watch) = attach(&rig).await;
	contact(&rig, &mut cli, PeerTrust::Verified, true).await;
	// A live outgoing call to someone else.
	let request = relay_api::pb::StartCallRequest {
		context: Some(ctx(&cli, 50)),
		target: Some(relay_api::pb::start_call_request::Target::PeerId(vec![
			9;
			32
		])),
		allow_parallel_same_peer: false,
	};
	let started = cli
		.raw()
		.start_call(request)
		.await
		.expect("start")
		.into_inner();
	assert_applied(&started);
	rings_unanswered(&rig, &mut cli, 10).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_flag_and_the_global_switch_show_in_the_snapshot() {
	let rig = Rig::start().await;
	let (mut cli, _watch) = attach(&rig).await;
	contact(&rig, &mut cli, PeerTrust::Verified, true).await;
	let snapshot = cli.snapshot().await.unwrap();
	assert!(snapshot.peers[0].auto_answer);
	let off = cli.set_peer_auto_answer(&[PEER; 32], false).await.unwrap();
	assert_applied(&off);
	assert!(!cli.snapshot().await.unwrap().peers[0].auto_answer);
	let bad = cli.set_peer_auto_answer(&[1, 2, 3], true).await;
	assert!(bad.is_err(), "a malformed id is refused");
}
