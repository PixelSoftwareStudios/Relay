mod common;

use std::sync::Arc;

use prost::Message;
use relay_net::control::{ControlError, control_channel, new_envelope, new_hello};
use relay_net::proto::{Accept, Envelope, Invite, Keepalive, envelope::Body};
use relay_net::record::RecordKind;
use relay_net::session::SecureSession;
use tokio::io::{AsyncWriteExt, DuplexStream, duplex};

fn invite(call: u8) -> Envelope {
	new_envelope(Body::Invite(Invite {
		call_id: vec![call; 16],
		offered_profiles: vec!["opus-mono-48k-10ms-v1".into()],
		ring_timeout_ms: 30_000,
		allow_parallel_same_peer: false,
	}))
}

/// Frames a sealed record exactly as the sender would.
fn frame(record: &[u8]) -> Vec<u8> {
	let mut out = (record.len() as u32).to_be_bytes().to_vec();
	out.extend_from_slice(record);
	out
}

fn sealed(session: &SecureSession, envelope: &Envelope) -> Vec<u8> {
	session
		.seal(RecordKind::Envelope, &envelope.encode_to_vec())
		.unwrap()
}

/// Receiver wired to a raw stream so tests can inject arbitrary bytes.
async fn raw_receiver() -> (
	Arc<SecureSession>,
	DuplexStream,
	relay_net::control::ControlReceiver<tokio::io::ReadHalf<DuplexStream>>,
	relay_net::identity::Fingerprint,
) {
	let link = common::link().await;
	let (raw, near) = duplex(1 << 20);
	// The receiver belongs to the responder; the raw end plays the initiator.
	let (_sender, receiver) = control_channel(near, Arc::clone(&link.responder));
	(
		link.initiator,
		raw,
		receiver,
		link.initiator_identity.fingerprint(),
	)
}

fn hello_from(fp: &relay_net::identity::Fingerprint) -> Envelope {
	new_envelope(Body::Hello(new_hello(fp, "alice", [7; 16])))
}

#[tokio::test]
async fn hello_then_call_signaling_roundtrips_over_loopback_tcp() {
	let link = common::link().await;
	let (mut a_tx, mut a_rx) = control_channel(link.initiator_stream, Arc::clone(&link.initiator));
	let (mut b_tx, mut b_rx) = control_channel(link.responder_stream, Arc::clone(&link.responder));

	a_tx.send(&hello_from(&link.initiator_identity.fingerprint()))
		.await
		.unwrap();
	b_tx.send(&hello_from(&link.responder_identity.fingerprint()))
		.await
		.unwrap();
	assert!(matches!(
		b_rx.recv().await.unwrap().body,
		Some(Body::Hello(_))
	));
	assert!(matches!(
		a_rx.recv().await.unwrap().body,
		Some(Body::Hello(_))
	));

	let inv = invite(9);
	a_tx.send(&inv).await.unwrap();
	assert_eq!(b_rx.recv().await.unwrap(), inv);

	let mut accept = new_envelope(Body::Accept(Accept {
		call_id: vec![9; 16],
		selected_profile: "opus-mono-48k-10ms-v1".into(),
		send_ssrc: 42,
		initial_stream_generation: 1,
	}));
	accept.reply_to_message_id = inv.message_id.clone();
	b_tx.send(&accept).await.unwrap();
	assert_eq!(a_rx.recv().await.unwrap(), accept);
}

#[tokio::test]
async fn first_message_must_be_a_hello() {
	let (init, mut raw, mut rx, _) = raw_receiver().await;
	raw.write_all(&frame(&sealed(&init, &invite(1))))
		.await
		.unwrap();
	assert!(matches!(rx.recv().await, Err(ControlError::HelloRequired)));
	assert!(matches!(rx.recv().await, Err(ControlError::Closed)));
}

#[tokio::test]
async fn hello_must_carry_the_authenticated_key_digest() {
	let (init, mut raw, mut rx, _) = raw_receiver().await;
	let impostor = relay_net::identity::Identity::generate().unwrap();
	raw.write_all(&frame(&sealed(&init, &hello_from(&impostor.fingerprint()))))
		.await
		.unwrap();
	assert!(matches!(rx.recv().await, Err(ControlError::HelloRequired)));
}

#[tokio::test]
async fn tampered_record_is_rejected_and_closes_the_channel() {
	let (init, mut raw, mut rx, fp) = raw_receiver().await;
	let mut record = sealed(&init, &hello_from(&fp));
	let last = record.len() - 1;
	record[last] ^= 0x01;
	raw.write_all(&frame(&record)).await.unwrap();
	assert!(matches!(rx.recv().await, Err(ControlError::Record(_))));
	assert!(matches!(rx.recv().await, Err(ControlError::Closed)));
}

