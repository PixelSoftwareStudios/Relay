#![cfg(unix)]
//! Message text, file names and paths exist in API bodies and events for the clients that
//! hold the capability, and nowhere else: not in any log line, even at TRACE.
//! This binary installs a global tracing subscriber that captures everything.

#[macro_use]
mod service_support;
mod support_channels;
mod support_daemon;
mod support_media;
mod support_messages;

use std::io::Write;
use std::sync::{Arc, Mutex};

use relay_api::pb::OfferState;
use support_messages::*;
use tracing_subscriber::fmt::MakeWriter;

const TEXT: &str = "zx9-text-marker-quokka";
const REPLY: &str = "zx9-reply-marker-dingo";
const FILE: &str = "zx9-file-marker-wombat.txt";
const DIR: &str = "zx9-dir-marker-numbat";
const BODY: &str = "zx9-body-marker-emu";

#[derive(Clone, Default)]
struct Capture(Arc<Mutex<Vec<u8>>>);

impl Write for Capture {
	fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
		self.0.lock().unwrap().extend_from_slice(buf);
		Ok(buf.len())
	}
	fn flush(&mut self) -> std::io::Result<()> {
		Ok(())
	}
}

impl<'a> MakeWriter<'a> for Capture {
	type Writer = Capture;
	fn make_writer(&'a self) -> Capture {
		self.clone()
	}
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn private_content_never_reaches_the_logs() {
	let capture = Capture::default();
	tracing::subscriber::set_global_default(
		tracing_subscriber::fmt()
			.with_max_level(tracing::Level::TRACE)
			.with_ansi(false)
			.with_writer(capture.clone())
			.finish(),
	)
	.expect("one subscriber per binary");

	let mut chat = chatting().await;
	let src = tempfile::tempdir().unwrap();
	let dst = tempfile::tempdir().unwrap();
	let dir = src.path().join(DIR);
	std::fs::create_dir(&dir).unwrap();
	let path = dir.join(FILE);
	std::fs::write(&path, format!("{BODY}\nsecond line\n")).unwrap();

	assert_ok(&chat.ma.send_session_message(&chat.sa, TEXT).await.unwrap());
	assert_ok(&chat.mb.send_session_message(&chat.sb, REPLY).await.unwrap());
	assert_ok(
		&chat
			.ma
			.send_session_image(&chat.sa, support_daemon::png(8, 8))
			.await
			.unwrap(),
	);
	let id = offer(&mut chat, &path).await;
	arrived(&mut chat, &id).await;
	assert_ok(&chat.mb.request_preview(&chat.sb, &id).await.unwrap());
	for _ in 0..300 {
		if offer_now(&mut chat.ma, &chat.sa, &id)
			.await
			.preview_approval_pending
		{
			break;
		}
		support_channels::settle(10).await;
	}
	assert_ok(&chat.ma.approve_preview(&chat.sa, &id, true).await.unwrap());
	assert_ok(
		&chat
			.mb
			.respond_offer(&chat.sb, &id, true, dst.path().to_str().unwrap(), "")
			.await
			.unwrap(),
	);
	wait_offer(&mut chat.mb, &chat.sb, &id, OfferState::Completed).await;
	// Refusals and failures log too: a bad path, bad text, a search, pins, an export.
	let _ = chat
		.ma
		.offer_file(&chat.sa, &format!("{}/{FILE}.missing", dir.display()))
		.await;
	let _ = chat
		.ma
		.send_session_message(&chat.sa, &format!("{TEXT}\u{202E}"))
		.await;
	let _ = chat.mb.search_session(&chat.sb, TEXT).await;
	let _ = chat.mb.pin_message(&chat.sb, false, 1, true).await;
	let exported = chat.mb.export_session(&chat.sb, false, None).await.unwrap();
	assert!(
		String::from_utf8_lossy(&exported.json).contains(TEXT),
		"the export is for the client"
	);
	let _ = chat.ma.clear_session(&chat.sa).await;
	support_channels::settle(500).await;
	let state_a = format!("{:?}", chat.a.cli.snapshot().await.unwrap());
	let state_b = format!("{:?}", chat.b.cli.snapshot().await.unwrap());

	chat.a.daemon.shutdown().await.unwrap();
	chat.b.daemon.shutdown().await.unwrap();

	let logs = String::from_utf8_lossy(&capture.0.lock().unwrap()).into_owned();
	assert!(
		logs.len() > 1_000,
		"the capture saw the engines: {} bytes",
		logs.len()
	);
	for secret in [TEXT, REPLY, FILE, DIR, BODY, "zx9-"] {
		assert!(!logs.contains(secret), "a log line leaked {secret}");
		assert!(
			!state_a.contains(secret) && !state_b.contains(secret),
			"state leaked {secret}"
		);
	}
}
