//! `relay messages` over a real gRPC connection: capabilities, the wire and the logs.

#![cfg(unix)]

mod common;

use std::io::Write;
use std::sync::{Arc, Mutex};

use common::chat::SESSION_A;
use common::engine::EngineHarness;

#[tokio::test]
async fn messaging_commands_register_with_the_messages_capability_only() {
	let h = EngineHarness::start().await;
	h.run(&["messages", "send", "hello over the wire"])
		.await
		.unwrap();
	let registered = h.engine.registered.lock().unwrap().clone();
	assert_eq!(registered, [vec!["messages".to_owned()]]);
	let chat = h.engine.chat.lock().unwrap();
	let entries = chat.transcript(&SESSION_A, 0, 10).unwrap().entries;
	assert_eq!(
		relay_api::entry_text(&entries[0]),
		Some("hello over the wire")
	);
}

#[tokio::test]
async fn other_commands_never_get_the_messages_capability_and_are_denied_messaging() {
	let h = EngineHarness::start().await;
	h.run(&["status"]).await.unwrap();
	assert_eq!(
		h.engine.registered.lock().unwrap().clone(),
		[vec!["observe".to_owned(), "call-control".to_owned()]]
	);
	// The engine gates on the capability, so a client without it sees PERMISSION_DENIED.
	let mut client = relay_client::wire::connect_registered(&h.endpoint(), "t", "0", &["observe"])
		.await
		.unwrap();
	let error = client.list_sessions().await.unwrap_err();
	assert!(error.to_string().contains("messages capability"), "{error}");
}

#[tokio::test]
async fn log_search_pin_and_offers_work_through_grpc() {
	let h = EngineHarness::start().await;
	{
		let mut chat = h.engine.chat.lock().unwrap();
		chat.incoming_text(&SESSION_A, "wire hello\n```py\nprint(1)\n```");
		chat.incoming_offer(&SESSION_A, "notes.txt", 5000);
	}
	let out = h.run(&["--json", "messages", "log"]).await.unwrap();
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	assert_eq!(v[0]["code_blocks"][0]["language"], "py");
	assert_eq!(v[1]["offer"]["name"], "notes.txt");
	let out = h.run(&["messages", "search", "WIRE"]).await.unwrap();
	assert!(out.contains("wire hello"), "{out}");
	h.run(&["messages", "pin", "1"]).await.unwrap();
	let out = h.run(&["messages", "offers"]).await.unwrap();
	assert!(
		out.contains("notes.txt") && out.contains("offered"),
		"{out}"
	);
	let dir = tempfile::tempdir().unwrap();
	let id = {
		let chat = h.engine.chat.lock().unwrap();
		let t = chat.transcript(&SESSION_A, 2, 1).unwrap();
		match &t.entries[0].body {
			Some(relay_api::pb::transcript_entry::Body::File(o)) => {
				hex::encode(&o.offer_id)[..8].to_owned()
			}
			_ => panic!("offer"),
		}
	};
	h.run(&[
		"messages",
		"accept",
		&id,
		"--dest",
		dir.path().to_str().unwrap(),
	])
	.await
	.unwrap();
	let out = h.run(&["messages", "offers"]).await.unwrap();
	assert!(out.contains("in-progress"), "{out}");
	let copy = h
		.run(&["messages", "copy", "code-block", "1", "--print"])
		.await;
	// A headless test machine may have no clipboard; the text is printed either way.
	let _ = copy;
}

