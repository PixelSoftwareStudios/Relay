//! Images and file offers through the command layer.

mod common;

use common::chat::SESSION_A;
use common::{FakeControl, run_messages};
use relay_cli::cli::CliError;

#[tokio::test]
async fn image_send_caps_at_15_kib_and_points_at_offer() {
	let dir = tempfile::tempdir().unwrap();
	let small = dir.path().join("s.png");
	std::fs::write(&small, vec![7u8; 15 * 1024]).unwrap();
	let big = dir.path().join("b.png");
	std::fs::write(&big, vec![7u8; 15 * 1024 + 1]).unwrap();
	let mut c = FakeControl::new();
	let out = run_messages(&mut c, &["image", "send", small.to_str().unwrap()], false)
		.await
		.ok();
	assert!(out.starts_with("sent image #1"), "{out}");
	let error = run_messages(&mut c, &["image", "send", big.to_str().unwrap()], false)
		.await
		.err();
	assert!(
		matches!(&error, CliError::Usage(m) if m.contains("offer") && m.contains("15 KiB")),
		"{error}"
	);
	assert_eq!(c.chat.log, ["send_image bytes=15360"]);
	let error = run_messages(
		&mut c,
		&["image", "send", dir.path().to_str().unwrap()],
		false,
	)
	.await
	.err();
	assert!(matches!(error, CliError::Usage(_)));
}

#[tokio::test]
async fn offer_makes_the_path_absolute_for_the_daemon() {
	let mut c = FakeControl::new();
	let out = run_messages(&mut c, &["offer", "Cargo.toml"], false)
		.await
		.ok();
	assert!(out.starts_with("offered #1 offer f0"), "{out}");
	let cwd = std::env::current_dir().unwrap();
	assert_eq!(
		c.chat.log,
		[format!("offer_file {}", cwd.join("Cargo.toml").display())]
	);
	run_messages(&mut c, &["offer", "../relay-cli/./Cargo.toml"], false)
		.await
		.ok();
	assert_eq!(
		c.chat.log[1], c.chat.log[0],
		"dots are folded: {:?}",
		c.chat.log
	);
}

fn with_offers() -> (FakeControl, Vec<u8>, Vec<u8>) {
	let mut c = FakeControl::new();
	let report = c
		.chat
		.incoming_offer(&SESSION_A, "report.pdf", 10 * 1024 * 1024);
	let tool = c.chat.incoming_offer(&SESSION_A, "setup.exe", 2048);
	c.chat.update_offer(&SESSION_A, &report, |o| {
		o.state = relay_api::pb::OfferState::InProgress as i32;
		o.transferred_bytes = 5 * 1024 * 1024;
	});
	c.chat
		.update_offer(&SESSION_A, &tool, |o| o.executable_warning = true);
	(c, report, tool)
}

#[tokio::test]
async fn offers_lists_state_progress_and_warnings() {
	let (mut c, report, _) = with_offers();
	let out = run_messages(&mut c, &["offers"], false).await.ok();
	assert!(
		out.contains("report.pdf") && out.contains("in-progress"),
		"{out}"
	);
	assert!(out.contains("50% of 10.0 MiB"), "{out}");
	assert!(out.contains("looks executable"), "{out}");
	assert!(out.contains(&hex::encode(&report)[..8]), "{out}");
	let out = run_messages(&mut c, &["offers"], true).await.ok();
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	assert_eq!(v[0]["percent"], 50);
	assert_eq!(v[0]["state"], "in-progress");
	assert_eq!(v[1]["executable_warning"], true);
}

#[tokio::test]
async fn accept_needs_an_existing_directory_and_passes_the_name_through() {
	let (mut c, report, tool) = with_offers();
	let dir = tempfile::tempdir().unwrap();
	let prefix = hex::encode(&report)[..6].to_owned();
	let d = dir.path().to_str().unwrap();
	run_messages(
		&mut c,
		&["accept", &prefix, "--dest", d, "--name", "mine.pdf"],
		false,
	)
	.await
	.ok();
	assert_eq!(
		c.chat.log.last().unwrap(),
		&format!("respond accept=true dest={d} name=mine.pdf")
	);
	let missing = dir.path().join("nope");
	let error = run_messages(
		&mut c,
		&[
			"accept",
			&hex::encode(&tool),
			"--dest",
			missing.to_str().unwrap(),
		],
		false,
	)
	.await
	.err();
	assert!(matches!(error, CliError::Usage(m) if m.contains("existing directory")));
}

#[tokio::test]
async fn reject_cancel_and_preview_find_offers_by_prefix_and_refuse_ambiguity() {
	let (mut c, report, tool) = with_offers();
	run_messages(&mut c, &["reject", &hex::encode(&tool)[..4]], false)
		.await
		.ok();
	run_messages(&mut c, &["cancel", &hex::encode(&report)[..4]], false)
		.await
		.ok();
	run_messages(
		&mut c,
		&["preview", "request", &hex::encode(&report)],
		false,
	)
	.await
	.ok();
	run_messages(
		&mut c,
		&["preview", "approve", &hex::encode(&report), "--deny"],
		false,
	)
	.await
	.ok();
	assert_eq!(
		&c.chat.log[..],
		[
			"respond accept=false dest= name=",
			"cancel",
			"preview request",
			"preview deny"
		]
	);
	// Both ids start with 'e', so one hex digit cannot pick.
	let error = run_messages(&mut c, &["cancel", "e"], false).await.err();
	assert_eq!(error.exit_code(), relay_cli::cli::exit::TARGET);
	assert!(error.to_string().contains("ambiguous"), "{error}");
	let error = run_messages(&mut c, &["cancel", "ab"], false).await.err();
	assert_eq!(error.exit_code(), relay_cli::cli::exit::TARGET);
}

#[tokio::test]
async fn files_off_surfaces_the_engines_refusal_with_a_hint() {
	let (mut c, report, _) = with_offers();
	c.chat.fail_with = Some("SESSION_PERMISSION_OFF".into());
	let dir = tempfile::tempdir().unwrap();
	let error = run_messages(
		&mut c,
		&[
			"accept",
			&hex::encode(&report),
			"--dest",
			dir.path().to_str().unwrap(),
		],
		false,
	)
	.await
	.err();
	assert_eq!(error.code(), "SESSION_PERMISSION_OFF");
	assert!(error.to_string().contains("perms"));
}