#[tokio::test]
async fn replayed_and_reordered_records_are_rejected() {
	let (init, mut raw, mut rx, fp) = raw_receiver().await;
	let hello = sealed(&init, &hello_from(&fp));
	raw.write_all(&frame(&hello)).await.unwrap();
	assert!(rx.recv().await.is_ok());
	raw.write_all(&frame(&hello)).await.unwrap();
	assert!(matches!(rx.recv().await, Err(ControlError::Replay)));

	// A fresh channel: the later record first, then the earlier one.
	let (init, mut raw, mut rx, fp) = raw_receiver().await;
	let first = sealed(&init, &hello_from(&fp));
	let later = sealed(&init, &invite(3));
	raw.write_all(&frame(&first)).await.unwrap();
	rx.recv().await.unwrap();
	raw.write_all(&frame(&later)).await.unwrap();
	rx.recv().await.unwrap();
	raw.write_all(&frame(&first)).await.unwrap();
	assert!(matches!(rx.recv().await, Err(ControlError::Replay)));
}

#[tokio::test]
async fn oversize_and_undersize_length_prefixes_are_rejected() {
	for len in [0u32, 10, 16 * 1024 + 1, u32::MAX] {
		let (_init, mut raw, mut rx, _) = raw_receiver().await;
		raw.write_all(&len.to_be_bytes()).await.unwrap();
		assert!(
			matches!(rx.recv().await, Err(ControlError::BadLength)),
			"len {len}"
		);
	}
}

#[tokio::test]
async fn media_lane_record_on_control_stream_is_rejected() {
	let (init, mut raw, mut rx, _) = raw_receiver().await;
	let media = init.seal(RecordKind::Audio, &[0u8; 24]).unwrap();
	raw.write_all(&frame(&media)).await.unwrap();
	assert!(matches!(rx.recv().await, Err(ControlError::WrongKind)));
}

#[tokio::test]
async fn garbage_body_that_authenticates_but_is_not_protobuf_is_rejected() {
	let (init, mut raw, mut rx, _) = raw_receiver().await;
	let record = init.seal(RecordKind::Envelope, &[0xFF; 20]).unwrap();
	raw.write_all(&frame(&record)).await.unwrap();
	assert!(matches!(
		rx.recv().await,
		Err(ControlError::Decode | ControlError::Invalid(_))
	));
}

#[tokio::test]
async fn identical_duplicate_message_is_dropped_but_conflicting_one_is_an_error() {
	let (init, mut raw, mut rx, fp) = raw_receiver().await;
	raw.write_all(&frame(&sealed(&init, &hello_from(&fp))))
		.await
		.unwrap();
	rx.recv().await.unwrap();

	let original = invite(4);
	let next = invite(5);
	for e in [&original, &original, &next] {
		raw.write_all(&frame(&sealed(&init, e))).await.unwrap();
	}
	assert_eq!(rx.recv().await.unwrap(), original);
	assert_eq!(
		rx.recv().await.unwrap(),
		next,
		"identical duplicate skipped"
	);

	let mut conflicting = invite(6);
	conflicting.message_id = original.message_id.clone();
	raw.write_all(&frame(&sealed(&init, &conflicting)))
		.await
		.unwrap();
	assert!(matches!(
		rx.recv().await,
		Err(ControlError::ConflictingDuplicate)
	));
}

#[tokio::test]
async fn structurally_invalid_envelopes_are_refused_on_both_ends() {
	let link = common::link().await;
	let (mut tx, _rx) = control_channel(link.initiator_stream, Arc::clone(&link.initiator));
	let mut bad = invite(1);
	bad.message_id = vec![1; 3];
	assert!(matches!(tx.send(&bad).await, Err(ControlError::Invalid(_))));
	// A failed send poisons the half rather than continuing in unknown state.
	assert!(matches!(
		tx.send(&invite(1)).await,
		Err(ControlError::Closed)
	));

	let (init, mut raw, mut rx, fp) = raw_receiver().await;
	raw.write_all(&frame(&sealed(&init, &hello_from(&fp))))
		.await
		.unwrap();
	rx.recv().await.unwrap();
	let mut no_body = invite(1);
	no_body.body = None;
	raw.write_all(&frame(&sealed(&init, &no_body)))
		.await
		.unwrap();
	assert!(matches!(rx.recv().await, Err(ControlError::Invalid(_))));
}

#[tokio::test]
async fn keepalive_and_wrong_major_envelopes() {
	let (init, mut raw, mut rx, fp) = raw_receiver().await;
	raw.write_all(&frame(&sealed(&init, &hello_from(&fp))))
		.await
		.unwrap();
	rx.recv().await.unwrap();
	let ka = new_envelope(Body::Keepalive(Keepalive {
		nonce: 5,
		reply: false,
	}));
	raw.write_all(&frame(&sealed(&init, &ka))).await.unwrap();
	assert_eq!(rx.recv().await.unwrap(), ka);
	let mut wrong = new_envelope(Body::Keepalive(Keepalive {
		nonce: 6,
		reply: false,
	}));
	wrong.protocol_major = 2;
	raw.write_all(&frame(&sealed(&init, &wrong))).await.unwrap();
	assert!(matches!(rx.recv().await, Err(ControlError::Invalid(_))));
}
