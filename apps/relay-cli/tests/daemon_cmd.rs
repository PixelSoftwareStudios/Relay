//! `relay daemon` against the real `relayd` binary: start, status, stop.
#![cfg(unix)]

use std::path::PathBuf;

use clap::Parser;
use relay_cli::cli::{Cli, CliError, exit};
use support_relayd::relayd;

mod support_relayd;

struct Setup {
	dir: tempfile::TempDir,
}

impl Setup {
	fn new() -> Self {
		Self {
			dir: tempfile::Builder::new()
				.prefix("rlyd")
				.tempdir_in("/tmp")
				.expect("dir"),
		}
	}

	fn path(&self, name: &str) -> PathBuf {
		self.dir.path().join(name)
	}

	async fn relay(&self, args: &[&str]) -> Result<String, CliError> {
		self.relay_with(&[], args).await
	}

	/// `relay --socket <private> <global...> <args...>`.
	async fn relay_with(&self, global: &[&str], args: &[&str]) -> Result<String, CliError> {
		let socket = self.path("e.sock");
		let mut argv = vec!["relay", "--socket", socket.to_str().expect("utf8")];
		argv.extend_from_slice(global);
		argv.extend_from_slice(args);
		let cli = Cli::try_parse_from(argv).expect("valid arguments");
		let mut out = Vec::new();
		relay_cli::commands::run(cli, &mut out).await?;
		Ok(String::from_utf8(out).expect("utf8"))
	}

	/// `daemon status` once the engine finished starting: the socket answers a moment before
	/// the identity is open, and a slow runner can ask in between (it then says `starting`).
	async fn running_status(&self) -> String {
		let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
		loop {
			let status = self.daemon("status").await.expect("status");
			if !status.starts_with("daemon starting") || std::time::Instant::now() > deadline {
				return status;
			}
			tokio::time::sleep(std::time::Duration::from_millis(100)).await;
		}
	}

	/// `relay daemon <verb>` with a throwaway home and no LAN.
	async fn daemon(&self, verb: &str) -> Result<String, CliError> {
		let binary = relayd().to_str().expect("utf8").to_owned();
		let log = self.path("relayd.log");
		let home = self.path("home");
		let home = home.to_str().expect("utf8").to_owned();
		let log = log.to_str().expect("utf8").to_owned();
		match verb {
			"start" | "restart" => {
				self.relay_with(
					&["--daemon-bin", &binary],
					&[
						"daemon",
						verb,
						"--log-file",
						&log,
						"--",
						"--home",
						&home,
						"--identity-store",
						"memory",
						"--listen-port",
						"0",
						"--no-discovery",
					],
				)
				.await
			}
			"logs" => self.relay(&["daemon", "logs", "--file", &log]).await,
			other => self.relay(&["daemon", other]).await,
		}
	}
}

#[tokio::test]
async fn the_daemon_is_started_inspected_and_stopped_by_the_cli() {
	let s = Setup::new();

	// Nothing runs: every command says so and exits with the unreachable code.
	let error = s.daemon("status").await.expect_err("not running");
	assert_eq!(error.exit_code(), exit::UNREACHABLE);
	assert!(error.to_string().contains("relay daemon start"), "{error}");
	let error = s
		.relay(&["--no-autostart", "peers"])
		.await
		.expect_err("autostart is off");
	assert_eq!(error.exit_code(), exit::UNREACHABLE);
	assert!(error.to_string().contains("relay daemon start"), "{error}");
	assert!(
		!s.path("e.sock").exists(),
		"nothing was started behind our back"
	);
	assert!(
		s.daemon("stop")
			.await
			.expect("stop")
			.contains("not running")
	);

	let started = s.daemon("start").await.expect("start");
	assert!(started.contains("started"), "{started}");
	assert!(
		s.daemon("start")
			.await
			.expect("again")
			.contains("already running")
	);
	let status = s.running_status().await;
	assert!(
		status.contains("running") && status.contains("engine"),
		"{status}"
	);
	// Ordinary commands now work against it.
	s.relay(&["--json", "status"])
		.await
		.expect("status over the API");
	assert!(
		s.daemon("logs")
			.await
			.expect("logs")
			.contains("engine listening")
	);

	// It outlives the command that started it: it is still there for the next one.
	let restarted = s.daemon("restart").await.expect("restart");
	assert!(restarted.contains("started"), "{restarted}");
	s.daemon("status").await.expect("running after restart");

	let stopped = s.daemon("stop").await.expect("stop");
	assert!(stopped.contains("stopped"), "{stopped}");
	assert!(!s.path("e.sock").exists(), "the endpoint is removed");
	let error = s.daemon("status").await.expect_err("gone");
	assert_eq!(error.exit_code(), exit::UNREACHABLE);
}

#[tokio::test]
async fn a_missing_binary_is_reported() {
	let s = Setup::new();
	let missing = s.path("nope").to_str().expect("utf8").to_owned();
	let error = s
		.relay_with(&["--daemon-bin", &missing], &["daemon", "start"])
		.await
		.expect_err("no binary");
	let text = error.to_string();
	assert!(text.contains("nope"), "{text}");
}
