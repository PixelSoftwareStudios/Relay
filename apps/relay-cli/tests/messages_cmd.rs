//! `relay messages` command behaviour against the scripted fake: targeting, stdin, output.

mod common;

use common::chat::{CALL_A, CALL_OLD, SESSION_A, SESSION_OLD};
use common::{FakeControl, run_messages, run_messages_with};
use relay_cli::cli::CliError;

fn control() -> FakeControl {
	FakeControl::new()
}

#[tokio::test]
async fn send_targets_the_only_active_call_and_reports_the_message() {
	let mut c = control();
	let ran = run_messages(&mut c, &["send", "hello there"], false).await;
	assert_eq!(ran.ok().trim(), "sent #1");
	let entries = c.chat.transcript(&SESSION_A, 0, 10).unwrap().entries;
	assert_eq!(entries.len(), 1);
	assert!(entries[0].outgoing);
}

#[tokio::test]
async fn send_json_has_the_message_reference() {
	let mut c = control();
	let out = run_messages(&mut c, &["send", "hi"], true).await.ok();
	let v: serde_json::Value = serde_json::from_str(&out).unwrap();
	assert_eq!(
		(
			v["action"].as_str(),
			v["message"].as_u64(),
			v["seq"].as_u64()
		),
		(Some("sent"), Some(1), Some(1))
	);
}

#[tokio::test]
async fn send_dash_reads_stdin_and_drops_one_trailing_newline() {
	let mut c = control();
	run_messages_with(&mut c, &["send", "-"], false, "line one\nline two\n")
		.await
		.ok();
	let entries = c.chat.transcript(&SESSION_A, 0, 10).unwrap().entries;
	assert_eq!(
		relay_api::entry_text(&entries[0]),
		Some("line one\nline two")
	);
}

#[tokio::test]
async fn empty_and_oversized_text_never_reach_the_engine() {
	let mut c = control();
	let ran = run_messages_with(&mut c, &["send", "-"], false, "  \n").await;
	assert!(matches!(ran.err(), CliError::Usage(_)));
	let big = "x".repeat(5000);
	let ran = run_messages(&mut c, &["send", &big], false).await;
	assert!(matches!(ran.err(), CliError::Usage(_)));
	let huge = "y".repeat(100_000);
	let ran = run_messages_with(&mut c, &["send", "-"], false, &huge).await;
	assert!(matches!(ran.err(), CliError::Usage(_)));
	assert!(
		c.chat.log.iter().all(|l| !l.starts_with("send_text")),
		"{:?}",
		c.chat.log
	);
}

#[tokio::test]
async fn several_active_calls_are_refused_not_guessed() {
	let mut c = control();
	c.chat.add_session([0x53; 16], [0xc3; 16], "Carol", true);
	let ran = run_messages(&mut c, &["send", "who gets this"], false).await;
	let error = ran.err();
	assert!(
		matches!(&error, CliError::Target(relay_cli::cli::TargetError::NoDefault(m)) if m.contains("Alice") && m.contains("Carol")),
		"{error}"
	);
	assert_eq!(error.exit_code(), relay_cli::cli::exit::TARGET);
	assert!(c.chat.log.is_empty(), "nothing was sent: {:?}", c.chat.log);
}

#[tokio::test]
async fn call_flag_picks_by_name_prefix_and_id() {
	let mut c = control();
	c.chat.add_session([0x53; 16], [0xc3; 16], "Carol", true);
	run_messages(&mut c, &["--call", "car", "send", "to carol"], false)
		.await
		.ok();
	run_messages(
		&mut c,
		&["send", "to alice", "--call", &hex::encode(&CALL_A[..3])],
		false,
	)
	.await
	.ok();
	let carol = c.chat.transcript(&[0x53; 16], 0, 10).unwrap().entries;
	let alice = c.chat.transcript(&SESSION_A, 0, 10).unwrap().entries;
	assert_eq!((carol.len(), alice.len()), (1, 1));
	let ran = run_messages(&mut c, &["--call", "nobody", "send", "x"], false).await;
	assert_eq!(ran.err().exit_code(), relay_cli::cli::exit::TARGET);
}

#[tokio::test]
async fn an_ambiguous_name_lists_the_candidates() {
	let mut c = control();
	c.chat.add_session([0x53; 16], [0xc3; 16], "Alicia", true);
	let error = run_messages(&mut c, &["--call", "ali", "send", "x"], false)
		.await
		.err();
	assert!(
		error.to_string().contains("Alice") && error.to_string().contains("Alicia"),
		"{error}"
	);
}

#[tokio::test]
async fn with_no_active_call_a_retained_one_needs_naming() {
	let mut c = control();
	c.chat.sessions.clear();
	c.chat.add_session(SESSION_OLD, CALL_OLD, "Bob", false);
	let error = run_messages(&mut c, &["log"], false).await.err();
	assert!(error.to_string().contains("--call"), "{error}");
	c.chat.incoming_text(&SESSION_OLD, "from before");
	let out = run_messages(&mut c, &["--call", "bob", "log"], false)
		.await
		.ok();
	assert!(
		out.contains("ended with Bob") && out.contains("from before"),
		"{out}"
	);
}

#[tokio::test]
async fn engine_problem_codes_pass_through_with_a_hint() {
	let mut c = control();
	c.chat.fail_with = Some("HISTORY_FULL".into());
	let error = run_messages(&mut c, &["send", "x"], false).await.err();
	match &error {
		CliError::Rejected { code, message, .. } => {
			assert_eq!(code, "HISTORY_FULL");
			assert!(
				message.contains("export") && message.contains("clear"),
				"{message}"
			);
		}
		other => panic!("{other:?}"),
	}
	assert_eq!(error.code(), "HISTORY_FULL");
	c.chat.fail_with = Some("SESSION_PERMISSION_OFF".into());
	let error = run_messages(&mut c, &["send", "x"], false).await.err();
	assert!(error.to_string().contains("perms"), "{error}");
}
