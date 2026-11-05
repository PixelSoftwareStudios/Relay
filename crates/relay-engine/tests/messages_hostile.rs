#![cfg(unix)]
//! A trusted but hostile peer: messages for sessions it is not in, forged control messages
//! and floods never reach another call's transcript or state.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_evil;
mod support_media;
mod support_messages;

use std::time::Duration;

use relay_net::identity::Fingerprint;
use relay_net::proto::session_message::Body;
use relay_net::proto::{
	CancelReason, DataLinkGrant, FileOffer, OfferResponse, PermissionUpdate, SessionMessage,
	TransferControl, chat_ack, transfer_control,
};
use support_channels::{id_of, pair, settle, side};
use support_evil::{acks, hostile, text};
use support_messages::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn chat_without_a_session_is_answered_ended_and_stored_nowhere() {
	let mut a = side(440.0).await;
	let mut evil = hostile(&mut a).await;
	let a_id =
		relay_engine::peers::peer_id(&Fingerprint::from_bytes(*a.daemon.local_peer().as_bytes()));
	evil.handle.send_session(a_id, text(1, "hello?")).unwrap();
	let got = acks(&mut evil, Duration::from_millis(500)).await;
	assert_eq!(got, [(1, chat_ack::Status::Ended as i32)]);
	let mut m = messenger(&a).await;
	assert!(m.list_sessions().await.unwrap().sessions.is_empty());
	a.daemon.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_stranger_cannot_write_into_someone_elses_call() {
	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	pair(&mut a, &mut b, 1).await;
	connect(&mut a, &mut b, 10).await;
	let mut ma = messenger(&a).await;
	let mut mb = messenger(&b).await;
	let sa = active_session(&mut ma).await.session_id;
	let sb = active_session(&mut mb).await.session_id;
	assert_ok(&ma.send_session_message(&sa, "real message").await.unwrap());
	wait_transcript(&mut mb, &sb, |e| e.len() == 1).await;

	let mut evil = hostile(&mut a).await;
	let a_id =
		relay_engine::peers::peer_id(&Fingerprint::from_bytes(*a.daemon.local_peer().as_bytes()));
	// Forged chat, an offer, an accept for an offer id it guessed, a data-link grant, a
	// cancel and a permission push: none of it touches the A-B session.
	evil.handle
		.send_session(a_id, text(1, "injected text"))
		.unwrap();
	evil.handle
		.send_session(
			a_id,
			SessionMessage {
				body: Some(Body::Offer(FileOffer {
					seq: 2,
					sent_at_unix_ms: 0,
					offer_id: vec![5; 16],
					name: "injected.exe".into(),
					size: 10,
					media_type: String::new(),
					blake3: vec![0; 32],
				})),
			},
		)
		.unwrap();
	for body in [
		Body::OfferResponse(OfferResponse {
			offer_id: vec![5; 16],
			accepted: true,
			reason: String::new(),
		}),
		Body::Transfer(TransferControl {
			offer_id: vec![5; 16],
			op: Some(transfer_control::Op::DataLink(DataLinkGrant {
				token: vec![1; 32],
				port: 4000,
				ttl_ms: 1000,
			})),
		}),
		Body::Transfer(TransferControl {
			offer_id: vec![5; 16],
			op: Some(transfer_control::Op::Cancel(
				CancelReason::CancelUser as i32,
			)),
		}),
		Body::Permissions(PermissionUpdate {
			allow_messages: false,
			allow_files: false,
		}),
	] {
		evil.handle
			.send_session(a_id, SessionMessage { body: Some(body) })
			.unwrap();
	}
	let got = acks(&mut evil, Duration::from_millis(500)).await;
	assert!(
		got.iter()
			.all(|(_, status)| *status == chat_ack::Status::Ended as i32),
		"{got:?}"
	);

	let sessions = ma.list_sessions().await.unwrap().sessions;
	assert_eq!(sessions.len(), 1);
	assert!(
		sessions[0].permissions.unwrap().allow_messages,
		"the forged push changed nothing"
	);
	assert_eq!(texts(&transcript(&mut ma, &sa).await), ["real message"]);
	assert_eq!(texts(&transcript(&mut mb, &sb).await), ["real message"]);
	// The real call keeps working.
	assert_ok(&mb.send_session_message(&sb, "still fine").await.unwrap());
	wait_transcript(&mut ma, &sa, |e| e.len() == 2).await;
	a.daemon.shutdown().await.unwrap();
	b.daemon.shutdown().await.unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_flood_closes_the_hostile_link_and_nothing_else() {
	let mut a = side(440.0).await;
	let mut b = side(1_700.0).await;
	pair(&mut a, &mut b, 1).await;
	connect(&mut a, &mut b, 10).await;
	let mut ma = messenger(&a).await;
	let mut mb = messenger(&b).await;
	let sa = active_session(&mut ma).await.session_id;
	let sb = active_session(&mut mb).await.session_id;
	let mut evil = hostile(&mut a).await;
	let a_id =
		relay_engine::peers::peer_id(&Fingerprint::from_bytes(*a.daemon.local_peer().as_bytes()));
	for n in 1..=400 {
		let _ = evil.handle.send_session(a_id, text(n, "flood"));
	}
	settle(800).await;
	let got = acks(&mut evil, Duration::from_millis(300)).await;
	assert!(
		got.len() < 400,
		"the link was cut before every message was answered: {}",
		got.len()
	);
	assert!(ma.list_sessions().await.unwrap().sessions.len() == 1);
	assert!(transcript(&mut ma, &sa).await.is_empty());
	assert_ok(&ma.send_session_message(&sa, "unaffected").await.unwrap());
	wait_transcript(&mut mb, &sb, |e| e.len() == 1).await;
	let _ = id_of(&a);
	a.daemon.shutdown().await.unwrap();
	b.daemon.shutdown().await.unwrap();
}
