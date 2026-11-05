#![cfg(unix)]
//! Transfer limits and failure paths: concurrency, a full or failing disk, permission
//! changes mid-transfer and the text preview.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;
mod support_messages;

use std::sync::Arc;

use relay_api::pb::OfferState;
use relay_engine::messaging::SinkFactory;
use relay_messages::SafeFileName;
use relay_messages::transfer::{FileSink, SinkError};
use support_channels::settle;
use support_messages::*;

/// A sink that pretends the disk is small or fails after some bytes.
struct FakeDisk {
	free: u64,
	fail_after: Option<usize>,
	written: usize,
}

impl FileSink for FakeDisk {
	fn free_space(&mut self) -> Result<u64, SinkError> {
		Ok(self.free)
	}
	fn create_temp(&mut self, _: &SafeFileName, _: u64) -> Result<(), SinkError> {
		Ok(())
	}
	fn write(&mut self, data: &[u8]) -> Result<(), SinkError> {
		self.written += data.len();
		match self.fail_after {
			Some(limit) if self.written > limit => Err(SinkError("disk full".to_owned())),
			_ => Ok(()),
		}
	}
	fn sync(&mut self) -> Result<(), SinkError> {
		Ok(())
	}
	fn commit(&mut self) -> Result<String, SinkError> {
		Ok("never-visible".to_owned())
	}
	fn abort(&mut self) {}
}

