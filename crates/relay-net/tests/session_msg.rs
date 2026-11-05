mod common;

use std::sync::Arc;

use proptest::prelude::*;
use prost::Message;
use relay_messages::limits::{MAX_FILE_BYTES, MAX_TEXT_BYTES};
use relay_net::control::session_msg::{MAX_INLINE_IMAGE_BYTES, MAX_PREVIEW_WIRE_BYTES, validate};
use relay_net::control::{
	ControlError, SessionRateLimit, control_channel, new_envelope, new_hello, validate_envelope,
};
use relay_net::limits::CONTROL_RECORD_MAX_BYTES;
use relay_net::proto::session_message::Body as S;
use relay_net::proto::{
	ChatAck, ChatImage, ChatMessage, DataLinkGrant, Envelope, FileOffer, OfferResponse,
	PermissionUpdate, PreviewReply, PreviewRequest, SessionMessage, TransferControl, chat_ack,
	chat_message, envelope::Body, transfer_control,
};
use relay_net::record::RecordKind;
use tokio::io::{AsyncWriteExt, duplex};

fn msg(body: S) -> SessionMessage {
	SessionMessage { body: Some(body) }
}

fn text(seq: u64, t: &str) -> SessionMessage {
	msg(S::Message(ChatMessage {
		seq,
		sent_at_unix_ms: 1,
		content: Some(chat_message::Content::Text(t.into())),
	}))
}

fn offer() -> FileOffer {
	FileOffer {
		seq: 1,
		sent_at_unix_ms: 0,
		offer_id: vec![1; 16],
		name: "report.pdf".into(),
		size: 10,
		media_type: "application/pdf".into(),
		blake3: vec![2; 32],
	}
}

fn grant() -> DataLinkGrant {
	DataLinkGrant {
		token: vec![3; 32],
		port: 4242,
		ttl_ms: 30_000,
	}
}

fn transfer(op: transfer_control::Op) -> SessionMessage {
	msg(S::Transfer(TransferControl {
		offer_id: vec![1; 16],
		op: Some(op),
	}))
}

fn all_valid() -> Vec<SessionMessage> {
	vec![
		text(1, "hello"),
		text(2, &"x".repeat(MAX_TEXT_BYTES)),
		msg(S::Message(ChatMessage {
			seq: 3,
			sent_at_unix_ms: 0,
			content: Some(chat_message::Content::Image(ChatImage {
				data: vec![7; MAX_INLINE_IMAGE_BYTES],
			})),
		})),
		msg(S::Ack(ChatAck {
			seq: 1,
			status: chat_ack::Status::RateLimited as i32,
		})),
		msg(S::Offer(offer())),
		msg(S::OfferResponse(OfferResponse {
			offer_id: vec![1; 16],
			accepted: false,
			reason: "no".into(),
		})),
		msg(S::PreviewRequest(PreviewRequest {
			offer_id: vec![1; 16],
		})),
		msg(S::PreviewReply(PreviewReply {
			offer_id: vec![1; 16],
			approved: true,
			text: "y".repeat(MAX_PREVIEW_WIRE_BYTES),
		})),
		transfer(transfer_control::Op::DataLink(grant())),
		transfer(transfer_control::Op::Result(0)),
		transfer(transfer_control::Op::Cancel(1)),
		msg(S::Permissions(PermissionUpdate {
			allow_messages: true,
			allow_files: false,
		})),
	]
}

#[test]
fn every_variant_validates_and_largest_fit_a_control_record() {
	for m in all_valid() {
		validate(&m).unwrap();
		let env = new_envelope(Body::SessionMessage(m));
		validate_envelope(&env).unwrap();
		// Record overhead is 81 bytes (header, inner copy, kind, tag).
		assert!(env.encoded_len() + 81 <= CONTROL_RECORD_MAX_BYTES);
		assert_eq!(
			Envelope::decode(env.encode_to_vec().as_slice()).unwrap(),
			env
		);
	}
}

