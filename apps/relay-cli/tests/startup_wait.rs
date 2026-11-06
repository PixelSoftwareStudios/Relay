//! `relay` against a daemon that is still opening the system keychain: it waits, never starts
//! a second daemon, and says plainly what to do when the keychain refused.
#![cfg(unix)]

#[path = "../../../crates/relay-engine/tests/support_prompt/mod.rs"]
mod support_prompt;

use std::path::Path;
use std::time::Duration;

use clap::Parser;
use relay_cli::cli::{Cli, CliError};
use relay_config::Paths;
use relay_engine::daemon::{Booting, DaemonHandle, NetworkOptions, RunOptions, Transport};
use relay_engine::service::fakes::FakePorts;
use support_prompt::{Handle, Mode, OpenOnDrop, Prompt};

async fn begin(dir: &Path, prompt: &std::sync::Arc<Prompt>) -> Booting {
	let mut options = RunOptions::new(Paths::under(dir.join("cfg")));
	options.transport = Transport::Unix(Some(dir.join("e.sock")));
	options.identity_store = Some(Box::new(Handle(prompt.clone())));
	options.network = NetworkOptions::loopback();
	let ports = FakePorts::default();
	options.adapters = Some(Box::new(move |_| ports.ports()));
	DaemonHandle::begin(options).await.expect("begin")
}

fn dir() -> tempfile::TempDir {
	tempfile::Builder::new()
		.prefix("rlyw")
		.tempdir_in("/tmp")
		.expect("dir")
}

/// `relay ...` against the private socket; a daemon binary that does not exist, so any attempt
/// to start another daemon fails loudly.
async fn relay(dir: &Path, args: &[&str]) -> Result<String, CliError> {
	let mut argv: Vec<String> = [
		"relay",
		"--socket",
		&dir.join("e.sock").display().to_string(),
		"--daemon-bin",
		&dir.join("no-such-relayd").display().to_string(),
		"--daemon-log",
		&dir.join("relayd.log").display().to_string(),
	]
	.iter()
	.map(|s| (*s).to_owned())
	.collect();
	argv.extend(args.iter().map(|a| (*a).to_owned()));
	let cli = Cli::try_parse_from(argv).expect("arguments");
	let mut out = Vec::new();
	relay_cli::commands::run(cli, &mut out).await?;
	Ok(String::from_utf8(out).expect("utf8"))
}

#[tokio::test]
async fn a_command_waits_for_the_keychain_prompt_and_does_not_start_another_daemon() {
	let dir = dir();
	let prompt = Prompt::new(Mode::Blocked);
	let _open = OpenOnDrop(prompt.clone());
	let booting = begin(dir.path(), &prompt).await;
	let ready = tokio::spawn(booting.ready());

	let status = relay(dir.path(), &["daemon", "status"])
		.await
		.expect("status");
	assert!(status.contains("starting"), "{status}");
	assert!(status.contains("secure storage"), "{status}");

	let mut command = Box::pin(relay(dir.path(), &["status"]));
	let waited = tokio::time::timeout(Duration::from_millis(900), &mut command).await;
	assert!(waited.is_err(), "the command waits for the prompt");

	prompt.set(Mode::Open);
	let daemon = ready.await.expect("join").expect("ready");
	let output = command.await.expect("status after the prompt");
	assert!(!output.is_empty());
	daemon.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn a_refused_keychain_is_explained_and_retry_recovers_the_same_daemon() {
	let dir = dir();
	let prompt = Prompt::new(Mode::Denied);
	let booting = begin(dir.path(), &prompt).await;
	let ready = tokio::spawn(booting.ready());

	let error = relay(dir.path(), &["status"]).await.expect_err("failed");
	let text = error.to_string();
	assert!(text.contains("keychain"), "{text}");
	assert!(text.contains("relay daemon retry"), "{text}");
	let status = relay(dir.path(), &["daemon", "status"])
		.await
		.expect("status");
	assert!(status.contains("failed"), "{status}");

	prompt.set(Mode::Open);
	relay(dir.path(), &["daemon", "retry"])
		.await
		.expect("retry");
	let daemon = ready.await.expect("join").expect("ready");
	assert!(relay(dir.path(), &["status"]).await.is_ok());
	daemon.shutdown().await.expect("shutdown");
}

#[tokio::test]
async fn the_file_store_and_the_identity_reset_need_an_explicit_yes() {
	let dir = dir();
	let prompt = Prompt::new(Mode::Corrupt);
	let booting = begin(dir.path(), &prompt).await;
	let ready = tokio::spawn(booting.ready());
	relay(dir.path(), &["daemon", "status"]).await.expect("up");

	for args in [
		&["daemon", "use-file-store"][..],
		&["identity", "reset"][..],
	] {
		let error = relay(dir.path(), args).await.expect_err("needs --yes");
		assert!(matches!(error, CliError::Usage(_)), "{error:?}");
	}
	let warning = relay(dir.path(), &["identity", "reset"])
		.await
		.unwrap_err()
		.to_string();
	assert!(
		warning.contains("verify"),
		"contacts must be warned: {warning}"
	);

	prompt.set(Mode::Open);
	relay(dir.path(), &["identity", "reset", "--yes"])
		.await
		.expect("reset");
	let daemon = ready.await.expect("join").expect("ready");
	daemon.shutdown().await.expect("shutdown");
}
