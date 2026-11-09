//! address, relay:// calls, privacy, channels and avatar against a scripted engine.

mod common;

use clap::Parser;
use common::fixtures::{CHANNEL_A, CHANNEL_B};
use common::{FakeControl, MemoryClipboard, ScriptedSecrets, run_scripted, run_with};
use relay_cli::cli::{Cli, CliError, exit};
use relay_cli::secret::SecretInput;

fn fingerprint() -> String {
	"0f".repeat(32)
}

async fn scripted(
	c: &mut FakeControl,
	args: &[&str],
	secrets: &[&str],
) -> Result<String, CliError> {
	let mut s = ScriptedSecrets::new(secrets);
	let mut board = MemoryClipboard::default();
	run_scripted(c, args, &mut s, &mut board).await
}

#[tokio::test]
async fn address_prints_only_the_connection_string_in_text_and_details_in_json() {
	let mut c = FakeControl::new();
	let text = run_with(&mut c, &["address"], false).await.unwrap();
	assert_eq!(
		text.trim(),
		format!("relay://192.168.1.20:47800#{}", fingerprint())
	);
	let json = run_with(&mut c, &["address"], true).await.unwrap();
	let v: serde_json::Value = serde_json::from_str(&json).unwrap();
	assert_eq!(v["listen_port"], 47_800);
	assert_eq!(v["local_addresses"][0], "192.168.1.20");
	assert_eq!(v["fingerprint"], fingerprint());
	assert_eq!(v["copied"], false);
}

#[tokio::test]
async fn address_copy_uses_the_clipboard_and_reports_failure_after_printing() {
	let mut c = FakeControl::new();
	let mut secrets = ScriptedSecrets::default();
	let mut board = MemoryClipboard::default();
	let text = run_scripted(&mut c, &["address", "--copy"], &mut secrets, &mut board)
		.await
		.unwrap();
	assert_eq!(board.text.as_deref(), Some(text.trim()));

	let mut broken = MemoryClipboard {
		fail: true,
		..MemoryClipboard::default()
	};
	let err = run_scripted(&mut c, &["address", "--copy"], &mut secrets, &mut broken)
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::FAILURE);
	assert!(err.to_string().contains("could not be copied"));
}

#[tokio::test]
async fn call_accepts_a_valid_connection_string_and_sends_it_normalized() {
	let mut c = FakeControl::new();
	let text = format!("relay://192.168.1.20:7000#{}", fingerprint());
	run_with(&mut c, &["call", &text], false).await.unwrap();
	assert_eq!(c.log, [format!("start_call_string {text}")]);
}

#[tokio::test]
async fn call_rejects_bad_connection_strings_before_any_request() {
	let mut c = FakeControl::new();
	for bad in [
		"relay://192.168.1.20:7000".to_owned(),
		format!("relay://192.168.1.20:7000#{}", "zz".repeat(32)),
		format!("relay://example.com:7000#{}", fingerprint()),
		format!("relay://8.8.8.8:7000#{}", fingerprint()),
		format!("relay://127.0.0.1:7000#{}", fingerprint()),
		format!("relay://user@192.168.1.20:7000#{}", fingerprint()),
	] {
		let err = run_with(&mut c, &["call", &bad], false).await.unwrap_err();
		assert_eq!(err.exit_code(), exit::USAGE, "{bad}: {err}");
	}
	assert!(c.log.is_empty(), "{:?}", c.log);
}

#[tokio::test]
async fn privacy_set_maps_flags_and_needs_one() {
	let mut c = FakeControl::new();
	run_with(
		&mut c,
		&[
			"privacy",
			"set",
			"--inbound",
			"shared-channel",
			"--advert",
			"hidden",
		],
		false,
	)
	.await
	.unwrap();
	run_with(&mut c, &["privacy", "set", "--advert", "visible"], false)
		.await
		.unwrap();
	run_with(&mut c, &["privacy", "set", "--inbound", "nobody"], false)
		.await
		.unwrap();
	assert_eq!(c.log[0], "privacy Some(SharedChannelOnly) Some(Hidden)");
	assert_eq!(c.log[1], "privacy None Some(Visible)");
	assert_eq!(c.log[2], "privacy Some(Nobody) None");
	let err = run_with(&mut c, &["privacy", "set"], false)
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
	assert!(Cli::try_parse_from(["relay", "privacy", "set", "--inbound", "everyone"]).is_err());
}

