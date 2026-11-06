#![cfg(unix)]
//! Transcripts after the call: retention, replacement, eviction, clearing, export, and the
//! guarantee that nothing is written to disk by itself.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;
mod support_messages;

use relay_api::pb::{OfferState, SessionState, session_event};
use relay_api::{SessionItem, entry_text};
use relay_messages::store::{Export, ExportContent};
use service_support::*;
use support_channels::{pair, settle, side};
use support_messages::*;

async fn hang_up(side: &mut support_channels::Side, call: &[u8], op: u8) {
	let generation = controller_of(&call_in(&mut side.cli, call).await.expect("call")).generation;
	let end = mutation(&side.cli, op, call, generation);
	assert_applied(
		&side
			.cli
			.raw()
			.end_call(end)
			.await
			.expect("end")
			.into_inner(),
	);
	wait_state(&mut side.cli, call, relay_api::pb::CallState::Ended).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn an_ended_call_leaves_a_read_only_transcript() {
	let mut chat = chatting().await;
	assert_ok(
		&chat
			.ma
			.send_session_message(&chat.sa, "before the end")
			.await
			.unwrap(),
	);
	wait_transcript(&mut chat.mb, &chat.sb, |e| e.len() == 1).await;
	let src = tempfile::tempdir().unwrap();
	let path = src.path().join("late.bin");
	std::fs::write(&path, b"1234").unwrap();
	let open_offer = offer(&mut chat, &path).await;
	arrived(&mut chat, &open_offer).await;
	let mut watch = chat.mb.watch_session(None, None).await.unwrap();
	assert!(matches!(
		watch.next().await.unwrap(),
		Some(SessionItem::List(_))
	));

	let call = chat.call.clone();
	hang_up(&mut chat.a, &call, 90).await;
	// The session ends on both sides; the transcript stays.
	let ended = eventually!(
		chat.mb
			.list_sessions()
			.await
			.unwrap()
			.sessions
			.into_iter()
			.find(|s| s.state == SessionState::Ended as i32)
	);
	assert!(ended.ended_at_unix_ms > 0);
	assert_eq!(ended.message_count, 2);
	let entries = transcript(&mut chat.mb, &chat.sb).await;
	assert_eq!(texts(&entries), ["before the end"]);
	// The open offer can no longer be downloaded: metadata only.
	assert_eq!(
		offer_now(&mut chat.mb, &chat.sb, &open_offer).await.state,
		OfferState::Expired as i32
	);
	let late = chat
		.mb
		.respond_offer(
			&chat.sb,
			&open_offer,
			true,
			src.path().to_str().unwrap(),
			"",
		)
		.await
		.unwrap();
	assert_refused(&late, "SESSION_ENDED");
	assert_refused(
		&chat
			.mb
			.send_session_message(&chat.sb, "too late")
			.await
			.unwrap(),
		"SESSION_ENDED",
	);
	assert_refused(
		&chat
			.ma
			.send_session_message(&chat.sa, "too late")
			.await
			.unwrap(),
		"SESSION_ENDED",
	);
	// Reading, searching and pinning still work on an ended transcript.
	assert_eq!(
		chat.mb
			.search_session(&chat.sb, "BEFORE")
			.await
			.unwrap()
			.hits
			.len(),
		1
	);
	assert_ok(&chat.mb.pin_message(&chat.sb, false, 1, true).await.unwrap());
	// Watchers were told the session ended.
	let mut saw_end = false;
	while let Ok(Ok(Some(SessionItem::Event { event, .. }))) =
		tokio::time::timeout(std::time::Duration::from_millis(500), watch.next()).await
	{
		if let Some(session_event::Event::Session(s)) = event.event
			&& s.state == SessionState::Ended as i32
		{
			saw_end = true;
		}
	}
	assert!(saw_end);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_new_call_with_the_same_peer_replaces_the_old_transcript() {
	let mut chat = chatting().await;
	assert_ok(
		&chat
			.ma
			.send_session_message(&chat.sa, "first call")
			.await
			.unwrap(),
	);
	wait_transcript(&mut chat.mb, &chat.sb, |e| e.len() == 1).await;
	let call = chat.call.clone();
	hang_up(&mut chat.a, &call, 90).await;
	eventually!(
		chat.ma
			.list_sessions()
			.await
			.unwrap()
			.sessions
			.iter()
			.any(|s| s.state == SessionState::Ended as i32)
			.then_some(())
	);
	let mut watch = chat.ma.watch_session(None, None).await.unwrap();
	assert!(matches!(
		watch.next().await.unwrap(),
		Some(SessionItem::List(_))
	));
	let second = connect(&mut chat.a, &mut chat.b, 100).await;
	assert_ne!(second, chat.call);
	let sessions = eventually!(
		Some(chat.ma.list_sessions().await.unwrap().sessions)
			.filter(|s| { s.len() == 1 && s[0].state == SessionState::Active as i32 })
	);
	assert_ne!(
		sessions[0].session_id, chat.sa,
		"the old transcript is gone"
	);
	let mut evicted = false;
	while let Ok(Ok(Some(SessionItem::Event { event, .. }))) =
		tokio::time::timeout(std::time::Duration::from_millis(500), watch.next()).await
	{
		if let Some(session_event::Event::Evicted(e)) = event.event {
			assert_eq!(e.session_id, chat.sa);
			evicted = true;
		}
	}
	assert!(evicted, "watchers hear about the dropped transcript");
	assert_eq!(
		code_of(&chat.ma.get_transcript(&chat.sa, 0, 0).await.unwrap_err()),
		tonic::Code::NotFound
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_retained_budget_evicts_the_oldest_ended_transcript_first() {
	// Two ended transcripts of two messages each do not fit in 2.5 KB.
	let mut a = support_channels::side_tuned(440.0, None, true, |o| {
		o.messaging.limits = Some(relay_engine::messaging::MessagingLimits {
			retained_bytes: 2_500,
			..Default::default()
		});
	})
	.await;
	let mut b = side(1_700.0).await;
	let mut c = side(3_100.0).await;
	pair(&mut a, &mut b, 1).await;
	pair(&mut a, &mut c, 10).await;
	let mut ma = messenger(&a).await;
	let mut watch = ma.watch_session(None, None).await.unwrap();
	assert!(matches!(
		watch.next().await.unwrap(),
		Some(SessionItem::List(_))
	));

	let mut ended = Vec::new();
	for (peer, op) in [(&mut b, 30u8), (&mut c, 40u8)] {
		let call = connect(&mut a, peer, op).await;
		let session = eventually!(
			ma.list_sessions()
				.await
				.unwrap()
				.sessions
				.into_iter()
				.find(|s| s.state == SessionState::Active as i32)
		)
		.session_id;
		for n in 0..2 {
			assert_ok(
				&ma.send_session_message(&session, &format!("msg {n}"))
					.await
					.unwrap(),
			);
		}
		hang_up(&mut a, &call, op + 5).await;
		eventually!(
			ma.list_sessions()
				.await
				.unwrap()
				.sessions
				.iter()
				.all(|s| s.state == SessionState::Ended as i32)
				.then_some(())
		);
		ended.push(session);
		settle(200).await;
	}
	let left = ma.list_sessions().await.unwrap().sessions;
	assert_eq!(left.len(), 1, "the older transcript was evicted");
	assert_eq!(left[0].session_id, ended[1]);
	assert_eq!(
		code_of(&ma.get_transcript(&ended[0], 0, 0).await.unwrap_err()),
		tonic::Code::NotFound
	);
	let mut told = false;
	while let Ok(Ok(Some(SessionItem::Event { event, .. }))) =
		tokio::time::timeout(std::time::Duration::from_millis(500), watch.next()).await
	{
		if let Some(session_event::Event::Evicted(e)) = event.event
			&& e.session_id == ended[0]
		{
			told = true;
		}
	}
	assert!(told);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn clearing_is_local_and_ordinals_keep_counting() {
	let mut chat = chatting().await;
	for n in 0..3 {
		assert_ok(
			&chat
				.ma
				.send_session_message(&chat.sa, &format!("m{n}"))
				.await
				.unwrap(),
		);
		settle(100).await;
	}
	wait_transcript(&mut chat.mb, &chat.sb, |e| e.len() == 3).await;
	assert_ok(&chat.mb.clear_session(&chat.sb).await.unwrap());
	assert!(transcript(&mut chat.mb, &chat.sb).await.is_empty());
	assert_eq!(
		transcript(&mut chat.ma, &chat.sa).await.len(),
		3,
		"the peer keeps its own"
	);
	assert_ok(
		&chat
			.ma
			.send_session_message(&chat.sa, "after clear")
			.await
			.unwrap(),
	);
	let after = wait_transcript(&mut chat.mb, &chat.sb, |e| e.len() == 1).await;
	assert_eq!(after[0].ordinal, 4, "ordinals are never reused");
	assert_eq!(entry_text(&after[0]), Some("after clear"));
	let summary = chat.mb.list_sessions().await.unwrap().sessions.remove(0);
	assert_eq!(summary.next_ordinal, 5);
	// Clearing an ended session removes it.
	let call = chat.call.clone();
	hang_up(&mut chat.a, &call, 90).await;
	eventually!(
		chat.mb
			.list_sessions()
			.await
			.unwrap()
			.sessions
			.iter()
			.any(|s| s.state == SessionState::Ended as i32)
			.then_some(())
	);
	assert_ok(&chat.mb.clear_session(&chat.sb).await.unwrap());
	assert!(chat.mb.list_sessions().await.unwrap().sessions.is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn export_returns_json_and_writes_only_where_told() {
	let mut chat = chatting().await;
	assert_ok(
		&chat
			.ma
			.send_session_message(&chat.sa, "Plan:\n```rust\nfn main() {}\n```")
			.await
			.unwrap(),
	);
	assert_ok(
		&chat
			.ma
			.send_session_image(&chat.sa, support_daemon::png(4, 4))
			.await
			.unwrap(),
	);
	wait_transcript(&mut chat.mb, &chat.sb, |e| e.len() == 2).await;
	assert_ok(&chat.mb.pin_message(&chat.sb, false, 1, true).await.unwrap());

	let home_before = listing(chat.b._home.path());
	let reply = chat.mb.export_session(&chat.sb, false, None).await.unwrap();
	assert_ok(&relay_api::pb::SessionOpResult {
		operation: reply.operation.clone(),
		..Default::default()
	});
	assert!(reply.written_path.is_empty());
	assert_eq!(
		listing(chat.b._home.path()),
		home_before,
		"nothing was written"
	);
	let export = Export::from_json(std::str::from_utf8(&reply.json).unwrap()).unwrap();
	assert_eq!(export.messages.len(), 2);
	assert_eq!(export.peer.display_name, "node-440");
	assert!(export.messages[0].pinned);
	assert_eq!(
		export.messages[0].direction,
		relay_messages::Direction::Incoming
	);
	match &export.messages[0].content {
		ExportContent::Text { text, code_blocks } => {
			assert!(text.starts_with("Plan:"));
			assert_eq!(code_blocks[0].language.as_deref(), Some("rust"));
			assert_eq!(code_blocks[0].text, "fn main() {}");
		}
		other => panic!("{other:?}"),
	}
	assert!(matches!(
		&export.messages[1].content,
		ExportContent::Image {
			data_base64: None,
			..
		}
	));
	let embedded = chat.mb.export_session(&chat.sb, true, None).await.unwrap();
	let export = Export::from_json(std::str::from_utf8(&embedded.json).unwrap()).unwrap();
	assert!(Export::decode_image(&export.messages[1].content).is_some());

	// Only an explicit destination makes the daemon write, owner-only, never over a file.
	let out = tempfile::tempdir().unwrap();
	let target = out.path().join("chat.json");
	let written = chat
		.mb
		.export_session(&chat.sb, false, Some(target.to_str().unwrap()))
		.await
		.unwrap();
	assert_eq!(written.written_path, target.to_str().unwrap());
	assert!(written.json.is_empty());
	use std::os::unix::fs::PermissionsExt;
	assert_eq!(
		std::fs::metadata(&target).unwrap().permissions().mode() & 0o777,
		0o600
	);
	let on_disk = Export::from_json(&std::fs::read_to_string(&target).unwrap()).unwrap();
	let first = Export::from_json(std::str::from_utf8(&reply.json).unwrap()).unwrap();
	assert_eq!(
		format!("{:?}", on_disk.messages),
		format!("{:?}", first.messages)
	);
	let again = chat
		.mb
		.export_session(&chat.sb, false, Some(target.to_str().unwrap()))
		.await
		.unwrap();
	assert_eq!(
		again.operation.unwrap().problem.unwrap().code,
		"EXPORT_WRITE_FAILED"
	);
	let relative = chat
		.mb
		.export_session(&chat.sb, false, Some("chat.json"))
		.await
		.unwrap();
	assert_eq!(
		relative.operation.unwrap().problem.unwrap().code,
		"EXPORT_WRITE_FAILED"
	);
	let still = Export::from_json(&std::fs::read_to_string(&target).unwrap()).unwrap();
	assert_eq!(still.exported_at_ms, on_disk.exported_at_ms, "untouched");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn transcripts_never_touch_the_disk() {
	let mut chat = chatting().await;
	const MARK: &str = "zx9-disk-marker-ibis";
	assert_ok(&chat.ma.send_session_message(&chat.sa, MARK).await.unwrap());
	assert_ok(&chat.mb.send_session_message(&chat.sb, MARK).await.unwrap());
	wait_transcript(&mut chat.mb, &chat.sb, |e| e.len() == 2).await;
	let homes = [
		chat.a._home.path().to_owned(),
		chat.b._home.path().to_owned(),
	];
	let call = chat.call.clone();
	hang_up(&mut chat.a, &call, 90).await;
	let (a, b) = (chat.a, chat.b);
	a.daemon.shutdown().await.unwrap();
	b.daemon.shutdown().await.unwrap();
	for home in homes {
		let mut stack = vec![home];
		while let Some(dir) = stack.pop() {
			for entry in std::fs::read_dir(&dir).unwrap().flatten() {
				let path = entry.path();
				if path.is_dir() {
					stack.push(path);
				} else if let Ok(bytes) = std::fs::read(&path) {
					assert!(
						!bytes.windows(MARK.len()).any(|w| w == MARK.as_bytes()),
						"message text found in {}",
						path.display()
					);
				}
			}
		}
	}
}
