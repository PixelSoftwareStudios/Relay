#![cfg(unix)]
//! The launcher against a stand-in daemon binary.

use std::path::{Path, PathBuf};
use std::time::Duration;

use relay_api::Endpoint;
use relay_client::daemon::{self, Launch, Started};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-relayd");

struct Home {
	dir: tempfile::TempDir,
}

impl Home {
	fn new() -> Self {
		Self {
			dir: tempfile::Builder::new()
				.prefix("rlyc")
				.tempdir_in("/tmp")
				.expect("dir"),
		}
	}
	fn socket(&self) -> PathBuf {
		self.dir.path().join("e.sock")
	}
	fn launch(&self) -> Launch {
		let mut launch = Launch::new(Endpoint::Unix(self.socket()));
		launch.binary = Some(PathBuf::from(FAKE));
		launch.log_file = Some(self.dir.path().join("relayd.log"));
		launch.timeout = Duration::from_secs(5);
		launch
	}
}

impl Drop for Home {
	fn drop(&mut self) {
		// The stand-in exits when its socket file is gone.
		let _ = std::fs::remove_file(self.socket());
	}
}

#[tokio::test]
async fn a_missing_daemon_is_started_once_and_then_found_running() {
	let home = Home::new();
	let launch = home.launch();
	assert!(!daemon::reachable(&launch.endpoint).await);
	let Started::Launched { pid, binary, log } = daemon::start(&launch).await else {
		panic!("the daemon must start");
	};
	assert!(pid > 0);
	assert_eq!(binary, Path::new(FAKE));
	assert!(daemon::reachable(&launch.endpoint).await);
	assert!(
		std::fs::read_to_string(log)
			.expect("log")
			.contains("listening")
	);
	assert_eq!(daemon::start(&launch).await, Started::AlreadyRunning);
}

#[tokio::test]
async fn the_daemon_runs_in_its_own_process_group() {
	let home = Home::new();
	let Started::Launched { pid, .. } = daemon::start(&home.launch()).await else {
		panic!("the daemon must start");
	};
	let group = |pid: u32| {
		let out = std::process::Command::new("ps")
			.args(["-o", "pgid=", "-p", &pid.to_string()])
			.output()
			.expect("ps");
		String::from_utf8_lossy(&out.stdout).trim().to_owned()
	};
	assert_eq!(
		group(pid),
		pid.to_string(),
		"its own group, so a terminal's signals miss it"
	);
	assert_ne!(group(pid), group(std::process::id()));
}

#[tokio::test]
async fn a_missing_binary_is_reported() {
	let home = Home::new();
	let mut launch = home.launch();
	launch.binary = Some(home.dir.path().join("no-such-relayd"));
	let Started::FailedToStart { reason, .. } = daemon::start(&launch).await else {
		panic!("expected FailedToStart");
	};
	assert!(reason.contains("no-such-relayd"), "{reason}");
}

fn mode(launch: &mut Launch, mode: &str) {
	launch.extra_args = vec!["--mode".into(), mode.into()];
}

#[tokio::test]
async fn a_daemon_that_dies_reports_its_log() {
	let home = Home::new();
	let mut launch = home.launch();
	mode(&mut launch, "die");
	let Started::FailedToStart { reason, log_tail } = daemon::start(&launch).await else {
		panic!("expected FailedToStart");
	};
	assert!(reason.contains("exited early"), "{reason}");
	assert!(log_tail.contains("boom"), "{log_tail}");
}

#[tokio::test]
async fn a_daemon_that_never_binds_is_reported_and_not_killed() {
	let home = Home::new();
	let mut launch = home.launch();
	mode(&mut launch, "hang");
	launch.timeout = Duration::from_millis(400);
	let Started::FailedToStart { reason, .. } = daemon::start(&launch).await else {
		panic!("expected FailedToStart");
	};
	// It may only be slow (or waiting for the user): it is left running, never killed.
	assert!(reason.contains("did not answer"), "{reason}");
	assert!(reason.contains("left running"), "{reason}");
	assert!(!daemon::reachable(&launch.endpoint).await);
}

#[tokio::test]
async fn a_slow_daemon_is_waited_for() {
	let home = Home::new();
	let mut launch = home.launch();
	mode(&mut launch, "slow");
	assert!(matches!(
		daemon::start(&launch).await,
		Started::Launched { .. }
	));
}

#[tokio::test]
async fn stopping_nothing_is_not_an_error() {
	let home = Home::new();
	let stopped = daemon::stop(
		&Endpoint::Unix(home.socket()),
		"test",
		Duration::from_secs(1),
	)
	.await;
	assert_eq!(stopped.expect("ok"), daemon::Stopped::NotRunning);
}
