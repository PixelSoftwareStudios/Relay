//! Commands start the daemon on their own, except where that would be wrong.
#![cfg(unix)]

mod support_relayd;

use std::path::PathBuf;
use std::time::Duration;

use clap::Parser;
use relay_cli::cli::{Cli, CliError, exit};
use support_relayd::{launches, wrapper};

struct Setup {
	dir: tempfile::TempDir,
}

impl Setup {
	fn new() -> Self {
		Self {
			dir: tempfile::Builder::new()
				.prefix("rlya")
				.tempdir_in("/tmp")
				.expect("dir"),
		}
	}

	fn socket(&self) -> PathBuf {
		self.dir.path().join("e.sock")
	}

	fn spawns(&self) -> PathBuf {
		self.dir.path().join("spawns")
	}

	fn launches(&self) -> usize {
		launches(&self.spawns())
	}

	fn cli(&self, args: &[&str]) -> Cli {
		let script = wrapper(self.dir.path(), &self.spawns());
		let log = self.dir.path().join("relayd.log");
		let mut argv: Vec<String> = ["relay", "--socket"]
			.into_iter()
			.map(str::to_owned)
			.chain([
				self.socket().display().to_string(),
				"--daemon-bin".into(),
				script.display().to_string(),
				"--daemon-log".into(),
				log.display().to_string(),
			])
			.collect();
		argv.extend(args.iter().map(|a| (*a).to_owned()));
		Cli::try_parse_from(argv).expect("valid arguments")
	}

	/// A separate `relay` process, as a user would run it.
	fn process(&self, args: &[&str]) -> std::process::Output {
		let script = wrapper(self.dir.path(), &self.spawns());
		std::process::Command::new(env!("CARGO_BIN_EXE_relay"))
			.arg("--socket")
			.arg(self.socket())
			.arg("--daemon-bin")
			.arg(script)
			.arg("--daemon-log")
			.arg(self.dir.path().join("relayd.log"))
			.args(args)
			.output()
			.expect("relay runs")
	}

	async fn relay(&self, args: &[&str]) -> Result<String, CliError> {
		let mut out = Vec::new();
		relay_cli::commands::run(self.cli(args), &mut out).await?;
		Ok(String::from_utf8(out).expect("utf8"))
	}
}

#[tokio::test]
async fn the_first_command_starts_the_daemon_and_later_ones_reuse_it() {
	let s = Setup::new();
	assert_eq!(s.launches(), 0);

	// JSON output stays clean: the note about starting goes to stderr.
	let out = s.relay(&["--json", "status"]).await.expect("status");
	let status: serde_json::Value = serde_json::from_str(&out).expect("pure JSON on stdout");
	assert!(status["calls"].as_array().unwrap().is_empty());
	assert_eq!(s.launches(), 1);
	assert!(s.socket().exists());

	s.relay(&["peers"]).await.expect("peers");
	s.relay(&["status"]).await.expect("status again");
	assert_eq!(s.launches(), 1, "a running daemon is not started twice");

	assert_eq!(s.process(&["daemon", "stop"]).status.code(), Some(0));
}

#[test]
fn a_stopped_daemon_stays_stopped_until_a_new_command_needs_it() {
	let s = Setup::new();
	let code = |out: &std::process::Output| out.status.code();
	let first = s.process(&["status"]);
	assert_eq!(code(&first), Some(0));
	assert!(
		String::from_utf8_lossy(&first.stderr).contains("starting relayd"),
		"the start is announced on stderr"
	);
	assert_eq!(s.launches(), 1);
	assert_eq!(code(&s.process(&["daemon", "stop"])), Some(0));
	assert!(!s.socket().exists());

	// Looking at the daemon never starts it.
	let status = s.process(&["daemon", "status"]);
	assert_eq!(code(&status), Some(i32::from(exit::UNREACHABLE)));
	s.process(&["doctor"]);
	s.process(&["daemon", "logs"]);
	assert_eq!(s.launches(), 1);
	assert!(!s.socket().exists());

	// The next command that needs it starts it again (a new process).
	assert_eq!(code(&s.process(&["status"])), Some(0));
	assert_eq!(s.launches(), 2);
	assert_eq!(code(&s.process(&["daemon", "stop"])), Some(0));
}

#[tokio::test]
async fn autostart_can_be_switched_off_and_exits_three() {
	let s = Setup::new();
	let error = s
		.relay(&["--no-autostart", "status"])
		.await
		.expect_err("off");
	assert_eq!(error.exit_code(), exit::UNREACHABLE);
	assert!(error.to_string().contains("relay daemon start"), "{error}");
	assert_eq!(s.launches(), 0);
}

#[tokio::test]
async fn a_failed_start_explains_itself_with_exit_three() {
	let s = Setup::new();
	let missing = s.dir.path().join("no-such-relayd");
	let mut out = Vec::new();
	let cli = Cli::try_parse_from([
		"relay",
		"--socket",
		s.socket().to_str().unwrap(),
		"--daemon-bin",
		missing.to_str().unwrap(),
		"status",
	])
	.unwrap();
	let error = relay_cli::commands::run(cli, &mut out)
		.await
		.expect_err("no binary");
	assert_eq!(error.exit_code(), exit::UNREACHABLE);
	assert!(error.to_string().contains("cannot find relayd"), "{error}");
}

#[tokio::test]
async fn call_stays_attached_until_hung_up() {
	let s = Setup::new();
	// A call to a LAN address nobody answers: it exists, rings nowhere, and
	// lives exactly as long as this command holds its watch.
	let target = format!("relay://192.168.1.50:7000#{}", "ab".repeat(32));
	let (hangup, mut rx) = tokio::sync::mpsc::channel(1);
	let mut out = Vec::new();
	let cli = s.cli(&["call", &target]);
	let held = relay_cli::commands::run_holding(cli, &mut out, &mut rx);
	let driver = async {
		tokio::time::sleep(Duration::from_millis(1_500)).await;
		let _ = hangup.send(()).await;
	};
	let (result, ()) = tokio::join!(held, driver);
	result.expect("the call ended on hangup");
	let text = String::from_utf8(out).unwrap();
	assert!(
		text.contains("preparing") || text.contains("outgoing"),
		"{text}"
	);
	assert!(
		text.contains("hanging up") || text.contains("ended") || text.contains("cancelled"),
		"{text}"
	);

	// With the holder gone nothing is left running for it.
	let status = s.relay(&["--json", "status"]).await.unwrap();
	let status: serde_json::Value = serde_json::from_str(&status).unwrap();
	let live = status["calls"]
		.as_array()
		.unwrap()
		.iter()
		.filter(|c| {
			!matches!(
				c["state"].as_str(),
				Some("ended" | "cancelled" | "failed" | "declined")
			)
		})
		.count();
	assert_eq!(live, 0, "{status}");
	assert_eq!(s.process(&["daemon", "stop"]).status.code(), Some(0));
}
