#![cfg(unix)]
//! Session messaging contract: capability gating, transcripts, watch semantics.

mod common;

use common::Harness;
use relay_api::pb::{DeliveryState, OperationState, session_event};
use relay_api::{ApiError, SessionItem, entry_text};
use tonic::Code;

const SESSION: [u8; 16] = [7; 16];

async fn with_messages(h: &Harness) -> relay_api::RelayClient {
	let mut c = relay_api::RelayClient::connect(&h.endpoint())
		.await
		.unwrap();
	c.register("test", "0", &["observe", "messages"])
		.await
		.unwrap();
	c
}

fn status(e: ApiError) -> Code {
	match e {
		ApiError::Status(s) => s.code(),
		other => panic!("not a status: {other}"),
	}
}

#[tokio::test]
async fn every_call_needs_the_messages_capability() {
	let h = Harness::start().await;
	h.engine.messages.add_session(&SESSION);
	let mut plain = h.client().await;
	assert_eq!(
		status(plain.list_sessions().await.unwrap_err()),
		Code::PermissionDenied
	);
	assert_eq!(
		status(plain.read_transcript(&SESSION, 0).await.unwrap_err()),
		Code::PermissionDenied
	);
	assert_eq!(
		status(
			plain
				.send_session_message(&SESSION, "hi")
				.await
				.unwrap_err()
		),
		Code::PermissionDenied
	);
	assert_eq!(
		status(plain.watch_session(None, None).await.err().unwrap()),
		Code::PermissionDenied
	);
}

#[tokio::test]
async fn send_then_read_pages_and_search() {
	let h = Harness::start().await;
	h.engine.messages.add_session(&SESSION);
	let mut c = with_messages(&h).await;
	let sent = c.send_session_message(&SESSION, "Hello").await.unwrap();
	assert_eq!(
		sent.operation.unwrap().state,
		OperationState::Applied as i32
	);
	for i in 0..6 {
		h.engine.messages.deliver(&SESSION, &format!("reply {i}"));
	}
	let all = c.read_transcript(&SESSION, 0).await.unwrap();
	assert_eq!(all.len(), 7, "paged reads follow `more`");
	assert_eq!(entry_text(&all[0]), Some("Hello"));
	assert_eq!(all[0].delivery, DeliveryState::Delivered as i32);
	let ordinals: Vec<_> = all.iter().map(|e| e.ordinal).collect();
	assert!(ordinals.windows(2).all(|w| w[1] == w[0] + 1));
	let hits = c.search_session(&SESSION, "REPLY 3").await.unwrap();
	assert_eq!(hits.hits.len(), 1);
	let missing = c.send_session_message(&[9; 16], "x").await.unwrap();
	assert_eq!(
		missing.operation.unwrap().problem.unwrap().code,
		"SESSION_NOT_FOUND"
	);
}

#[tokio::test]
async fn watch_starts_with_a_list_then_events_without_gaps() {
	let h = Harness::start().await;
	h.engine.messages.add_session(&SESSION);
	let mut c = with_messages(&h).await;
	let mut watch = c.watch_session(None, None).await.unwrap();
	let Some(SessionItem::List(list)) = watch.next().await.unwrap() else {
		panic!("list first");
	};
	assert_eq!(list.sessions.len(), 1);
	h.engine.messages.deliver(&SESSION, "one");
	h.engine.messages.deliver(&SESSION, "two");
	let mut seen = Vec::new();
	for _ in 0..2 {
		let Some(SessionItem::Event { revision, event }) = watch.next().await.unwrap() else {
			panic!("event");
		};
		let Some(session_event::Event::Entry(entry)) = event.event else {
			panic!("entry");
		};
		seen.push((
			revision,
			entry_text(entry.entry.as_ref().unwrap())
				.unwrap()
				.to_owned(),
		));
	}
	assert_eq!(seen[0].1, "one");
	assert_eq!(seen[1].0, seen[0].0 + 1);
}

#[tokio::test]
async fn resume_after_a_missed_window_asks_for_resync() {
	let h = Harness::start().await;
	h.engine.messages.add_session(&SESSION);
	let mut c = with_messages(&h).await;
	let listed = c.list_sessions().await.unwrap();
	for i in 0..20 {
		h.engine.messages.deliver(&SESSION, &format!("m{i}"));
	}
	let mut watch = c.watch_session(None, Some(listed.revision)).await.unwrap();
	assert!(matches!(
		watch.next().await.unwrap(),
		Some(SessionItem::Resync(_))
	));
	// A fresh watch plus a transcript read recovers everything.
	let mut fresh = c.watch_session(None, None).await.unwrap();
	assert!(matches!(
		fresh.next().await.unwrap(),
		Some(SessionItem::List(_))
	));
	assert_eq!(c.read_transcript(&SESSION, 0).await.unwrap().len(), 20);
}

#[test]
fn debug_output_never_contains_message_text() {
	let request = relay_api::pb::SendSessionMessageRequest {
		text: "super secret words".into(),
		..Default::default()
	};
	assert!(!format!("{request:?}").contains("secret"));
	let entry = relay_api::pb::TranscriptEntry {
		body: Some(relay_api::pb::transcript_entry::Body::Text(
			"private".into(),
		)),
		..Default::default()
	};
	assert!(!format!("{entry:?}").contains("private"));
}
