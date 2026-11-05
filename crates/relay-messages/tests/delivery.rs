mod common;

use common::*;
use relay_messages::Clock;
use relay_messages::limits::*;
use relay_messages::message::*;
use relay_messages::store::{ReceiveOutcome, StoreAction};
use relay_messages::{RejectReason, StoreError};

#[test]
fn outbound_burst_then_refill_with_injected_clock() {
	let (mut s, clock) = store();
	for i in 0..MESSAGE_BURST {
		s.compose_text(&format!("m{i}")).unwrap();
	}
	assert_eq!(s.compose_text("over").unwrap_err(), StoreError::RateLimited);
	clock.advance(99);
	assert_eq!(
		s.compose_text("still over").unwrap_err(),
		StoreError::RateLimited
	);
	clock.advance(1);
	s.compose_text("one token").unwrap();
	clock.advance(1000);
	for i in 0..MESSAGE_RATE_PER_SEC {
		s.compose_text(&format!("n{i}")).unwrap();
	}
	assert_eq!(
		s.compose_text("again over").unwrap_err(),
		StoreError::RateLimited
	);
}

#[test]
fn inbound_flood_is_rejected_with_reason() {
	let (mut s, _) = store();
	let mut rejected = 0;
	for i in 1..=100u64 {
		match s.receive(wire_text(i, "spam")) {
			Ok(_) => {}
			Err(e) => {
				assert_eq!(e.reject_reason(), RejectReason::RateLimited);
				rejected += 1;
			}
		}
	}
	assert_eq!(s.messages().len(), MESSAGE_BURST as usize);
	assert_eq!(rejected, 100 - MESSAGE_BURST);
}

#[test]
fn inbound_byte_cap_stops_image_floods() {
	let (mut s, clock) = store();
	let mut img = png(10, 10);
	img.resize(MAX_IMAGE_BYTES, 0);
	let img: std::sync::Arc<[u8]> = img.into();
	let mut ok = 0;
	for i in 1..=6u64 {
		clock.advance(1000);
		if s.receive(Wire {
			seq: i,
			sent_at_ms: 0,
			payload: Payload::Image(img.clone()),
		})
		.is_ok()
		{
			ok += 1;
		}
	}
	// 16 MiB burst plus refill: far fewer than 6 full-size images fit in 6 s.
	assert!((2..6).contains(&ok), "ok = {ok}");
}

#[test]
fn ordering_uses_arrival_not_remote_time() {
	let (mut s, clock) = store();
	s.receive(Wire {
		seq: 1,
		sent_at_ms: 9_000_000_000_000,
		payload: Payload::Text("far future".into()),
	})
	.unwrap();
	clock.advance(10);
	s.receive(Wire {
		seq: 2,
		sent_at_ms: 1,
		payload: Payload::Text("epoch".into()),
	})
	.unwrap();
	clock.advance(10);
	s.compose_text("mine").unwrap();
	let order: Vec<_> = s
		.messages()
		.iter()
		.map(|m| (m.id.origin, m.id.seq))
		.collect();
	assert_eq!(
		order,
		vec![(Origin::Remote, 1), (Origin::Remote, 2), (Origin::Local, 1)]
	);
	let m = s
		.get(MessageId {
			origin: Origin::Remote,
			seq: 1,
		})
		.unwrap();
	assert_eq!(m.sent_at_ms, 9_000_000_000_000);
	assert_ne!(m.received_at_ms, m.sent_at_ms);
}

#[test]
fn duplicates_are_idempotent_and_gaps_are_legal() {
	let (mut s, _) = store();
	assert!(matches!(
		s.receive(wire_text(1, "a")),
		Ok(ReceiveOutcome::Stored(_))
	));
	assert_eq!(
		s.receive(wire_text(1, "a")).unwrap(),
		ReceiveOutcome::Duplicate
	);
	assert_eq!(
		s.receive(wire_text(0, "a")).unwrap_err(),
		StoreError::BadSequence(0)
	);
	assert!(matches!(
		s.receive(wire_text(5, "b")),
		Ok(ReceiveOutcome::Stored(_))
	));
	assert_eq!(
		s.receive(wire_text(3, "late")).unwrap(),
		ReceiveOutcome::Duplicate
	);
	assert_eq!(s.messages().len(), 2);
}

#[test]
fn rejected_content_still_consumes_the_sequence() {
	let (mut s, _) = store();
	assert!(s.receive(wire_text(1, "bad\u{202E}text")).is_err());
	assert_eq!(
		s.receive(wire_text(1, "good")).unwrap(),
		ReceiveOutcome::Duplicate
	);
	assert!(s.receive(wire_text(2, "good")).is_ok());
}

