//! Argument parsing, exit codes and the compiled binary.

use std::process::Command;

use clap::Parser;
use relay_cli::cli::{Cli, CliError, Command as Cmd, TargetError, exit};

fn parse(args: &[&str]) -> Result<Cli, clap::Error> {
	Cli::try_parse_from(std::iter::once("relay").chain(args.iter().copied()))
}

#[test]
fn every_documented_command_parses() {
	for args in [
		&["daemon", "start"][..],
		&["daemon", "stop"],
		&["daemon", "restart", "--wait", "3"],
		&["daemon", "status"],
		&["daemon", "logs", "-n", "5"],
		&["daemon", "run", "--", "--log-level", "debug"],
		&["status"],
		&["peers"],
		&["call", "alice"],
		&["accept"],
		&["decline", "abcd"],
		&["end"],
		&["mute"],
		&["mute", "off", "--call", "x"],
		&["deafen", "on"],
		&["gain", "playback", "0.8"],
		&["talk", "ptt"],
		&["devices", "list"],
		&["devices", "select", "--input", "usb", "--output", "hdmi"],
		&["ns", "set", "off"],
		&["profile", "set", "--name", "x"],
		&["trust", "list"],
		&["trust", "forget", "bob"],
		&["mic-test", "--seconds", "2"],
		&["doctor", "--bundle", "out.txt"],
		&["tui"],
	] {
		parse(args).unwrap_or_else(|e| panic!("{args:?}: {e}"));
	}
}

#[test]
fn global_flags_work_before_and_after_the_subcommand() {
	let a = parse(&["--json", "--socket", "/tmp/x.sock", "status"]).unwrap();
	let b = parse(&["status", "--json", "--socket", "/tmp/x.sock"]).unwrap();
	for cli in [a, b] {
		assert!(cli.global.json);
		assert_eq!(
			cli.global.socket.as_deref(),
			Some(std::path::Path::new("/tmp/x.sock"))
		);
		assert!(matches!(cli.command, Cmd::Status));
	}
	assert!(parse(&["--socket", "/a", "--tcp-profile", "/b", "status"]).is_err());
}

#[test]
fn unsafe_combinations_are_rejected_by_the_parser() {
	assert!(parse(&["mic-test", "--accept-feedback"]).is_err());
	assert!(parse(&["talk", "loud"]).is_err());
	assert!(
		parse(&["trust", "verify", "bob"]).is_err(),
		"fingerprint is required"
	);
}

#[test]
fn exit_codes_are_distinct_per_failure_class() {
	let target = CliError::Target(TargetError::NotFound {
		kind: "peer",
		query: "x".into(),
	});
	assert_eq!(target.exit_code(), exit::TARGET);
	assert_eq!(CliError::Usage("x".into()).exit_code(), exit::USAGE);
	let rejected = CliError::Rejected {
		code: "C".into(),
		message: "m".into(),
		retryable: true,
	};
	assert_eq!(rejected.exit_code(), exit::REJECTED);
	assert_eq!(
		CliError::Api(relay_api::ApiError::OperationTimeout).exit_code(),
		exit::TIMEOUT
	);
	let unavailable = relay_api::ApiError::from(tonic::Status::unavailable("down"));
	assert_eq!(CliError::Api(unavailable).exit_code(), exit::UNREACHABLE);
	let codes = [
		exit::FAILURE,
		exit::USAGE,
		exit::UNREACHABLE,
		exit::REJECTED,
		exit::TARGET,
		exit::TIMEOUT,
	];
	let mut unique = codes.to_vec();
	unique.sort_unstable();
	unique.dedup();
	assert_eq!(unique.len(), codes.len());
}

fn relay() -> Command {
	Command::new(env!("CARGO_BIN_EXE_relay"))
}

#[test]
fn binary_exits_2_on_bad_usage_and_3_without_an_engine() {
	let usage = relay().arg("frobnicate").output().unwrap();
	assert_eq!(usage.status.code(), Some(2));

	let dir = tempfile::tempdir().unwrap();
	let socket = dir.path().join("absent.sock");
	let out = relay()
		.args(["--json", "--no-autostart", "--socket"])
		.arg(&socket)
		.arg("status")
		.output()
		.unwrap();
	assert_eq!(out.status.code(), Some(i32::from(exit::UNREACHABLE)));
	assert!(out.stdout.is_empty(), "errors go to stderr");
	let body: serde_json::Value = serde_json::from_slice(&out.stderr).unwrap();
	assert_eq!(body["error"]["code"], "engine_unreachable");

	// The environment switch does the same as the flag.
	let out = relay()
		.env("RELAY_NO_AUTOSTART", "1")
		.args(["--json", "--socket"])
		.arg(&socket)
		.arg("status")
		.output()
		.unwrap();
	assert_eq!(out.status.code(), Some(i32::from(exit::UNREACHABLE)));
	assert!(!socket.exists(), "nothing was started");
}

#[test]
fn binary_reports_its_version() {
	let out = relay().arg("--version").output().unwrap();
	assert!(out.status.success());
	assert!(String::from_utf8_lossy(&out.stdout).starts_with("relay "));
}

#[test]
fn hotkey_status_reports_a_tagged_state_without_needing_an_engine() {
	let out = relay()
		.args(["--json", "hotkey", "status"])
		.output()
		.unwrap();
	assert!(
		out.status.success(),
		"{}",
		String::from_utf8_lossy(&out.stderr)
	);
	let body: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
	let state = body["state"].as_str().unwrap();
	assert!(
		[
			"active",
			"disabled",
			"needs_permission",
			"unsupported",
			"failed"
		]
		.contains(&state),
		"{body}"
	);
	assert!(body["binding"].is_string() && body["lock_detection"].is_string());
}
