#![cfg(unix)]
//! `relay messages` against two real daemons in a connected call: text both ways and a file
//! offer end to end. The daemon harness is the engine's own test support, included by path.

#[macro_use]
#[path = "../../../crates/relay-engine/tests/service_support/mod.rs"]
mod service_support;
#[path = "../../../crates/relay-engine/tests/support_channels/mod.rs"]
mod support_channels;
#[path = "../../../crates/relay-engine/tests/support_daemon/mod.rs"]
mod support_daemon;
#[path = "../../../crates/relay-engine/tests/support_media/mod.rs"]
mod support_media;
#[path = "../../../crates/relay-engine/tests/support_messages/mod.rs"]
mod support_messages;

use clap::Parser;
use relay_api::Endpoint;
use support_messages::chatting;

fn socket_of(endpoint: &Endpoint) -> String {
	match endpoint {
		Endpoint::Unix(path) => path.to_str().unwrap().to_owned(),
		other => panic!("unexpected endpoint {other:?}"),
	}
}

/// `relay --socket S messages ...` in-process; the output or the error text.
async fn relay(socket: &str, args: &[&str]) -> Result<String, String> {
	let mut argv = vec!["relay", "--socket", socket, "--no-autostart", "messages"];
	argv.extend_from_slice(args);
	let cli = relay_cli::cli::Cli::try_parse_from(argv).expect("valid arguments");
	let mut buffer = Vec::new();
	relay_cli::commands::run(cli, &mut buffer)
		.await
		.map_err(|e| e.to_string())?;
	Ok(String::from_utf8(buffer).expect("utf8"))
}

/// Run until `done(output)` holds, polling.
async fn until(socket: &str, args: &[&str], done: impl Fn(&str) -> bool) -> String {
	for _ in 0..400 {
		if let Ok(out) = relay(socket, args).await
			&& done(&out)
		{
			return out;
		}
		tokio::time::sleep(std::time::Duration::from_millis(25)).await;
	}
	panic!("`relay messages {args:?}` never reached the expected output");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn text_and_a_file_offer_travel_between_two_daemons_through_the_cli() {
	let chat = chatting().await;
	let (a, b) = (
		socket_of(chat.a.daemon.endpoint()),
		socket_of(chat.b.daemon.endpoint()),
	);

	// Text, A to B, with the code block numbered on the receiving side.
	let sent = relay(&a, &["send", "hello from A\n```rust\nlet x = 1;\n```"])
		.await
		.unwrap();
	assert!(sent.starts_with("sent #"), "{sent}");
	let seen = until(&b, &["log"], |o| o.contains("hello from A")).await;
	assert!(seen.contains("[code 1: rust, 1 line]"), "{seen}");
	until(&a, &["log"], |o| o.contains("delivered")).await;
	// And B back to A.
	relay(&b, &["send", "reply from B"]).await.unwrap();
	until(&a, &["log"], |o| o.contains("reply from B")).await;
	let json = relay(&a, &["search", "REPLY"]).await.unwrap();
	assert!(
		json.contains("reply from B") && !json.contains("hello from A"),
		"{json}"
	);

	// A file offer: nothing moves until B accepts.
	let src = tempfile::tempdir().unwrap();
	let dst = tempfile::tempdir().unwrap();
	let data: Vec<u8> = (0..700_000u32).map(|i| (i % 251) as u8).collect();
	let file = src.path().join("data.bin");
	std::fs::write(&file, &data).unwrap();
	let offered = relay(&a, &["offer", file.to_str().unwrap()]).await.unwrap();
	assert!(offered.starts_with("offered #"), "{offered}");
	let listing = until(&b, &["offers"], |o| {
		o.contains("data.bin") && o.contains("offered")
	})
	.await;
	let id = listing
		.lines()
		.find(|l| l.contains("data.bin"))
		.and_then(|l| l.split_whitespace().next())
		.unwrap()
		.to_owned();
	assert_eq!(std::fs::read_dir(dst.path()).unwrap().count(), 0);
	let accepted = relay(&b, &["accept", &id, "--dest", dst.path().to_str().unwrap()])
		.await
		.unwrap();
	assert!(accepted.starts_with("accepted"), "{accepted}");
	until(&b, &["offers"], |o| o.contains("completed")).await;
	until(&a, &["offers"], |o| o.contains("completed")).await;
	assert_eq!(std::fs::read(dst.path().join("data.bin")).unwrap(), data);

	// Pins are local, and a transcript exports to a new file only.
	relay(&a, &["pin", "1"]).await.unwrap();
	let out = dst.path().join("export.json");
	relay(&a, &["export", "--out", out.to_str().unwrap()])
		.await
		.unwrap();
	assert!(
		std::fs::read_to_string(&out)
			.unwrap()
			.contains("hello from A")
	);
	assert!(
		relay(&a, &["export", "--out", out.to_str().unwrap()])
			.await
			.is_err()
	);
	chat.a.daemon.shutdown().await.ok();
}
