//! Reading and organising a transcript: log, search, copy, pin, clear, export, perms.

mod common;

use common::chat::{SESSION_A, SESSION_OLD};
use common::{FakeControl, run_messages, run_messages_with};
use relay_cli::cli::CliError;

fn with_chat() -> FakeControl {
	let mut c = FakeControl::new();
	c.chat.incoming_text(&SESSION_A, "hello from Alice");
	c.chat.send_text(&SESSION_A, "hi back");
	c.chat.incoming_text(
		&SESSION_A,
		"try this\n```rust\nfn main() {}\n```\nand\n```\nplain block\n```",
	);
	c.chat.incoming_text(&SESSION_A, "```sh\nls -la\n```");
	c
}

#[tokio::test]
async fn log_numbers_messages_and_labels_code_blocks_across_the_session() {
	let mut c = with_chat();
	let out = run_messages(&mut c, &["log"], false).await.ok();
	assert!(
		out.contains("active with Alice (local times), 4 messages"),
		"{out}"
	);
	assert!(
		out.contains("#1  ") && out.contains("in   delivered"),
		"{out}"
	);
	assert!(out.contains("[code 1: rust, 1 line]"), "{out}");
	assert!(out.contains("[code 2: no language, 1 line]"), "{out}");
	assert!(out.contains("[code 3: sh, 1 line]"), "{out}");
	assert!(out.contains("00:13:21"), "{out}");
}

#[tokio::test]
async fn log_since_keeps_block_numbers_and_limit_says_how_to_continue() {
	let mut c = with_chat();
	let out = run_messages(&mut c, &["log", "--since", "4"], false)
		.await
		.ok();
	assert!(out.contains("[code 3: sh"), "numbers do not restart: {out}");
	assert!(!out.contains("hello from Alice"));
	let out = run_messages(&mut c, &["log", "--limit", "2"], false)
		.await
		.ok();
	assert!(out.contains("-- more: continue with --since 3"), "{out}");
}

#[tokio::test]
async fn log_json_is_an_array_with_code_blocks_links_and_delivery() {
	let mut c = with_chat();
	c.chat.incoming_text(
		&SESSION_A,
		"see https://example.com/a and https://xn--e1awd7f.example/",
	);
	let out = run_messages(&mut c, &["log"], true).await.ok();
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	let a = v.as_array().unwrap();
	assert_eq!(a.len(), 5);
	assert_eq!(a[1]["direction"], "out");
	assert_eq!(a[1]["delivery"], "sending");
	assert_eq!(a[2]["code_blocks"][1]["language"], serde_json::Value::Null);
	assert_eq!(a[2]["code_blocks"][0]["language"], "rust");
	assert_eq!(a[4]["links"][0]["host"], "example.com");
	assert_eq!(a[4]["links"][0]["warning"], serde_json::Value::Null);
	assert_eq!(a[4]["links"][1]["warning"], "international-domain");
}

#[tokio::test]
async fn links_show_the_safe_host_and_warnings_but_are_never_opened() {
	let mut c = FakeControl::new();
	c.chat
		.incoming_text(&SESSION_A, "click https://xn--80ak6aa92e.com/login");
	let out = run_messages(&mut c, &["log"], false).await.ok();
	assert!(out.contains("host xn--80ak6aa92e.com"), "{out}");
	assert!(
		out.contains("WARNING") && out.contains("(not opened)"),
		"{out}"
	);
}

#[tokio::test]
async fn control_characters_in_received_text_cannot_reach_the_terminal() {
	let mut c = FakeControl::new();
	c.chat
		.incoming_text(&SESSION_A, "evil \u{1b}[31mred\u{7} text");
	let out = run_messages(&mut c, &["log"], false).await.ok();
	assert!(!out.contains('\u{1b}') && !out.contains('\u{7}'), "{out:?}");
	assert!(out.contains("evil \u{fffd}[31mred"), "{out:?}");
}

#[tokio::test]
async fn copy_takes_a_message_or_a_numbered_code_block() {
	let mut c = with_chat();
	let ran = run_messages(&mut c, &["copy", "1"], false).await;
	assert_eq!(ran.clipboard.as_deref(), Some("hello from Alice"));
	assert!(ran.ok().contains("copied 16 bytes of message #1"));
	let ran = run_messages(&mut c, &["copy", "code-block", "1"], false).await;
	assert_eq!(ran.clipboard.as_deref(), Some("fn main() {}"));
	let ran = run_messages(&mut c, &["copy", "code-block", "3", "--print"], false).await;
	assert_eq!(ran.clipboard.as_deref(), Some("ls -la"));
	assert_eq!(ran.out.trim(), "ls -la");
	let error = run_messages(&mut c, &["copy", "code-block", "9"], false)
		.await
		.err();
	assert_eq!(error.exit_code(), relay_cli::cli::exit::TARGET);
	let error = run_messages(&mut c, &["copy", "code-block"], false)
		.await
		.err();
	assert!(matches!(error, CliError::Usage(_)));
}

#[tokio::test]
async fn copy_without_a_clipboard_still_can_print_and_fails_honestly() {
	use relay_cli::commands::clipboard::Clipboard;
	struct Broken;
	impl Clipboard for Broken {
		fn copy(&mut self, _: &str) -> Result<(), String> {
			Err("no display".into())
		}
	}
	let mut c = with_chat();
	let mut buffer = Vec::new();
	let mut broken = Broken;
	let args = relay_cli::cli::MessagesArgs {
		call: None,
		command: relay_cli::cli::MessagesCommand::Copy {
			target: "1".into(),
			number: None,
			print: true,
		},
	};
	let mut out = relay_cli::commands::Out::new(false, &mut buffer).with_clipboard(&mut broken);
	let error = relay_cli::commands::messages::run(&mut c, args, &mut out)
		.await
		.unwrap_err();
	assert!(error.to_string().contains("no display"));
	assert!(
		String::from_utf8(buffer)
			.unwrap()
			.contains("hello from Alice")
	);
}

