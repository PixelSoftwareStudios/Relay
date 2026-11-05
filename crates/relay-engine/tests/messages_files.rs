#![cfg(unix)]
//! File offers between two real daemons: the bytes travel over a per-transfer data link,
//! land under a temp name, are verified and only then appear.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;
mod support_messages;

use relay_api::pb::OfferState;
use support_channels::settle;
use support_messages::*;

fn blake3_of(data: &[u8]) -> Vec<u8> {
	blake3::hash(data).as_bytes().to_vec()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn offer_accept_transfer_verify_and_never_overwrite() {
	let mut chat = chatting().await;
	let src = tempfile::tempdir().expect("src");
	let dst = tempfile::tempdir().expect("dst");
	let data = bytes(3 * 1024 * 1024 + 123, 7);
	let path = src.path().join("report.bin");
	std::fs::write(&path, &data).expect("write");

	let id = offer(&mut chat, &path).await;
	arrived(&mut chat, &id).await;
	let seen = offer_now(&mut chat.mb, &chat.sb, &id).await;
	assert_eq!(seen.name, "report.bin");
	assert_eq!(seen.size, data.len() as u64);
	assert_eq!(
		seen.blake3,
		blake3_of(&data),
		"the offer carries the BLAKE3 of the snapshot"
	);
	assert_eq!(seen.state, OfferState::Offered as i32);
	assert!(!seen.outgoing);
	assert!(
		listing(dst.path()).is_empty(),
		"nothing lands before the user accepts"
	);

	let accepted = chat
		.mb
		.respond_offer(&chat.sb, &id, true, dst.path().to_str().unwrap(), "")
		.await
		.expect("accept");
	assert_ok(&accepted);
	let done = wait_offer(&mut chat.mb, &chat.sb, &id, OfferState::Completed).await;
	assert_eq!(done.saved_as, "report.bin");
	assert_eq!(
		std::fs::read(dst.path().join("report.bin")).expect("file"),
		data
	);
	assert_eq!(
		listing(dst.path()),
		["report.bin"],
		"no temp file is left behind"
	);
	let sent = wait_offer(&mut chat.ma, &chat.sa, &id, OfferState::Completed).await;
	assert_eq!(sent.transferred_bytes, data.len() as u64);
	assert!(sent.outgoing);

	// Offering the same file again never replaces the first copy.
	let again = offer(&mut chat, &path).await;
	arrived(&mut chat, &again).await;
	assert_ok(
		&chat
			.mb
			.respond_offer(&chat.sb, &again, true, dst.path().to_str().unwrap(), "")
			.await
			.expect("accept again"),
	);
	let second = wait_offer(&mut chat.mb, &chat.sb, &again, OfferState::Completed).await;
	assert_eq!(second.saved_as, "report (1).bin");
	assert_eq!(listing(dst.path()), ["report (1).bin", "report.bin"]);
	assert_eq!(std::fs::read(dst.path().join("report.bin")).unwrap(), data);

	// A suggested name is sanitized: only the last component survives.
	let third = offer(&mut chat, &path).await;
	arrived(&mut chat, &third).await;
	assert_ok(
		&chat
			.mb
			.respond_offer(
				&chat.sb,
				&third,
				true,
				dst.path().to_str().unwrap(),
				"../../evil/../name.bin",
			)
			.await
			.expect("accept named"),
	);
	let named = wait_offer(&mut chat.mb, &chat.sb, &third, OfferState::Completed).await;
	assert_eq!(named.saved_as, "name.bin");
	assert!(!dst.path().parent().unwrap().join("name.bin").exists());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn rejecting_an_offer_transfers_nothing() {
	let mut chat = chatting().await;
	let src = tempfile::tempdir().expect("src");
	let dst = tempfile::tempdir().expect("dst");
	let path = src.path().join("no.txt");
	std::fs::write(&path, b"unwanted").unwrap();
	let id = offer(&mut chat, &path).await;
	arrived(&mut chat, &id).await;
	assert_ok(
		&chat
			.mb
			.respond_offer(&chat.sb, &id, false, "", "")
			.await
			.expect("reject"),
	);
	wait_offer(&mut chat.mb, &chat.sb, &id, OfferState::Rejected).await;
	wait_offer(&mut chat.ma, &chat.sa, &id, OfferState::Rejected).await;
	settle(200).await;
	assert!(listing(dst.path()).is_empty());
	// An answered offer cannot be answered again.
	let late = chat
		.mb
		.respond_offer(&chat.sb, &id, true, dst.path().to_str().unwrap(), "")
		.await
		.expect("late");
	assert_refused(&late, "OFFER_STATE");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_receiver_can_cancel_a_running_transfer() {
	let mut chat = chatting().await;
	// 1 MB/s makes an 8 MiB transfer last long enough to cancel.
	assert_ok(
		&chat
			.ma
			.set_messaging_settings(None, None, Some(1))
			.await
			.expect("slow"),
	);
	let src = tempfile::tempdir().expect("src");
	let dst = tempfile::tempdir().expect("dst");
	let path = src.path().join("big.bin");
	std::fs::write(&path, bytes(8 * 1024 * 1024, 3)).unwrap();
	let id = offer(&mut chat, &path).await;
	arrived(&mut chat, &id).await;
	assert_ok(
		&chat
			.mb
			.respond_offer(&chat.sb, &id, true, dst.path().to_str().unwrap(), "")
			.await
			.expect("accept"),
	);
	// It is under way: bytes moved, a temp file exists, the final name does not.
	for _ in 0..500 {
		let now = offer_now(&mut chat.mb, &chat.sb, &id).await;
		if now.transferred_bytes > 0 {
			break;
		}
		settle(10).await;
	}
	assert_eq!(
		offer_now(&mut chat.mb, &chat.sb, &id).await.state,
		OfferState::InProgress as i32
	);
	assert!(
		!dst.path().join("big.bin").exists(),
		"not visible before verification"
	);
	assert_ok(
		&chat
			.mb
			.cancel_transfer(&chat.sb, &id)
			.await
			.expect("cancel"),
	);
	let on_b = wait_offer(&mut chat.mb, &chat.sb, &id, OfferState::Cancelled).await;
	assert_eq!(on_b.failure, "cancelled");
	let on_a = wait_offer(&mut chat.ma, &chat.sa, &id, OfferState::Cancelled).await;
	assert_eq!(on_a.failure, "cancelled_by_peer");
	settle(200).await;
	assert!(
		listing(dst.path()).is_empty(),
		"temp file removed, nothing visible"
	);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_sender_can_cancel_and_withdraw_offers() {
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
	std::fs::write(&path, bytes(6 * 1024 * 1024, 5)).unwrap();
	// An open offer is withdrawn.
	let first = offer(&mut chat, &path).await;
	arrived(&mut chat, &first).await;
	assert_ok(&chat.ma.cancel_transfer(&chat.sa, &first).await.unwrap());
	wait_offer(&mut chat.ma, &chat.sa, &first, OfferState::Cancelled).await;
	wait_offer(&mut chat.mb, &chat.sb, &first, OfferState::Cancelled).await;
	// A running transfer is stopped by its sender.
	let second = offer(&mut chat, &path).await;
	arrived(&mut chat, &second).await;
	assert_ok(
		&chat
			.mb
			.respond_offer(&chat.sb, &second, true, dst.path().to_str().unwrap(), "")
			.await
			.unwrap(),
	);
	for _ in 0..500 {
		if offer_now(&mut chat.ma, &chat.sa, &second)
			.await
			.transferred_bytes
			> 0 || offer_now(&mut chat.mb, &chat.sb, &second)
			.await
			.transferred_bytes
			> 0
		{
			break;
		}
		settle(10).await;
	}
	assert_ok(&chat.ma.cancel_transfer(&chat.sa, &second).await.unwrap());
	wait_offer(&mut chat.ma, &chat.sa, &second, OfferState::Cancelled).await;
	wait_offer(&mut chat.mb, &chat.sb, &second, OfferState::Cancelled).await;
	settle(200).await;
	assert!(listing(dst.path()).is_empty());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_file_that_changes_after_the_offer_never_arrives() {
	let mut chat = chatting().await;
	let src = tempfile::tempdir().unwrap();
	let dst = tempfile::tempdir().unwrap();
	let path = src.path().join("moving.bin");
	std::fs::write(&path, bytes(1024 * 1024, 1)).unwrap();
	let id = offer(&mut chat, &path).await;
	arrived(&mut chat, &id).await;
	// The user keeps editing after offering.
	std::fs::write(&path, bytes(1024 * 1024 + 17, 2)).unwrap();
	assert_ok(
		&chat
			.mb
			.respond_offer(&chat.sb, &id, true, dst.path().to_str().unwrap(), "")
			.await
			.unwrap(),
	);
	let on_a = wait_offer(&mut chat.ma, &chat.sa, &id, OfferState::Failed).await;
	assert_eq!(on_a.failure, "source_changed");
	let on_b = wait_offer(&mut chat.mb, &chat.sb, &id, OfferState::Cancelled).await;
	assert!(!on_b.failure.is_empty());
	settle(200).await;
	assert!(listing(dst.path()).is_empty(), "the receiver got no file");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn the_daemon_refuses_paths_it_must_not_open() {
	let mut chat = chatting().await;
	let src = tempfile::tempdir().unwrap();
	let real = src.path().join("real.txt");
	std::fs::write(&real, b"secret").unwrap();
	std::os::unix::fs::symlink(&real, src.path().join("link.txt")).unwrap();
	std::fs::create_dir(src.path().join("dir")).unwrap();
	for (path, why) in [
		("relative/file.txt".to_owned(), "relative"),
		(
			src.path().join("link.txt").to_string_lossy().into_owned(),
			"symlink",
		),
		(
			src.path().join("dir").to_string_lossy().into_owned(),
			"directory",
		),
		(
			src.path().join("missing").to_string_lossy().into_owned(),
			"missing",
		),
		(
			format!("{}/dir/../real.txt", src.path().display()),
			"dot-dot",
		),
	] {
		let refused = chat.ma.offer_file(&chat.sa, &path).await.unwrap();
		assert_eq!(op_code(&refused), "FILE_REFUSED", "{why}");
	}
	assert!(transcript(&mut chat.ma, &chat.sa).await.is_empty());

	// The size limit applies on both sides.
	let big = src.path().join("big.bin");
	std::fs::write(&big, bytes(2 * 1024 * 1024, 9)).unwrap();
	assert_ok(
		&chat
			.ma
			.set_messaging_settings(None, Some(1), None)
			.await
			.unwrap(),
	);
	let too_big = chat
		.ma
		.offer_file(&chat.sa, big.to_str().unwrap())
		.await
		.unwrap();
	assert_eq!(op_code(&too_big), "FILE_TOO_LARGE");
	assert_ok(
		&chat
			.ma
			.set_messaging_settings(None, Some(2048), None)
			.await
			.unwrap(),
	);
	assert_ok(
		&chat
			.mb
			.set_messaging_settings(None, Some(1), None)
			.await
			.unwrap(),
	);
	let id = offer(&mut chat, &big).await;
	let on_a = wait_offer(&mut chat.ma, &chat.sa, &id, OfferState::Failed).await;
	assert_eq!(
		on_a.state,
		OfferState::Failed as i32,
		"the receiver's limit rejects the offer"
	);
	assert!(transcript(&mut chat.mb, &chat.sb).await.is_empty());
}