#[test]
fn hostile_structures_are_refused() {
	let mut bad: Vec<SessionMessage> = vec![
		SessionMessage { body: None },
		text(0, "seq zero"),
		text(1, ""),
		text(1, &"x".repeat(MAX_TEXT_BYTES + 1)),
		msg(S::Message(ChatMessage {
			seq: 1,
			sent_at_unix_ms: 0,
			content: None,
		})),
		msg(S::Message(ChatMessage {
			seq: 1,
			sent_at_unix_ms: 0,
			content: Some(chat_message::Content::Image(ChatImage { data: vec![] })),
		})),
		msg(S::Message(ChatMessage {
			seq: 1,
			sent_at_unix_ms: 0,
			content: Some(chat_message::Content::Image(ChatImage {
				data: vec![0; MAX_INLINE_IMAGE_BYTES + 1],
			})),
		})),
		msg(S::Ack(ChatAck { seq: 1, status: 99 })),
		msg(S::Ack(ChatAck { seq: 0, status: 0 })),
		msg(S::PreviewRequest(PreviewRequest {
			offer_id: vec![1; 15],
		})),
		msg(S::PreviewReply(PreviewReply {
			offer_id: vec![1; 16],
			approved: true,
			text: "y".repeat(MAX_PREVIEW_WIRE_BYTES + 1),
		})),
		msg(S::PreviewReply(PreviewReply {
			offer_id: vec![1; 16],
			approved: false,
			text: "smuggled".into(),
		})),
		msg(S::OfferResponse(OfferResponse {
			offer_id: vec![1; 16],
			accepted: false,
			reason: "r".repeat(65),
		})),
		msg(S::Transfer(TransferControl {
			offer_id: vec![1; 16],
			op: None,
		})),
		transfer(transfer_control::Op::Result(77)),
		transfer(transfer_control::Op::Cancel(-1)),
	];
	for edit in [
		|o: &mut FileOffer| o.offer_id = vec![0; 17],
		|o: &mut FileOffer| o.name = String::new(),
		|o: &mut FileOffer| o.name = "n".repeat(256),
		|o: &mut FileOffer| o.size = MAX_FILE_BYTES + 1,
		|o: &mut FileOffer| o.media_type = "m".repeat(128),
		|o: &mut FileOffer| o.blake3 = vec![0; 31],
		|o: &mut FileOffer| o.seq = 0,
	] {
		let mut o = offer();
		edit(&mut o);
		bad.push(msg(S::Offer(o)));
	}
	for edit in [
		|g: &mut DataLinkGrant| g.token = vec![0; 31],
		|g: &mut DataLinkGrant| g.port = 0,
		|g: &mut DataLinkGrant| g.port = 70_000,
		|g: &mut DataLinkGrant| g.ttl_ms = 0,
		|g: &mut DataLinkGrant| g.ttl_ms = 120_001,
	] {
		let mut g = grant();
		edit(&mut g);
		bad.push(transfer(transfer_control::Op::DataLink(g)));
	}
	for m in bad {
		assert!(validate(&m).is_err(), "accepted {m:?}");
		assert!(validate_envelope(&new_envelope(Body::SessionMessage(m))).is_err());
	}
}

#[test]
fn unknown_fields_are_tolerated_and_full_size_offer_name_is_bounded() {
	// Field 99 (varint) appended to a valid message decodes and validates.
	let mut bytes = text(1, "hi").encode_to_vec();
	bytes.extend_from_slice(&[0xF8, 0x06, 0x01]);
	let decoded = SessionMessage::decode(bytes.as_slice()).unwrap();
	assert_eq!(decoded, text(1, "hi"));
	validate(&decoded).unwrap();
}

#[tokio::test]
async fn invalid_send_is_refused_without_poisoning_the_sender() {
	let link = common::link().await;
	let (mut tx, rx) = control_channel(link.initiator_stream, Arc::clone(&link.initiator));
	let (_t2, mut rx2) = control_channel(link.responder_stream, Arc::clone(&link.responder));
	let hello = new_envelope(Body::Hello(new_hello(
		&link.initiator_identity.fingerprint(),
		"a",
		[1; 16],
	)));
	tx.send(&hello).await.unwrap();
	let too_big = new_envelope(Body::SessionMessage(text(1, &"x".repeat(5000))));
	assert!(matches!(
		tx.send(&too_big).await,
		Err(ControlError::Invalid(_))
	));
	let ok = new_envelope(Body::SessionMessage(text(1, "fine")));
	tx.send(&ok).await.unwrap();
	assert!(matches!(
		rx2.recv().await.unwrap().body,
		Some(Body::Hello(_))
	));
	assert_eq!(rx2.recv().await.unwrap(), ok);
	drop(rx);
}