#[tokio::test]
async fn watch_streams_a_live_message_and_survives_an_engine_resync() {
	let h = EngineHarness::start().await;
	h.engine.chat.lock().unwrap().resync_next_watch = true;
	let socket = h.socket.to_str().unwrap().to_owned();
	let shared = Arc::new(Mutex::new(Vec::<u8>::new()));
	struct Sink(Arc<Mutex<Vec<u8>>>);
	impl Write for Sink {
		fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
			self.0.lock().unwrap().extend_from_slice(b);
			Ok(b.len())
		}
		fn flush(&mut self) -> std::io::Result<()> {
			Ok(())
		}
	}
	let mut sink = Sink(Arc::clone(&shared));
	let watch = async {
		use clap::Parser;
		let cli = relay_cli::cli::Cli::try_parse_from([
			"relay", "--socket", &socket, "messages", "watch",
		])
		.unwrap();
		relay_cli::commands::run(cli, &mut sink).await
	};
	let seen = |needle: &str| String::from_utf8_lossy(&shared.lock().unwrap()).contains(needle);
	let driver = async {
		// The resync happens first; then a message arrives on the fresh watch.
		for _ in 0..200 {
			if seen("resync") {
				break;
			}
			tokio::time::sleep(std::time::Duration::from_millis(20)).await;
		}
		tokio::time::sleep(std::time::Duration::from_millis(300)).await;
		h.engine
			.chat
			.lock()
			.unwrap()
			.incoming_text(&SESSION_A, "live one");
		for _ in 0..200 {
			if seen("live one") {
				break;
			}
			tokio::time::sleep(std::time::Duration::from_millis(20)).await;
		}
	};
	tokio::select! {
		result = watch => panic!("watch ended early: {result:?}"),
		() = driver => {}
	}
	let out = String::from_utf8(shared.lock().unwrap().clone()).unwrap();
	assert!(out.contains("-- resync: lagged"), "{out}");
	assert!(
		out.contains("[Alice] #1") && out.contains("live one"),
		"{out}"
	);
	assert_eq!(out.matches("live one").count(), 1, "{out}");
}

/// A subscriber that records every event at TRACE level, fields included.
#[derive(Clone, Default)]
struct Capture(Arc<Mutex<String>>);

impl Write for Capture {
	fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
		self.0.lock().unwrap().push_str(&String::from_utf8_lossy(b));
		Ok(b.len())
	}
	fn flush(&mut self) -> std::io::Result<()> {
		Ok(())
	}
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Capture {
	type Writer = Capture;
	fn make_writer(&'a self) -> Capture {
		self.clone()
	}
}

#[tokio::test]
async fn message_text_and_names_never_reach_a_trace_log() {
	const SECRET_TEXT: &str = "ZEBRA-PASSPHRASE-7731";
	// A global subscriber: per-thread ones lose to callsites that other tests registered first.
	static CAPTURE: std::sync::OnceLock<Capture> = std::sync::OnceLock::new();
	let capture = CAPTURE
		.get_or_init(|| {
			let capture = Capture::default();
			let subscriber = tracing_subscriber::fmt()
				.with_max_level(tracing::Level::TRACE)
				.with_writer(capture.clone())
				.with_ansi(false)
				.finish();
			tracing::subscriber::set_global_default(subscriber).expect("first subscriber");
			capture
		})
		.clone();
	let h = EngineHarness::start().await;
	h.engine
		.chat
		.lock()
		.unwrap()
		.incoming_text(&SESSION_A, &format!("peer says {SECRET_TEXT}-in"));
	let dir = tempfile::tempdir().unwrap();
	let image = dir.path().join("pic-ZEBRA.png");
	std::fs::write(&image, [1u8; 64]).unwrap();
	h.run(&["messages", "send", &format!("{SECRET_TEXT}-out")])
		.await
		.unwrap();
	h.run(&["messages", "log"]).await.unwrap();
	h.run(&["messages", "search", SECRET_TEXT]).await.unwrap();
	h.run(&["messages", "image", "send", image.to_str().unwrap()])
		.await
		.unwrap();
	h.run(&["messages", "offer", image.to_str().unwrap()])
		.await
		.ok();
	h.run(&["messages", "pin", "1"]).await.unwrap();
	let logged = capture.0.lock().unwrap().clone();
	assert!(
		logged.contains("sending a session message"),
		"the log is not empty"
	);
	assert!(
		!logged.contains("ZEBRA"),
		"message text or a file name was logged:\n{logged}"
	);
	let sent = h.engine.chat.lock().unwrap().log.clone();
	assert!(sent.iter().any(|l| l.starts_with("send_text")), "{sent:?}");
}
