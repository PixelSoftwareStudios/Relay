#![cfg(unix)]
//! Acks, retries and give-ups against a raw peer that controls what it answers.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_evil;
mod support_media;
mod support_messages;

use std::time::Duration;

use relay_api::pb::{CallState, DeliveryState};
use relay_engine::peers::{PeerOutput, SessionEvent};
use relay_net::identity::Fingerprint;
use relay_net::proto::session_message::Body;
use relay_net::proto::{ChatAck, SessionMessage, chat_ack, chat_message};
use support_channels::id_of;
use support_evil::hostile;
use support_messages::*;

/// A side whose connecting calls wait long enough for the retry timers.
async fn patient() -> support_channels::Side {
	support_channels::side_tuned(440.0, None, true, |o| {
		o.limits.path_timeout = Duration::from_secs(60);
	})
	.await
}

fn ack(seq: u64, status: chat_ack::Status) -> SessionMessage {
	SessionMessage {
		body: Some(Body::Ack(ChatAck {
			seq,
			status: status as i32,
		})),
	}
}

/// The next chat message the daemon sent to the raw peer: (seq, text).
async fn next_chat(evil: &mut support_evil::TestPeer, within: Duration) -> Option<(u64, String)> {
	let deadline = tokio::time::Instant::now() + within;
	while let Ok(Some(item)) = tokio::time::timeout_at(deadline, evil.output.recv()).await {
		if let PeerOutput::Session(SessionEvent::Message { message, .. }) = item
			&& let Some(Body::Message(m)) = message.body
			&& let Some(chat_message::Content::Text(t)) = m.content
		{
			return Some((m.seq, t));
		}
	}
	None
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_unacked_message_is_resent_with_the_same_sequence_until_acked() {
	let mut a = patient().await;
	let mut evil = hostile(&mut a).await;
	let a_peer =
		relay_engine::peers::peer_id(&Fingerprint::from_bytes(*a.daemon.local_peer().as_bytes()));
	// A rings the raw peer, which accepts: the session exists while the call connects.
	let call = ring_raw(&mut a, &mut evil).await;
	let mut ma = messenger(&a).await;
	let session = active_session(&mut ma).await.session_id;
	let sent = ma
		.send_session_message(&session, "are you there?")
		.await
		.unwrap();
	assert_ok(&sent);

	let first = next_chat(&mut evil, Duration::from_secs(2))
		.await
		.expect("first send");
	assert_eq!(first, (1, "are you there?".to_owned()));
	assert!(
		next_chat(&mut evil, Duration::from_secs(3)).await.is_none(),
		"no resend inside 5 s"
	);
	let state = ma.list_sessions().await.unwrap().sessions.remove(0).state;
	let again = next_chat(&mut evil, Duration::from_secs(4)).await;
	let call_state = service_support::call_in(&mut a.cli, &call)
		.await
		.map(|c| c.state);
	let again = again.unwrap_or_else(|| panic!("resend; session {state}, call {call_state:?}"));
	assert_eq!(again, first, "the same sequence and text");
	let waiting = transcript(&mut ma, &session).await;
	assert_eq!(waiting[0].delivery, DeliveryState::Sending as i32);

	evil.handle
		.send_session(a_peer, ack(1, chat_ack::Status::Delivered))
		.unwrap();
	wait_transcript(&mut ma, &session, |e| {
		e[0].delivery == DeliveryState::Delivered as i32
	})
	.await;
	// Acks are idempotent: a duplicate changes nothing and a stray one is ignored.
	evil.handle
		.send_session(a_peer, ack(1, chat_ack::Status::Disabled))
		.unwrap();
	evil.handle
		.send_session(a_peer, ack(99, chat_ack::Status::Delivered))
		.unwrap();
	tokio::time::sleep(Duration::from_millis(300)).await;
	assert_eq!(
		transcript(&mut ma, &session).await[0].delivery,
		DeliveryState::Delivered as i32
	);
	let _ = call;
	a.daemon.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_rejected_ack_fails_the_message_at_once_and_a_user_retry_resends_it() {
	let mut a = patient().await;
	let mut evil = hostile(&mut a).await;
	let a_peer =
		relay_engine::peers::peer_id(&Fingerprint::from_bytes(*a.daemon.local_peer().as_bytes()));
	ring_raw(&mut a, &mut evil).await;
	let mut ma = messenger(&a).await;
	let session = active_session(&mut ma).await.session_id;
	assert_ok(&ma.send_session_message(&session, "please").await.unwrap());
	next_chat(&mut evil, Duration::from_secs(2))
		.await
		.expect("sent");
	evil.handle
		.send_session(a_peer, ack(1, chat_ack::Status::RateLimited))
		.unwrap();
	let failed = wait_transcript(&mut ma, &session, |e| {
		e[0].delivery == DeliveryState::Failed as i32
	})
	.await;
	assert_eq!(failed[0].failure, "rate_limited");
	assert!(
		next_chat(&mut evil, Duration::from_millis(800))
			.await
			.is_none(),
		"a rejection never retries"
	);
	// The user may retry: the same sequence goes out again with a fresh attempt budget.
	assert_ok(&ma.retry_session_message(&session, 1).await.unwrap());
	assert_eq!(
		next_chat(&mut evil, Duration::from_secs(2))
			.await
			.unwrap()
			.0,
		1
	);
	evil.handle
		.send_session(a_peer, ack(1, chat_ack::Status::Delivered))
		.unwrap();
	wait_transcript(&mut ma, &session, |e| {
		e[0].delivery == DeliveryState::Delivered as i32
	})
	.await;
	// Retrying a delivered message is refused.
	assert_refused(
		&ma.retry_session_message(&session, 1).await.unwrap(),
		"MESSAGE_STATE",
	);
	let _ = (id_of(&a), CallState::Connected);
	a.daemon.shutdown().await.unwrap();
}

/// A rings the raw peer and the peer accepts, without any media: the call is connecting.
async fn ring_raw(a: &mut support_channels::Side, evil: &mut support_evil::TestPeer) -> Vec<u8> {
	let call = {
		let request = relay_api::pb::StartCallRequest {
			context: Some(service_support::ctx(&a.cli, 150)),
			target: Some(relay_api::pb::start_call_request::Target::PeerId(
				evil.id().as_bytes().to_vec(),
			)),
			allow_parallel_same_peer: false,
		};
		a.cli
			.raw()
			.start_call(request)
			.await
			.expect("call")
			.into_inner()
			.call_id
	};
	let a_peer =
		relay_engine::peers::peer_id(&Fingerprint::from_bytes(*a.daemon.local_peer().as_bytes()));
	let (id, session) = loop {
		let item = tokio::time::timeout(Duration::from_secs(5), evil.output.recv())
			.await
			.expect("invite in time")
			.expect("output");
		if let PeerOutput::Core(relay_core::PeerEvent::Invite { call, session, .. }) = item {
			break (call, session);
		}
	};
	evil.handle
		.send(
			a_peer,
			relay_core::PeerMessage::Accept { call: id, session },
		)
		.unwrap();
	service_support::wait_state(&mut a.cli, &call, CallState::Negotiating).await;
	call
}