#[tokio::test]
async fn channel_list_renders_a_table_and_json() {
	let mut c = FakeControl::new();
	let text = run_with(&mut c, &["channel", "list"], false).await.unwrap();
	assert!(
		text.contains("Dorm") && text.contains("owner") && text.contains("password"),
		"{text}"
	);
	let json = run_with(&mut c, &["channel", "list"], true).await.unwrap();
	let v: serde_json::Value = serde_json::from_str(&json).unwrap();
	assert_eq!(v[0]["name"], "Dorm");
	assert_eq!(v[0]["id"], hex::encode(CHANNEL_A));
	assert_eq!(v[1]["role"], "member");
}

#[tokio::test]
async fn channel_create_reads_the_password_from_the_prompt_twice() {
	let mut c = FakeControl::new();
	let mut s = ScriptedSecrets::new(&["hunter2", "hunter2"]);
	let mut board = MemoryClipboard::default();
	run_scripted(
		&mut c,
		&["channel", "create", "Lab", "--password", "--hidden"],
		&mut s,
		&mut board,
	)
	.await
	.unwrap();
	assert_eq!(c.secrets, ["hunter2"]);
	assert_eq!(
		c.log,
		["create_channel Lab listed=false password=true invite_only=false"]
	);
	assert!(matches!(s.asked[0], SecretInput::Prompt(_)));

	let mut mismatch = ScriptedSecrets::new(&["one", "two"]);
	let err = run_scripted(
		&mut c,
		&["channel", "create", "Lab2", "--password"],
		&mut mismatch,
		&mut board,
	)
	.await
	.unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
	assert_eq!(c.log.len(), 1, "nothing sent when the confirmation differs");
}

#[tokio::test]
async fn channel_create_password_stdin_and_open_channels() {
	let mut c = FakeControl::new();
	let mut s = ScriptedSecrets::new(&["from-pipe"]);
	let mut board = MemoryClipboard::default();
	run_scripted(
		&mut c,
		&[
			"channel",
			"create",
			"Pipe",
			"--password-stdin",
			"--invite-only",
		],
		&mut s,
		&mut board,
	)
	.await
	.unwrap();
	assert_eq!(s.asked, [SecretInput::Stdin]);
	assert_eq!(
		c.log[0],
		"create_channel Pipe listed=false password=true invite_only=true"
	);
	scripted(&mut c, &["channel", "create", "Open"], &[])
		.await
		.unwrap();
	assert_eq!(
		c.log[1],
		"create_channel Open listed=true password=false invite_only=false"
	);
}

#[test]
fn secrets_cannot_be_given_on_the_command_line() {
	for args in [
		&["channel", "create", "Lab", "--password", "hunter2"][..],
		&["channel", "join", "Lab", "--password", "hunter2"],
		&["channel", "join", "Lab", "--invite", "RLYI1-abc"],
		&["channel", "create", "Lab", "--password=hunter2"],
	] {
		let parsed = Cli::try_parse_from(std::iter::once("relay").chain(args.iter().copied()));
		assert!(parsed.is_err(), "{args:?} must not parse");
	}
	assert!(
		Cli::try_parse_from(["relay", "channel", "join", "Lab", "--password", "--invite"]).is_err()
	);
}

