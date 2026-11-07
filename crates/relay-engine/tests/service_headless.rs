#![cfg(unix)]
//! A daemon nobody watches while `background.available` is on: it is home itself, so it rings,
//! auto-answers and keeps running; with the setting off the old client-held rules apply. A shell
//! that alerts on its own can mark its client quiet.

#[macro_use]
mod service_support;

use std::time::Duration;

use relay_api::pb::{AcquireControllerRequest, CallState};
use relay_api::{RelayClient, WatchOptions};
use relay_audio::tones::Sound;
use relay_core::{CallId, PeerEvent, PeerMessage, SessionId};
use relay_engine::service::fakes::MediaCall;
use relay_engine::service::{PeerInfo, PeerTrust, ToneCommand};
use service_support::*;

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

fn rang(rig: &Rig) -> bool {
	tones(rig).iter().any(|t| {
		matches!(
			t,
			ToneCommand::Start {
				sound: Sound::Ring,
				..
			}
		)
	})
}

fn sent(rig: &Rig, pick: fn(&PeerMessage) -> bool) -> bool {
	rig.fakes.peers.sent().iter().any(|(_, m)| pick(m))
}

fn accepted(m: &PeerMessage) -> bool {
	matches!(m, PeerMessage::Accept { .. })
}

fn declined(m: &PeerMessage) -> bool {
	matches!(m, PeerMessage::Decline { .. })
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

async fn settle() {
	tokio::time::sleep(Duration::from_millis(400)).await;
}

async fn setting(cli: &mut RelayClient, key: &str, value: &str) {
	cli.set_settings([(key, value)]).await.expect("settings");
}

/// Makes the peer a verified contact with auto-answer on, using a client that never watches.
async fn flagged_contact(rig: &Rig, cli: &mut RelayClient) {
	let info = PeerInfo {
		trust: PeerTrust::Verified,
		reachable: true,
		display_name: "Kitchen".to_owned(),
		..PeerInfo::bare(peer_id())
	};
	rig.daemon
		.control()
		.peers_changed(vec![info])
		.await
		.expect("peers");
	let done = cli
		.set_peer_auto_answer(&[PEER; 32], true)
		.await
		.expect("flag");
	assert_applied(&done);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_background_on_an_unwatched_engine_rings_and_keeps_the_call() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("status").await;
	let (call, _) = invite(&rig, 1).await;
	let id = call.to_bytes().to_vec();
	wait_state(&mut cli, &id, CallState::IncomingRinging).await;
	eventually!(rang(&rig).then_some(()));
	assert!(!sent(&rig, declined), "nobody declined it");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_background_off_an_unwatched_engine_declines_and_stays_silent() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("status").await;
	setting(&mut cli, "background.available", "false").await;
	invite(&rig, 2).await;
	eventually!(sent(&rig, declined).then_some(()));
	settle().await;
	assert!(!rang(&rig));
	assert!(cli.snapshot().await.expect("snapshot").calls.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn turning_background_off_withdraws_presence_and_declines_the_unanswered_call() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("status").await;
	let (call, _) = invite(&rig, 3).await;
	let id = call.to_bytes().to_vec();
	wait_state(&mut cli, &id, CallState::IncomingRinging).await;
	setting(&mut cli, "background.available", "false").await;
	wait_state(&mut cli, &id, CallState::Declined).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_verified_flagged_contact_is_answered_with_no_client_and_a_client_can_take_over() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("status").await;
	flagged_contact(&rig, &mut cli).await;
	let (call, _) = invite(&rig, 4).await;
	let id = call.to_bytes().to_vec();
	wait_state(&mut cli, &id, CallState::IncomingRinging).await;
	let live = wait_state(&mut cli, &id, CallState::Negotiating).await;
	assert!(live.auto_answered);
	assert!(sent(&rig, accepted));

	// A client that attaches later controls the call without asking for a takeover.
	let mut ui = rig.full_client("ui").await;
	let mut watch = ui.watch(WatchOptions::default()).await.expect("watch");
	let _ = watch.next().await;
	let context = ctx(&ui, 50);
	let lease = ui
		.raw()
		.acquire_controller(AcquireControllerRequest {
			context: Some(context),
			call_id: id.clone(),
			request_explicit_takeover: false,
		})
		.await
		.expect("lease");
	assert!(lease.into_inner().generation > 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn with_background_off_a_flagged_contact_is_declined_not_answered() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("status").await;
	flagged_contact(&rig, &mut cli).await;
	setting(&mut cli, "background.available", "false").await;
	invite(&rig, 5).await;
	eventually!(sent(&rig, declined).then_some(()));
	assert!(!sent(&rig, accepted));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_quiet_shell_silences_the_engine_ring_until_it_says_otherwise() {
	let rig = Rig::start().await;
	let mut shell = rig.full_client("phone").await;
	let mut watch = shell.watch(WatchOptions::default()).await.expect("watch");
	let _ = watch.next().await;
	let done = shell.set_client_quiet(true).await.expect("quiet");
	assert_applied(&done);

	let (call, session) = invite(&rig, 6).await;
	let id = call.to_bytes().to_vec();
	wait_state(&mut shell, &id, CallState::IncomingRinging).await;
	settle().await;
	assert!(!rang(&rig), "the shell alerts this call itself");

	// Back to loud: the same ringing call now rings.
	shell.set_client_quiet(false).await.expect("loud");
	eventually!(rang(&rig).then_some(()));
	rig.daemon
		.control()
		.peer(PeerEvent::Ended { call, session })
		.await
		.expect("ended");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn another_attached_client_that_is_not_quiet_still_gets_the_ring() {
	let rig = Rig::start().await;
	let mut shell = rig.full_client("phone").await;
	let mut shell_watch = shell.watch(WatchOptions::default()).await.expect("watch");
	let _ = shell_watch.next().await;
	shell.set_client_quiet(true).await.expect("quiet");
	let mut tui = rig.full_client("tui").await;
	let mut tui_watch = tui.watch(WatchOptions::default()).await.expect("watch");
	let _ = tui_watch.next().await;

	let (call, _) = invite(&rig, 7).await;
	let id = call.to_bytes().to_vec();
	wait_state(&mut tui, &id, CallState::IncomingRinging).await;
	eventually!(rang(&rig).then_some(()));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_quiet_flag_ends_with_the_last_watch() {
	let rig = Rig::start().await;
	let mut shell = rig.full_client("phone").await;
	let watch = shell.watch(WatchOptions::default()).await.expect("watch");
	shell.set_client_quiet(true).await.expect("quiet");
	drop(watch);
	settle().await;
	let mut again = shell.watch(WatchOptions::default()).await.expect("watch");
	let _ = again.next().await;

	let (call, _) = invite(&rig, 8).await;
	let id = call.to_bytes().to_vec();
	wait_state(&mut shell, &id, CallState::IncomingRinging).await;
	eventually!(rang(&rig).then_some(()));
}