#[tokio::test]
async fn search_prints_the_matching_messages_with_their_numbers() {
	let mut c = with_chat();
	let out = run_messages(&mut c, &["search", "HELLO"], false).await.ok();
	assert!(
		out.contains("#1 ") && out.contains("hello from Alice"),
		"{out}"
	);
	assert!(!out.contains("hi back"));
	let out = run_messages(&mut c, &["search", "nothing-like-it"], false)
		.await
		.ok();
	assert_eq!(out.trim(), "no matches");
	let error = run_messages(&mut c, &["search", "  "], false).await.err();
	assert!(matches!(error, CliError::Usage(_)));
}

#[tokio::test]
async fn pin_resolves_the_message_number_to_its_direction_and_sequence() {
	let mut c = with_chat();
	assert_eq!(
		run_messages(&mut c, &["pin", "2"], false).await.ok().trim(),
		"pinned #2"
	);
	assert!(
		c.chat
			.log
			.contains(&"pin outgoing=true seq=1 true".to_owned()),
		"{:?}",
		c.chat.log
	);
	run_messages(&mut c, &["unpin", "1"], false).await.ok();
	assert!(
		c.chat
			.log
			.contains(&"pin outgoing=false seq=1 false".to_owned())
	);
	let error = run_messages(&mut c, &["pin", "77"], false).await.err();
	assert_eq!(error.exit_code(), relay_cli::cli::exit::TARGET);
}

#[tokio::test]
async fn clear_asks_first_and_yes_skips_the_question() {
	let mut c = with_chat();
	let ran = run_messages_with(&mut c, &["clear"], false, "n\n").await;
	assert!(matches!(ran.err(), CliError::Usage(m) if m.contains("not cleared")));
	assert_eq!(
		c.chat.transcript(&SESSION_A, 0, 10).unwrap().entries.len(),
		4
	);
	run_messages_with(&mut c, &["clear"], false, "yes\n")
		.await
		.ok();
	assert!(
		c.chat
			.transcript(&SESSION_A, 0, 10)
			.unwrap()
			.entries
			.is_empty()
	);
	c.chat.incoming_text(&SESSION_A, "after");
	assert_eq!(
		c.chat.transcript(&SESSION_A, 0, 10).unwrap().entries[0].ordinal,
		5,
		"ordinals keep counting"
	);
	run_messages(&mut c, &["clear", "--yes"], false).await.ok();
	assert!(c.chat.log.iter().filter(|l| *l == "clear").count() == 2);
}

#[tokio::test]
async fn clearing_an_ended_session_removes_it() {
	let mut c = FakeControl::new();
	c.chat.add_session(SESSION_OLD, [0xc2; 16], "Bob", false);
	run_messages(&mut c, &["--call", "bob", "clear", "--yes"], false)
		.await
		.ok();
	assert_eq!(c.chat.sessions.len(), 1);
}

#[tokio::test]
async fn export_prints_json_or_writes_a_new_file_and_never_replaces() {
	let mut c = with_chat();
	let out = run_messages(&mut c, &["export"], false).await.ok();
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	assert_eq!(v["messages"].as_array().unwrap().len(), 4);

	let dir = tempfile::tempdir().unwrap();
	let file = dir.path().join("chat.json");
	let path = file.to_str().unwrap();
	let out = run_messages(&mut c, &["export", "--out", path], false)
		.await
		.ok();
	assert!(out.contains(path) && file.exists(), "{out}");
	let error = run_messages(&mut c, &["export", "--out", path], false)
		.await
		.err();
	assert!(error.to_string().contains("never replaces"), "{error}");
	assert!(
		c.chat
			.log
			.iter()
			.filter(|l| l.starts_with("export dest=") && l.contains(path))
			.count() == 1
	);
}

#[tokio::test]
async fn export_makes_a_relative_path_absolute_before_the_daemon_sees_it() {
	let mut c = with_chat();
	let name = format!("relay-export-{}.json", std::process::id());
	let ran = run_messages(&mut c, &["export", "--out", &name], true).await;
	let out = ran.ok();
	let cwd = std::env::current_dir().unwrap();
	let expected = cwd.join(&name);
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	assert_eq!(v["path"], expected.to_str().unwrap());
	let _ = std::fs::remove_file(expected);
}

#[tokio::test]
async fn perms_shows_and_changes_what_the_session_allows() {
	let mut c = FakeControl::new();
	let out = run_messages(&mut c, &["perms"], false).await.ok();
	assert!(
		out.contains("messages on, files on") && out.contains("you are the host"),
		"{out}"
	);
	let out = run_messages(&mut c, &["perms", "--files", "off"], false)
		.await
		.ok();
	assert!(out.contains("messages on, files off"), "{out}");
	assert!(
		c.chat
			.log
			.contains(&"permissions None Some(false)".to_owned())
	);
	let out = run_messages(&mut c, &["perms", "--messages", "off"], true)
		.await
		.ok();
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	assert_eq!(
		(v["allow_messages"].as_bool(), v["allow_files"].as_bool()),
		(Some(false), Some(false))
	);
}

#[tokio::test]
async fn a_full_budget_is_stated_with_the_way_out() {
	let mut c = FakeControl::new();
	c.chat.sessions[0].summary.used_bytes = c.chat.sessions[0].summary.limit_bytes;
	let out = run_messages(&mut c, &["perms"], false).await.ok();
	assert!(
		out.contains("budget is full") && out.contains("export"),
		"{out}"
	);
}
