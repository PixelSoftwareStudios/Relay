#![cfg(unix)]
//! Call history recording on a real daemon with fake adapters: one entry per call the user
//! saw, with the outcome, nothing while the setting is off, and a revision for watchers.

#[macro_use]
mod service_support;

use relay_api::pb::{
	AcquireControllerRequest, CallDirection, CallHistoryEntry, CallOutcome, CallState,
};
use relay_api::{RelayClient, WatchOptions};
use relay_core::{CallId, PeerEvent, SessionId};
use service_support::*;

async fn history(cli: &mut RelayClient) -> Vec<CallHistoryEntry> {
	cli.list_call_history(0, None).await.expect("list").entries
}

async fn entries(cli: &mut RelayClient, count: usize) -> Vec<CallHistoryEntry> {
	eventually!({
		let found = history(cli).await;
		(found.len() >= count).then_some(found)
	})
}

async fn attach(rig: &Rig) -> (RelayClient, relay_api::StateWatch) {
	let mut cli = rig.full_client("ui").await;
	let mut watch = cli.watch(WatchOptions::default()).await.expect("watch");
	let _ = watch.next().await;
	(cli, watch)
}

async fn invite(rig: &Rig, call: u128) -> (CallId, SessionId) {
	let control = rig.daemon.control();
	let (call, session) = (CallId::from_u128(call), SessionId::from_u128(call + 100));
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

/// Starts a call to the fake peer and brings it to ringing.
async fn ring_out(rig: &Rig, cli: &mut RelayClient, op: u8) -> Vec<u8> {
	let call = start_call(cli, op).await.call_id;
	rig.daemon
		.control()
		.peer(PeerEvent::LinkUp(peer_id()))
		.await
		.unwrap();
	wait_state(cli, &call, CallState::OutgoingRinging).await;
	call
}

async fn hang_up(cli: &mut RelayClient, op: u8, call: &[u8]) {
	let generation = controller_of(&call_in(cli, call).await.expect("call")).generation;
	let request = mutation(cli, op, call, generation);
	let done = cli.raw().end_call(request).await.expect("end").into_inner();
	assert_applied(&done);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_completed_outgoing_call_is_recorded_with_its_duration() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("ui").await;
	let op = start_call(&mut cli, 1).await;
	let call = op.call_id.clone();
	connect_call(&rig, &mut cli, &call).await;
	tokio::time::sleep(std::time::Duration::from_millis(120)).await;
	hang_up(&mut cli, 2, &call).await;
	let rows = entries(&mut cli, 1).await;
	let row = &rows[0];
	assert_eq!(row.outcome, CallOutcome::Completed as i32);
	assert_eq!(row.direction, CallDirection::Outgoing as i32);
	assert!(row.duration_ms >= 100, "{}", row.duration_ms);
	assert_eq!(row.call_id, call);
	assert_eq!(row.peer_id, vec![PEER; 32]);
	assert_eq!(row.peer_fingerprint_short.len(), 24);
	assert!(row.started_at_unix_ms > 1_600_000_000_000);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_outgoing_call_the_peer_declined_or_the_user_cancelled_is_recorded() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("ui").await;
	let control = rig.daemon.control();

	let declined = start_call(&mut cli, 1).await.call_id;
	control.peer(PeerEvent::LinkUp(peer_id())).await.unwrap();
	let id = call_id_of(&declined);
	let session = eventually!(
		rig.fakes
			.peers
			.sent()
			.into_iter()
			.find_map(|(_, m)| match m {
				relay_core::PeerMessage::Invite { call, session, .. } if call == id =>
					Some(session),
				_ => None,
			})
	);
	control
		.peer(PeerEvent::Declined { call: id, session })
		.await
		.unwrap();
	wait_state(&mut cli, &declined, CallState::Declined).await;
	assert_eq!(
		entries(&mut cli, 1).await[0].outcome,
		CallOutcome::Declined as i32
	);

	let cancelled = ring_out(&rig, &mut cli, 2).await;
	hang_up(&mut cli, 3, &cancelled).await;
	let rows = entries(&mut cli, 2).await;
	assert_eq!(rows[0].outcome, CallOutcome::Cancelled as i32);
	assert_eq!(rows[1].outcome, CallOutcome::Declined as i32);
	assert!(rows[0].id > rows[1].id, "newest first");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_outgoing_call_that_cannot_reach_the_peer_is_recorded_as_failed() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("ui").await;
	let call = start_call(&mut cli, 1).await.call_id;
	let control = rig.daemon.control();
	control.peer(PeerEvent::LinkDown(peer_id())).await.unwrap();
	wait_state(&mut cli, &call, CallState::Failed).await;
	let rows = entries(&mut cli, 1).await;
	assert_eq!(rows[0].outcome, CallOutcome::Failed as i32);
	assert_eq!(rows[0].duration_ms, 0);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unanswered_incoming_call_is_missed_and_a_declined_one_declined() {
	let rig = Rig::start().await;
	let (mut ui, _watch) = attach(&rig).await;
	let control = rig.daemon.control();

	let (missed, session) = invite(&rig, 1).await;
	wait_state(&mut ui, &missed.to_bytes(), CallState::IncomingRinging).await;
	control
		.peer(PeerEvent::Ended {
			call: missed,
			session,
		})
		.await
		.unwrap();
	let rows = entries(&mut ui, 1).await;
	assert_eq!(rows[0].outcome, CallOutcome::Missed as i32);
	assert_eq!(rows[0].direction, CallDirection::Incoming as i32);
	assert_eq!(rows[0].call_id, missed.to_bytes());

	let (refused, _) = invite(&rig, 2).await;
	wait_state(&mut ui, &refused.to_bytes(), CallState::IncomingRinging).await;
	let call = refused.to_bytes();
	let take = AcquireControllerRequest {
		context: Some(ctx(&ui, 8)),
		call_id: call.to_vec(),
		request_explicit_takeover: false,
	};
	let generation = ui
		.raw()
		.acquire_controller(take)
		.await
		.unwrap()
		.into_inner()
		.generation;
	let request = mutation(&ui, 9, &call, generation);
	let done = ui.raw().decline_call(request).await.unwrap().into_inner();
	assert_applied(&done);
	let rows = entries(&mut ui, 2).await;
	assert_eq!(rows[0].outcome, CallOutcome::Declined as i32);
	assert_eq!(rows[1].outcome, CallOutcome::Missed as i32);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_call_nobody_saw_is_not_recorded() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("ui").await;
	// No client watches, so the daemon answers busy and the call never exists.
	let (call, _) = invite(&rig, 1).await;
	tokio::time::sleep(std::time::Duration::from_millis(300)).await;
	assert!(call_in(&mut cli, &call.to_bytes()).await.is_none());
	assert!(history(&mut cli).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nothing_is_recorded_while_history_is_off_and_old_entries_stay_until_cleared() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("ui").await;
	let first = ring_out(&rig, &mut cli, 1).await;
	hang_up(&mut cli, 2, &first).await;
	entries(&mut cli, 1).await;

	let answer = cli
		.set_settings([("history.enabled", "false")])
		.await
		.unwrap();
	assert_eq!(
		answer.results[0].outcome,
		relay_api::pb::SettingOutcome::AppliedLive as i32
	);
	assert!(
		!cli.snapshot()
			.await
			.unwrap()
			.settings
			.unwrap()
			.history_enabled
	);
	let second = ring_out(&rig, &mut cli, 3).await;
	hang_up(&mut cli, 4, &second).await;
	wait_state(&mut cli, &second, CallState::Cancelled).await;
	tokio::time::sleep(std::time::Duration::from_millis(200)).await;
	assert_eq!(history(&mut cli).await.len(), 1, "kept, nothing new");

	assert_applied(&cli.clear_call_history().await.unwrap());
	assert!(history(&mut cli).await.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_revision_changes_when_an_entry_is_added_or_cleared() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("ui").await;
	async fn revision(cli: &mut RelayClient) -> u64 {
		cli.snapshot()
			.await
			.unwrap()
			.settings
			.unwrap()
			.history_revision
	}
	let before = revision(&mut cli).await;
	let call = ring_out(&rig, &mut cli, 1).await;
	hang_up(&mut cli, 2, &call).await;
	let added = eventually!({
		let now = revision(&mut cli).await;
		(now > before).then_some(now)
	});
	assert_eq!(
		cli.list_call_history(0, None).await.unwrap().revision,
		added
	);
	cli.clear_call_history().await.unwrap();
	assert!(revision(&mut cli).await > added);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pages_continue_after_the_cursor_and_reading_needs_only_observe() {
	let rig = Rig::start().await;
	let mut cli = rig.full_client("ui").await;
	for op in 0..3u8 {
		let call = ring_out(&rig, &mut cli, 10 + op * 2).await;
		hang_up(&mut cli, 11 + op * 2, &call).await;
		entries(&mut cli, usize::from(op) + 1).await;
	}
	let page = cli.list_call_history(2, None).await.unwrap();
	assert_eq!(page.entries.len(), 2);
	assert!(page.more);
	let rest = cli
		.list_call_history(2, Some(page.entries[1].id))
		.await
		.unwrap();
	assert_eq!(rest.entries.len(), 1);
	assert!(!rest.more);

	let mut reader = rig.client("reader", &["observe"]).await;
	assert_eq!(
		reader
			.list_call_history(0, None)
			.await
			.unwrap()
			.entries
			.len(),
		3
	);
	let denied = reader.clear_call_history().await.unwrap_err();
	assert_eq!(code_of(&denied), tonic::Code::PermissionDenied);
}