#[test]
fn ack_delivers_and_rejection_fails_without_retry() {
	let (mut s, clock) = store();
	let a = s.compose_text("a").unwrap();
	let b = s.compose_text("b").unwrap();
	assert_eq!(
		s.get(MessageId {
			origin: Origin::Local,
			seq: a.seq
		})
		.unwrap()
		.delivery,
		Delivery::Sending
	);
	s.on_ack(a.seq, AckStatus::Delivered).unwrap();
	s.on_ack(b.seq, AckStatus::Rejected(RejectReason::Disabled))
		.unwrap();
	let get = |s: &relay_messages::SessionStore, q| {
		s.get(MessageId {
			origin: Origin::Local,
			seq: q,
		})
		.unwrap()
		.delivery
	};
	assert_eq!(get(&s, a.seq), Delivery::Delivered);
	assert_eq!(
		get(&s, b.seq),
		Delivery::Failed(FailReason::Rejected(RejectReason::Disabled))
	);
	clock.advance(10 * 60_000);
	assert!(s.poll().is_empty());
	assert_eq!(
		s.on_ack(99, AckStatus::Delivered).unwrap_err(),
		StoreError::NotFound
	);
}

#[test]
fn unacked_message_is_resent_with_backoff_then_fails() {
	let (mut s, clock) = store();
	let w = s.compose_text("hello").unwrap();
	assert!(s.poll().is_empty());
	let mut waits = Vec::new();
	let mut resends = 0;
	let mut last = clock.mono_ms();
	loop {
		let d = s.next_deadline().expect("pending");
		clock.advance(d - clock.mono_ms());
		let acts = s.poll();
		assert_eq!(acts.len(), 1);
		waits.push(clock.mono_ms() - last);
		last = clock.mono_ms();
		match &acts[0] {
			StoreAction::Resend(r) => {
				assert_eq!(r, &w);
				resends += 1;
			}
			StoreAction::Failed(seq) => {
				assert_eq!(*seq, w.seq);
				break;
			}
		}
	}
	assert_eq!(resends, MAX_RETRIES);
	assert_eq!(waits, vec![5_000, 10_000, 20_000, 40_000]);
	let m = s
		.get(MessageId {
			origin: Origin::Local,
			seq: w.seq,
		})
		.unwrap();
	assert_eq!(m.delivery, Delivery::Failed(FailReason::Timeout));
	assert!(s.next_deadline().is_none());
}

#[test]
fn user_retry_resets_attempts_and_late_ack_still_delivers() {
	let (mut s, clock) = store();
	let w = s.compose_text("hello").unwrap();
	clock.advance(120_000);
	let mut guard = 0;
	while s.next_deadline().is_some() && guard < 10 {
		clock.advance(60_000);
		s.poll();
		guard += 1;
	}
	let again = s.retry(w.seq).unwrap();
	assert_eq!(again.seq, w.seq);
	assert_eq!(s.retry(w.seq).unwrap_err(), StoreError::WrongState);
	s.on_ack(w.seq, AckStatus::Delivered).unwrap();
	assert_eq!(
		s.get(MessageId {
			origin: Origin::Local,
			seq: w.seq
		})
		.unwrap()
		.delivery,
		Delivery::Delivered
	);
}

#[test]
fn ending_a_session_fails_pending_and_stops_timers() {
	let (mut s, _) = store();
	let w = s.compose_text("hello").unwrap();
	s.end();
	assert_eq!(
		s.get(MessageId {
			origin: Origin::Local,
			seq: w.seq
		})
		.unwrap()
		.delivery,
		Delivery::Failed(FailReason::Timeout)
	);
	assert!(s.next_deadline().is_none());
	assert_eq!(s.retry(w.seq).unwrap_err(), StoreError::Ended);
}

#[test]
fn oversize_and_invalid_text_get_typed_reasons() {
	let (mut s, _) = store();
	let big = "a".repeat(MAX_TEXT_BYTES + 1);
	assert_eq!(
		s.receive(wire_text(1, &big)).unwrap_err().reject_reason(),
		RejectReason::TooLarge
	);
	assert_eq!(
		s.receive(wire_text(2, "x\u{0}y"))
			.unwrap_err()
			.reject_reason(),
		RejectReason::Invalid
	);
	assert!(s.receive(wire_text(3, &"a".repeat(MAX_TEXT_BYTES))).is_ok());
}

#[test]
fn redacted_output_hides_content() {
	let (mut s, _) = store();
	s.compose_text("super secret").unwrap();
	let m = &s.messages()[0];
	assert!(!format!("{m:?}").contains("secret"));
	assert!(!format!("{}", Redacted(m)).contains("secret"));
	assert!(!format!("{:?}", s.compose_text("another secret").unwrap()).contains("secret"));
}
