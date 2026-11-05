#![cfg(unix)]
//! Permissions are enforced on BOTH sides, the capability gates every call, and images and
//! budgets are bounded.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;
mod support_messages;

use relay_api::pb::{DeliveryState, OfferState, SessionState, session_event};
use relay_api::{RelayClient, SessionItem, entry_text};
use service_support::code_of;
use support_channels::{settle, side};
use support_daemon::png;
use support_messages::*;

fn failure_of(entries: &[relay_api::pb::TranscriptEntry], seq: u64) -> (i32, String) {
	let e = entries
		.iter()
		.find(|e| e.outgoing && e.seq == seq)
		.expect("entry");
	(e.delivery, e.failure.clone())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_receiver_enforces_permissions_even_when_the_sender_does_not() {
	let mut chat = chatting().await;
	// The guest turns text off for itself; the host's side still allows it.
	assert_ok(
		&chat
			.mb
			.set_session_permissions(&chat.sb, Some(false), None)
			.await
			.unwrap(),
	);
	let sent = chat
		.ma
		.send_session_message(&chat.sa, "ignored by B")
		.await
		.unwrap();
	assert_ok(&sent);
	let on_a = wait_transcript(&mut chat.ma, &chat.sa, |e| {
		e.len() == 1 && e[0].delivery == DeliveryState::Failed as i32
	})
	.await;
	assert_eq!(failure_of(&on_a, 1).1, "disabled");
	assert!(
		transcript(&mut chat.mb, &chat.sb).await.is_empty(),
		"B stored nothing"
	);
	// The guest cannot send text either.
	let own = chat
		.mb
		.send_session_message(&chat.sb, "nope")
		.await
		.unwrap();
	assert_refused(&own, "SESSION_PERMISSION_OFF");

	// Images and files are a separate switch and are refused the same way.
	assert_ok(
		&chat
			.mb
			.set_session_permissions(&chat.sb, Some(true), Some(false))
			.await
			.unwrap(),
	);
	assert_ok(
		&chat
			.ma
			.send_session_image(&chat.sa, png(8, 8))
			.await
			.unwrap(),
	);
	let src = tempfile::tempdir().unwrap();
	let path = src.path().join("f.txt");
	std::fs::write(&path, b"hello").unwrap();
	let id = offer(&mut chat, &path).await;
	let failed = wait_offer(&mut chat.ma, &chat.sa, &id, OfferState::Failed).await;
	assert_eq!(failed.state, OfferState::Failed as i32);
	assert!(transcript(&mut chat.mb, &chat.sb).await.is_empty());

	// Text works again once B allows it.
	assert_ok(
		&chat
			.mb
			.set_session_permissions(&chat.sb, Some(true), None)
			.await
			.unwrap(),
	);
	assert_ok(
		&chat
			.ma
			.send_session_message(&chat.sa, "now fine")
			.await
			.unwrap(),
	);
	wait_transcript(&mut chat.mb, &chat.sb, |e| texts(e) == ["now fine"]).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_host_pushes_permissions_and_only_the_host() {
	let mut chat = chatting().await;
	let mut watch = chat.mb.watch_session(None, None).await.unwrap();
	assert!(matches!(
		watch.next().await.unwrap(),
		Some(SessionItem::List(_))
	));
	assert_ok(
		&chat
			.ma
			.set_session_permissions(&chat.sa, Some(false), Some(false))
			.await
			.unwrap(),
	);
	let changed = loop {
		let Some(SessionItem::Event { event, .. }) = watch.next().await.unwrap() else {
			panic!("event");
		};
		if let Some(session_event::Event::Permissions(p)) = event.event {
			break p;
		}
	};
	assert!(changed.by_peer);
	let perms = changed.permissions.unwrap();
	assert!(!perms.allow_messages && !perms.allow_files);
	let summary = chat.mb.list_sessions().await.unwrap().sessions.remove(0);
	assert!(!summary.local_is_host);
	assert!(!summary.permissions.unwrap().allow_messages);
	// A change by the guest stays local: the host's permissions did not move.
	assert_ok(
		&chat
			.mb
			.set_session_permissions(&chat.sb, Some(true), Some(true))
			.await
			.unwrap(),
	);
	settle(300).await;
	let host = chat.ma.list_sessions().await.unwrap().sessions.remove(0);
	assert!(host.local_is_host);
	assert!(!host.permissions.unwrap().allow_messages);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_master_switch_stops_sending_and_receiving() {
	let mut chat = chatting().await;
	assert_ok(
		&chat
			.mb
			.set_messaging_settings(Some(false), None, None)
			.await
			.unwrap(),
	);
	assert_ok(
		&chat
			.ma
			.send_session_message(&chat.sa, "to a closed door")
			.await
			.unwrap(),
	);
	let on_a = wait_transcript(&mut chat.ma, &chat.sa, |e| {
		e.len() == 1 && e[0].delivery == DeliveryState::Failed as i32
	})
	.await;
	assert_eq!(failure_of(&on_a, 1).1, "disabled");
	let own = chat.mb.send_session_message(&chat.sb, "x").await.unwrap();
	assert_refused(&own, "MESSAGES_DISABLED");
	let settings = chat.mb.list_sessions().await.unwrap().settings.unwrap();
	assert!(!settings.enabled);
	// Existing transcripts stay readable while it is off.
	assert!(transcript(&mut chat.mb, &chat.sb).await.is_empty());
	assert_ok(
		&chat
			.mb
			.set_messaging_settings(Some(true), None, None)
			.await
			.unwrap(),
	);
	assert_ok(
		&chat
			.mb
			.send_session_message(&chat.sb, "open again")
			.await
			.unwrap(),
	);
	wait_transcript(&mut chat.ma, &chat.sa, |e| e.len() == 2).await;
	// Bad values are refused with a stable code.
	let bad = chat
		.mb
		.set_messaging_settings(None, Some(0), None)
		.await
		.unwrap();
	assert_refused(&bad, "INVALID_SETTING");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_client_without_the_messages_capability_gets_nothing() {
	let mut chat = chatting().await;
	assert_ok(
		&chat
			.ma
			.send_session_message(&chat.sa, "private words")
			.await
			.unwrap(),
	);
	wait_transcript(&mut chat.mb, &chat.sb, |e| e.len() == 1).await;
	// The default capabilities, and admin, do not include messages.
	let mut plain = RelayClient::connect(chat.a.daemon.endpoint())
		.await
		.unwrap();
	plain.register("plain", "test", &[]).await.unwrap();
	let mut admin = RelayClient::connect(chat.a.daemon.endpoint())
		.await
		.unwrap();
	admin
		.register("admin", "test", &["observe", "call-control", "admin"])
		.await
		.unwrap();
	for client in [&mut plain, &mut admin] {
		assert_eq!(
			code_of(&client.list_sessions().await.unwrap_err()),
			tonic::Code::PermissionDenied
		);
		assert_eq!(
			code_of(&client.get_transcript(&chat.sa, 0, 0).await.unwrap_err()),
			tonic::Code::PermissionDenied
		);
		assert_eq!(
			code_of(
				&client
					.send_session_message(&chat.sa, "x")
					.await
					.unwrap_err()
			),
			tonic::Code::PermissionDenied
		);
		assert_eq!(
			code_of(
				&client
					.search_session(&chat.sa, "private")
					.await
					.unwrap_err()
			),
			tonic::Code::PermissionDenied
		);
		assert_eq!(
			code_of(
				&client
					.export_session(&chat.sa, false, None)
					.await
					.unwrap_err()
			),
			tonic::Code::PermissionDenied
		);
		assert_eq!(
			code_of(&client.clear_session(&chat.sa).await.unwrap_err()),
			tonic::Code::PermissionDenied
		);
		assert_eq!(
			code_of(
				&client
					.set_session_permissions(&chat.sa, Some(false), None)
					.await
					.unwrap_err()
			),
			tonic::Code::PermissionDenied
		);
		assert_eq!(
			code_of(&client.offer_file(&chat.sa, "/etc/hosts").await.unwrap_err()),
			tonic::Code::PermissionDenied
		);
		assert_eq!(
			code_of(
				&client
					.set_messaging_settings(Some(false), None, None)
					.await
					.unwrap_err()
			),
			tonic::Code::PermissionDenied
		);
		assert_eq!(
			code_of(&client.watch_session(None, None).await.err().unwrap()),
			tonic::Code::PermissionDenied
		);
	}
	// Nothing about the conversation shows up in what such a client may read.
	let snapshot = format!("{:?}", plain.snapshot().await.unwrap());
	assert!(!snapshot.contains("private words"));
	let mut state = plain
		.watch(relay_api::WatchOptions::default())
		.await
		.unwrap();
	assert_ok(
		&chat
			.ma
			.send_session_message(&chat.sa, "more private words")
			.await
			.unwrap(),
	);
	while let Ok(Ok(Some(item))) =
		tokio::time::timeout(std::time::Duration::from_millis(400), state.next()).await
	{
		assert!(!format!("{item:?}").contains("private words"));
	}
	// And the messages are all still there for the right client.
	assert_eq!(transcript(&mut chat.ma, &chat.sa).await.len(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn images_are_validated_bounded_and_stored_as_sent() {
	let mut chat = chatting().await;
	let image = png(16, 12);
	assert_ok(
		&chat
			.ma
			.send_session_image(&chat.sa, image.clone())
			.await
			.unwrap(),
	);
	let on_b = wait_transcript(&mut chat.mb, &chat.sb, |e| e.len() == 1).await;
	let Some(relay_api::pb::transcript_entry::Body::Image(got)) = &on_b[0].body else {
		panic!("image entry");
	};
	assert_eq!((got.width, got.height), (16, 12));
	assert_eq!(got.media_type, "image/png");
	assert_eq!(got.data, image, "the original bytes, never re-encoded");
	assert!(entry_text(&on_b[0]).is_none());

	let garbage = chat
		.ma
		.send_session_image(&chat.sa, vec![7; 300])
		.await
		.unwrap();
	assert_refused(&garbage, "IMAGE_UNSUPPORTED");
	let huge = chat
		.ma
		.send_session_image(&chat.sa, vec![0; 16 * 1024])
		.await
		.unwrap();
	assert_refused(&huge, "IMAGE_TOO_LARGE");
	// A header that promises a decompression bomb is refused before anything decodes it.
	let mut bomb = png(16, 12);
	bomb[16..20].copy_from_slice(&40_000u32.to_be_bytes());
	bomb[20..24].copy_from_slice(&40_000u32.to_be_bytes());
	let refused = chat.ma.send_session_image(&chat.sa, bomb).await.unwrap();
	assert_refused(&refused, "IMAGE_TOO_LARGE");
	assert_eq!(transcript(&mut chat.ma, &chat.sa).await.len(), 1);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_full_history_rejects_new_messages_and_keeps_the_old() {
	// The receiver's session budget holds two small messages (1 KiB overhead each).
	let mut chat = chatting_tuned(|_| {}, |o| o.messaging.limits.session_bytes = 2_500).await;
	for n in 0..4 {
		assert_ok(
			&chat
				.ma
				.send_session_message(&chat.sa, &format!("message {n}"))
				.await
				.unwrap(),
		);
		settle(150).await;
	}
	let on_a = wait_transcript(&mut chat.ma, &chat.sa, |e| {
		e.len() == 4
			&& e.iter()
				.all(|x| x.delivery != DeliveryState::Sending as i32)
	})
	.await;
	assert_eq!(failure_of(&on_a, 3).1, "history_full");
	assert_eq!(failure_of(&on_a, 4).1, "history_full");
	assert_eq!(on_a[0].delivery, DeliveryState::Delivered as i32);
	assert_eq!(
		texts(&transcript(&mut chat.mb, &chat.sb).await),
		["message 0", "message 1"]
	);
	// The full side also refuses to send; clearing makes room again.
	let own = chat
		.mb
		.send_session_message(&chat.sb, "reply")
		.await
		.unwrap();
	assert_refused(&own, "HISTORY_FULL");
	assert_ok(&chat.mb.clear_session(&chat.sb).await.unwrap());
	assert_ok(
		&chat
			.mb
			.send_session_message(&chat.sb, "reply")
			.await
			.unwrap(),
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn sending_faster_than_the_rate_limit_is_refused_locally() {
	let mut chat = chatting().await;
	let mut refused = 0;
	let mut accepted = 0;
	for n in 0..40 {
		let sent = chat
			.ma
			.send_session_message(&chat.sa, &format!("burst {n}"))
			.await
			.unwrap();
		if op_code(&sent) == "RATE_LIMITED" {
			refused += 1;
		} else {
			assert_ok(&sent);
			accepted += 1;
		}
	}
	assert!(
		(20..=30).contains(&accepted),
		"burst of 20 plus refill: {accepted}"
	);
	assert_eq!(refused + accepted, 40);
	wait_transcript(&mut chat.mb, &chat.sb, |e| e.len() == accepted).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn nothing_is_sent_without_a_session() {
	let a = side(440.0).await;
	let mut m = messenger(&a).await;
	assert!(m.list_sessions().await.unwrap().sessions.is_empty());
	let sent = m.send_session_message(&[1; 16], "x").await.unwrap();
	assert_refused(&sent, "SESSION_NOT_FOUND");
	assert_eq!(
		code_of(&m.get_transcript(&[1; 16], 0, 0).await.unwrap_err()),
		tonic::Code::NotFound
	);
	let _ = SessionState::Active;
	a.daemon.shutdown().await.unwrap();
}
