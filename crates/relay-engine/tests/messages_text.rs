#![cfg(unix)]
//! Text between two real daemons: both directions, acks, ordering, idempotency, pins,
//! search and the event stream.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;
mod support_messages;

use relay_api::pb::{DeliveryState, session_event};
use relay_api::{SessionItem, entry_text};
use service_support::mctx;
use support_channels::settle;
use support_messages::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn text_round_trip_in_both_directions_with_acks() {
	let mut chat = chatting().await;
	let sent = chat
		.ma
		.send_session_message(&chat.sa, "Hello from A")
		.await
		.expect("send");
	assert_ok(&sent);
	let reference = sent.message.expect("message ref");
	assert!(reference.outgoing);
	assert_eq!(reference.seq, 1);

	// B stores it as incoming; A's copy becomes delivered once B acked.
	let on_b = wait_transcript(&mut chat.mb, &chat.sb, |e| e.len() == 1).await;
	assert_eq!(entry_text(&on_b[0]), Some("Hello from A"));
	assert!(!on_b[0].outgoing);
	assert_eq!(on_b[0].seq, 1);
	let on_a = wait_transcript(&mut chat.ma, &chat.sa, |e| {
		e.len() == 1 && e[0].delivery == DeliveryState::Delivered as i32
	})
	.await;
	assert!(on_a[0].outgoing);

	let back = chat
		.mb
		.send_session_message(&chat.sb, "Reply from B\nsecond line")
		.await
		.expect("reply");
	assert_ok(&back);
	let on_a = wait_transcript(&mut chat.ma, &chat.sa, |e| e.len() == 2).await;
	// Arrival order, exact text preserved.
	assert_eq!(texts(&on_a), ["Hello from A", "Reply from B\nsecond line"]);
	assert_eq!(
		on_a.iter().map(|e| e.ordinal).collect::<Vec<_>>(),
		[1, 2],
		"ordinals count arrivals"
	);
	wait_transcript(&mut chat.mb, &chat.sb, |e| {
		e.len() == 2
			&& e.iter()
				.all(|x| x.delivery == DeliveryState::Delivered as i32)
	})
	.await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn invalid_text_is_refused_locally_and_repeats_are_idempotent() {
	let mut chat = chatting().await;
	let bad = chat
		.ma
		.send_session_message(&chat.sa, "bad \u{202E} text")
		.await
		.expect("send");
	assert_refused(&bad, "TEXT_INVALID");
	let long = chat
		.ma
		.send_session_message(&chat.sa, &"x".repeat(5000))
		.await
		.expect("send");
	assert_refused(&long, "TEXT_TOO_LONG");
	let unknown = chat
		.ma
		.send_session_message(&[9; 16], "x")
		.await
		.expect("send");
	assert_refused(&unknown, "SESSION_NOT_FOUND");

	// The same operation id with the same request answers the stored outcome.
	let request = relay_api::pb::SendSessionMessageRequest {
		context: Some(mctx(chat.ma.context(), 77)),
		session_id: chat.sa.clone(),
		text: "once".to_owned(),
	};
	let first = chat
		.ma
		.raw()
		.send_session_message(request.clone())
		.await
		.expect("first")
		.into_inner();
	let again = chat
		.ma
		.raw()
		.send_session_message(request.clone())
		.await
		.expect("again")
		.into_inner();
	assert_eq!(first.message, again.message);
	// Reusing the id for different text is an error.
	let other = relay_api::pb::SendSessionMessageRequest {
		text: "different".to_owned(),
		..request
	};
	let err = chat.ma.raw().send_session_message(other).await.unwrap_err();
	assert_eq!(err.code(), tonic::Code::AlreadyExists);
	let on_b = wait_transcript(&mut chat.mb, &chat.sb, |e| !e.is_empty()).await;
	settle(300).await;
	assert_eq!(texts(&on_b), ["once"], "stored exactly once");
	assert_eq!(texts(&transcript(&mut chat.mb, &chat.sb).await), ["once"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn pins_search_and_pages() {
	let mut chat = chatting().await;
	for i in 0..6 {
		let text = if i == 3 {
			"needle in a haystack".to_owned()
		} else {
			format!("filler {i}")
		};
		assert_ok(
			&chat
				.ma
				.send_session_message(&chat.sa, &text)
				.await
				.expect("send"),
		);
		settle(120).await;
	}
	wait_transcript(&mut chat.mb, &chat.sb, |e| e.len() == 6).await;
	let hits = chat
		.mb
		.search_session(&chat.sb, "NEEDLE")
		.await
		.expect("search");
	assert_eq!(hits.hits.len(), 1);
	assert_eq!(hits.hits[0].seq, 4);
	assert!(!hits.hits[0].outgoing);
	assert_ok(
		&chat
			.mb
			.pin_message(&chat.sb, false, 4, true)
			.await
			.expect("pin"),
	);
	let entries = transcript(&mut chat.mb, &chat.sb).await;
	assert!(entries.iter().find(|e| e.seq == 4).unwrap().pinned);
	assert_eq!(entries.iter().filter(|e| e.pinned).count(), 1);
	// Pins are local: A's copy is unpinned.
	assert!(
		transcript(&mut chat.ma, &chat.sa)
			.await
			.iter()
			.all(|e| !e.pinned)
	);
	// Paging follows `more`.
	let first = chat.mb.get_transcript(&chat.sb, 0, 4).await.expect("page");
	assert_eq!(first.entries.len(), 4);
	assert!(first.more);
	let rest = chat
		.mb
		.get_transcript(&chat.sb, first.next_ordinal, 4)
		.await
		.expect("page");
	assert_eq!(rest.entries.len(), 2);
	assert!(!rest.more);
	assert_eq!(rest.entries[0].ordinal, first.next_ordinal);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn watchers_see_messages_deliveries_and_stay_gapless() {
	let mut chat = chatting().await;
	let listed = chat.ma.list_sessions().await.expect("list");
	let mut watch = chat
		.ma
		.watch_session(None, Some(listed.revision))
		.await
		.expect("watch");
	assert_ok(
		&chat
			.ma
			.send_session_message(&chat.sa, "ping")
			.await
			.expect("send"),
	);
	assert_ok(
		&chat
			.mb
			.send_session_message(&chat.sb, "pong")
			.await
			.expect("send"),
	);
	let mut kinds = Vec::new();
	let mut last = listed.revision;
	while kinds.len() < 3 {
		let item = tokio::time::timeout(std::time::Duration::from_secs(5), watch.next())
			.await
			.expect("event in time")
			.expect("stream")
			.expect("open");
		let SessionItem::Event { revision, event } = item else {
			panic!("only events after a resume");
		};
		assert_eq!(revision, last + 1, "revisions are contiguous");
		last = revision;
		kinds.push(match event.event.expect("event") {
			session_event::Event::Entry(e) => {
				format!("entry:{}", entry_text(&e.entry.unwrap()).unwrap())
			}
			session_event::Event::Delivery(d) => {
				assert_eq!(d.delivery, DeliveryState::Delivered as i32);
				"delivery".to_owned()
			}
			other => panic!("unexpected {other:?}"),
		});
	}
	kinds.sort();
	assert_eq!(kinds, ["delivery", "entry:ping", "entry:pong"]);
}