#[tokio::test]
async fn channel_join_by_name_or_id_with_each_kind_of_credential() {
	let mut c = FakeControl::new();
	scripted(&mut c, &["channel", "join", "Lab", "--password"], &["pw"])
		.await
		.unwrap();
	scripted(
		&mut c,
		&["channel", "join", "Lab", "--invite-stdin"],
		&["RLYI1-x"],
	)
	.await
	.unwrap();
	let id = hex::encode(CHANNEL_B);
	scripted(&mut c, &["channel", "join", &id], &[])
		.await
		.unwrap();
	assert!(
		c.log[0].contains("Name(\"Lab\")") && c.log[0].ends_with("password"),
		"{:?}",
		c.log
	);
	assert!(c.log[1].ends_with("invite"));
	assert!(c.log[2].contains("Id(") && c.log[2].ends_with("none"));
	assert_eq!(c.secrets, ["pw", "RLYI1-x"]);
	let leaked = c.log.join("\n");
	assert!(
		!leaked.contains("pw\n") && !leaked.contains("RLYI1-x"),
		"secrets stay out of logs"
	);
}

#[tokio::test]
async fn leave_invite_and_revoke_resolve_the_channel_first() {
	let mut c = FakeControl::new();
	scripted(&mut c, &["channel", "leave", "dorm"], &[])
		.await
		.unwrap();
	let invite = scripted(
		&mut c,
		&[
			"channel",
			"invite",
			"Study",
			"--ttl-seconds",
			"600",
			"--uses",
			"3",
		],
		&[],
	)
	.await
	.unwrap();
	assert_eq!(invite.trim(), "RLYI1-test-token");
	let fp = "ab".repeat(32);
	scripted(&mut c, &["channel", "revoke", "Dorm", &fp], &[])
		.await
		.unwrap();
	assert_eq!(
		c.log[0],
		format!("leave_channel {}", hex::encode(CHANNEL_A))
	);
	assert_eq!(
		c.log[1],
		format!("create_invite {} 600 3", hex::encode(CHANNEL_B))
	);
	assert_eq!(c.log[2], format!("revoke {} {fp}", hex::encode(CHANNEL_A)));
	let err = scripted(&mut c, &["channel", "leave", "nope"], &[])
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::TARGET);
	let err = scripted(&mut c, &["channel", "revoke", "Dorm", "abcd"], &[])
		.await
		.unwrap_err();
	assert_eq!(err.exit_code(), exit::USAGE);
}

#[tokio::test]
async fn invite_json_carries_the_token_and_expiry() {
	let mut c = FakeControl::new();
	let json = run_with(&mut c, &["channel", "invite", "Dorm"], true)
		.await
		.unwrap();
	let v: serde_json::Value = serde_json::from_str(&json).unwrap();
	assert_eq!(v["invite"], "RLYI1-test-token");
	assert_eq!(v["expires_unix"], 1_900_000_000_u64);
	assert_eq!(
		c.log[0],
		format!("create_invite {} 3600 1", hex::encode(CHANNEL_A))
	);
}

#[test]
fn secret_debug_is_redacted() {
	let secret = relay_cli::secret::Secret::new("hunter2");
	assert!(!format!("{secret:?}").contains("hunter2"));
	let credential = relay_cli::client::Credential::Password(secret);
	assert!(!format!("{credential:?}").contains("hunter2"));
}

#[test]
fn stdin_secrets_drop_line_endings_and_refuse_empty() {
	let mut input = std::io::Cursor::new(b"s3cret\r\nrest".to_vec());
	assert_eq!(
		relay_cli::secret::read_line(&mut input).unwrap().expose(),
		"s3cret"
	);
	let mut empty = std::io::Cursor::new(b"\n".to_vec());
	assert_eq!(
		relay_cli::secret::read_line(&mut empty)
			.unwrap_err()
			.exit_code(),
		exit::USAGE
	);
}

#[tokio::test]
async fn a_refused_invite_is_a_rejection_not_an_empty_token() {
	let mut c = FakeControl::new();
	c.invite_failure = Some(relay_api::pb::Problem {
		code: "CHANNEL_DENIED".into(),
		message: "not allowed".into(),
		..Default::default()
	});
	let err = run_with(&mut c, &["channel", "invite", "Dorm"], false)
		.await
		.unwrap_err();
	assert_eq!(err.code(), "CHANNEL_DENIED");
	assert_eq!(err.exit_code(), exit::REJECTED);
}