#[tokio::test]
async fn receiver_refuses_hostile_session_message_and_closes() {
	let link = common::link().await;
	let (raw, near) = duplex(1 << 20);
	let (_s, mut rx) = control_channel(near, Arc::clone(&link.responder));
	let mut raw = raw;
	let send = |env: &Envelope| {
		let record = link
			.initiator
			.seal(RecordKind::Envelope, &env.encode_to_vec())
			.unwrap();
		let mut out = (record.len() as u32).to_be_bytes().to_vec();
		out.extend_from_slice(&record);
		out
	};
	let hello = new_envelope(Body::Hello(new_hello(
		&link.initiator_identity.fingerprint(),
		"a",
		[1; 16],
	)));
	raw.write_all(&send(&hello)).await.unwrap();
	// Bypass sender-side validation by building the hostile envelope directly.
	let hostile = new_envelope(Body::SessionMessage(text(1, &"x".repeat(5000))));
	raw.write_all(&send(&hostile)).await.unwrap();
	assert!(rx.recv().await.is_ok());
	assert!(matches!(rx.recv().await, Err(ControlError::Invalid(_))));
	assert!(matches!(rx.recv().await, Err(ControlError::Closed)));
}

#[tokio::test]
async fn flood_of_session_messages_trips_the_link_guard() {
	let link = common::link().await;
	let (raw, near) = duplex(8 << 20);
	let (_s, mut rx) = control_channel(near, Arc::clone(&link.responder));
	rx.set_session_limit(SessionRateLimit::with_rate(1, 5, 0));
	let mut raw = raw;
	let mut all = Vec::new();
	let hello = new_envelope(Body::Hello(new_hello(
		&link.initiator_identity.fingerprint(),
		"a",
		[1; 16],
	)));
	let mut frames = vec![hello];
	frames.extend((1..=50).map(|i| new_envelope(Body::SessionMessage(text(i, "spam")))));
	for env in &frames {
		let record = link
			.initiator
			.seal(RecordKind::Envelope, &env.encode_to_vec())
			.unwrap();
		all.extend_from_slice(&(record.len() as u32).to_be_bytes());
		all.extend_from_slice(&record);
	}
	raw.write_all(&all).await.unwrap();
	let mut accepted = 0;
	loop {
		match rx.recv().await {
			Ok(_) => accepted += 1,
			Err(ControlError::RateLimited) => break,
			Err(e) => panic!("unexpected {e}"),
		}
	}
	// Hello plus about the burst (the wall clock may refill a token or two).
	assert!((6..=9).contains(&accepted), "accepted {accepted}");
}

#[test]
fn limiter_refills_on_its_injected_clock() {
	let mut l = SessionRateLimit::with_rate(10, 2, 0);
	assert!(l.allow(0) && l.allow(0));
	assert!(!l.allow(0));
	assert!(!l.allow(50));
	assert!(l.allow(100));
	assert!(!l.allow(100));
	// A clock that goes backwards neither panics nor refunds tokens.
	assert!(!l.allow(10));
}

proptest! {
	#[test]
	fn arbitrary_bytes_never_panic_and_accepted_messages_are_bounded(
		bytes in proptest::collection::vec(any::<u8>(), 0..2048)
	) {
		if let Ok(m) = SessionMessage::decode(bytes.as_slice())
			&& validate(&m).is_ok()
		{
			prop_assert!(m.encoded_len() <= CONTROL_RECORD_MAX_BYTES);
		}
	}

	#[test]
	fn text_validity_matches_the_bound(len in 0usize..9000) {
		let ok = validate(&text(1, &"a".repeat(len))).is_ok();
		prop_assert_eq!(ok, (1..=MAX_TEXT_BYTES).contains(&len));
	}

	#[test]
	fn offer_fields_are_checked_independently(
		id in proptest::collection::vec(any::<u8>(), 0..40),
		hash in proptest::collection::vec(any::<u8>(), 0..40),
		size in any::<u64>(),
		name in "\\PC{0,300}",
	) {
		let o = FileOffer { seq: 1, sent_at_unix_ms: 0, offer_id: id.clone(), name: name.clone(),
			size, media_type: String::new(), blake3: hash.clone() };
		let expect = id.len() == 16 && hash.len() == 32 && size <= MAX_FILE_BYTES
			&& !name.is_empty() && name.len() <= 255;
		prop_assert_eq!(validate(&msg(S::Offer(o))).is_ok(), expect);
	}
}