fn disk(free: u64, fail_after: Option<usize>) -> SinkFactory {
	Arc::new(move |_| {
		Ok(Box::new(FakeDisk {
			free,
			fail_after,
			written: 0,
		}))
	})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_full_disk_refuses_the_accept_and_the_offer_stays_open() {
	let mut chat = chatting_tuned(|_| {}, |o| o.messaging.sinks = disk(1024 * 1024, None)).await;
	let src = tempfile::tempdir().unwrap();
	let path = src.path().join("f.bin");
	std::fs::write(&path, bytes(2 * 1024 * 1024, 4)).unwrap();
	let id = offer(&mut chat, &path).await;
	arrived(&mut chat, &id).await;
	let refused = chat
		.mb
		.respond_offer(&chat.sb, &id, true, src.path().to_str().unwrap(), "")
		.await
		.unwrap();
	assert_refused(&refused, "NO_SPACE");
	assert_eq!(
		offer_now(&mut chat.mb, &chat.sb, &id).await.state,
		OfferState::Offered as i32
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_disk_that_fails_mid_transfer_fails_both_sides_without_a_file() {
	let mut chat = chatting_tuned(
		|_| {},
		|o| o.messaging.sinks = disk(u64::MAX / 2, Some(256 * 1024)),
	)
	.await;
	let src = tempfile::tempdir().unwrap();
	let path = src.path().join("f.bin");
	std::fs::write(&path, bytes(2 * 1024 * 1024, 4)).unwrap();
	let id = offer(&mut chat, &path).await;
	arrived(&mut chat, &id).await;
	assert_ok(
		&chat
			.mb
			.respond_offer(&chat.sb, &id, true, src.path().to_str().unwrap(), "")
			.await
			.unwrap(),
	);
	let on_b = wait_offer(&mut chat.mb, &chat.sb, &id, OfferState::Failed).await;
	assert_eq!(on_b.failure, "destination_error");
	wait_offer(&mut chat.ma, &chat.sa, &id, OfferState::Failed).await;
	assert_eq!(listing(src.path()), ["f.bin"]);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_two_transfers_run_at_once_per_session() {
	let mut chat = chatting().await;
	assert_ok(
		&chat
			.ma
			.set_messaging_settings(None, None, Some(1))
			.await
			.unwrap(),
	);
	let src = tempfile::tempdir().unwrap();
	let dst = tempfile::tempdir().unwrap();
	let mut ids = Vec::new();
	for n in 0..3 {
		let path = src.path().join(format!("f{n}.bin"));
		std::fs::write(&path, bytes(6 * 1024 * 1024, n)).unwrap();
		let id = offer(&mut chat, &path).await;
		arrived(&mut chat, &id).await;
		ids.push(id);
		settle(600).await; // offers are limited to 2 per second
	}
	for id in &ids[..2] {
		assert_ok(
			&chat
				.mb
				.respond_offer(&chat.sb, id, true, dst.path().to_str().unwrap(), "")
				.await
				.unwrap(),
		);
	}
	let third = chat
		.mb
		.respond_offer(&chat.sb, &ids[2], true, dst.path().to_str().unwrap(), "")
		.await
		.unwrap();
	assert_refused(&third, "TOO_MANY_TRANSFERS");
	assert_eq!(
		offer_now(&mut chat.mb, &chat.sb, &ids[2]).await.state,
		OfferState::Offered as i32,
		"the refused offer stays open"
	);
	// Finishing frees a slot: lift the cap and let the two complete.
	assert_ok(
		&chat
			.ma
			.set_messaging_settings(None, None, Some(0))
			.await
			.unwrap(),
	);
	for id in &ids[..2] {
		wait_offer(&mut chat.mb, &chat.sb, id, OfferState::Completed).await;
	}
	assert_ok(
		&chat
			.mb
			.respond_offer(&chat.sb, &ids[2], true, dst.path().to_str().unwrap(), "")
			.await
			.unwrap(),
	);
	wait_offer(&mut chat.mb, &chat.sb, &ids[2], OfferState::Completed).await;
	assert_eq!(listing(dst.path()).len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn turning_files_off_cancels_running_transfers_and_refuses_new_accepts() {
	let mut chat = chatting().await;
	assert_ok(
		&chat
			.ma
			.set_messaging_settings(None, None, Some(1))
			.await
			.unwrap(),
	);
	let src = tempfile::tempdir().unwrap();
	let dst = tempfile::tempdir().unwrap();
	let path = src.path().join("f.bin");
	std::fs::write(&path, bytes(6 * 1024 * 1024, 8)).unwrap();
	let running = offer(&mut chat, &path).await;
	arrived(&mut chat, &running).await;
	assert_ok(
		&chat
			.mb
			.respond_offer(&chat.sb, &running, true, dst.path().to_str().unwrap(), "")
			.await
			.unwrap(),
	);
	settle(400).await;
	settle(600).await;
	let waiting = offer(&mut chat, &path).await;
	arrived(&mut chat, &waiting).await;
	// The host turns files off; the guest follows the push.
	assert_ok(
		&chat
			.ma
			.set_session_permissions(&chat.sa, None, Some(false))
			.await
			.unwrap(),
	);
	wait_offer(&mut chat.ma, &chat.sa, &running, OfferState::Cancelled).await;
	wait_offer(&mut chat.mb, &chat.sb, &running, OfferState::Cancelled).await;
	let refused = chat
		.mb
		.respond_offer(&chat.sb, &waiting, true, dst.path().to_str().unwrap(), "")
		.await
		.unwrap();
	assert_refused(&refused, "SESSION_PERMISSION_OFF");
	let sent = chat
		.ma
		.offer_file(&chat.sa, path.to_str().unwrap())
		.await
		.unwrap();
	assert_refused(&sent, "SESSION_PERMISSION_OFF");
	assert!(listing(dst.path()).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_preview_needs_the_senders_approval_and_is_only_text() {
	let mut chat = chatting().await;
	let src = tempfile::tempdir().unwrap();
	let path = src.path().join("notes.txt");
	let lines: Vec<String> = (0..150).map(|i| format!("line {i}")).collect();
	std::fs::write(&path, lines.join("\n")).unwrap();
	let id = offer(&mut chat, &path).await;
	arrived(&mut chat, &id).await;
	assert_ok(&chat.mb.request_preview(&chat.sb, &id).await.unwrap());
	assert!(
		offer_now(&mut chat.mb, &chat.sb, &id)
			.await
			.preview_requested
	);
	// Nothing arrives until the sender's user approves.
	for _ in 0..500 {
		if offer_now(&mut chat.ma, &chat.sa, &id)
			.await
			.preview_approval_pending
		{
			break;
		}
		settle(10).await;
	}
	settle(200).await;
	assert!(
		offer_now(&mut chat.mb, &chat.sb, &id)
			.await
			.preview
			.is_empty()
	);
	assert_ok(&chat.ma.approve_preview(&chat.sa, &id, true).await.unwrap());
	for _ in 0..500 {
		if !offer_now(&mut chat.mb, &chat.sb, &id)
			.await
			.preview
			.is_empty()
		{
			break;
		}
		settle(10).await;
	}
	let shown = offer_now(&mut chat.mb, &chat.sb, &id).await;
	assert_eq!(shown.preview.lines().count(), 100, "at most 100 lines");
	assert!(shown.preview.starts_with("line 0\n"));
	assert!(!shown.preview_requested);

	// A refusal carries no text; a binary file has no preview at all.
	let bin = src.path().join("data.bin");
	std::fs::write(&bin, [0u8, 159, 146, 150, 0, 1, 2]).unwrap();
	let other = offer(&mut chat, &bin).await;
	arrived(&mut chat, &other).await;
	assert_ok(&chat.mb.request_preview(&chat.sb, &other).await.unwrap());
	for _ in 0..500 {
		if offer_now(&mut chat.ma, &chat.sa, &other)
			.await
			.preview_approval_pending
		{
			break;
		}
		settle(10).await;
	}
	let none = chat
		.ma
		.approve_preview(&chat.sa, &other, true)
		.await
		.unwrap();
	assert_refused(&none, "PREVIEW_UNAVAILABLE");
	settle(200).await;
	let seen = offer_now(&mut chat.mb, &chat.sb, &other).await;
	assert!(seen.preview.is_empty() && !seen.preview_requested);
}
