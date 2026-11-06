#![cfg(unix)]
//! A daemon that answers but is still opening its secure storage is waited for, never killed,
//! restarted or started twice.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use relay_api::pb::{StartupReason, StartupState};
use relay_api::{Endpoint, RelayClient};
use relay_client::daemon::{self, Ensure, EnsureError, NotReady, StartOptions};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-relayd");

struct Home {
	dir: tempfile::TempDir,
}

impl Home {
	fn new() -> Self {
		Self {
			dir: tempfile::Builder::new()
				.prefix("rlys")
				.tempdir_in("/tmp")
				.expect("dir"),
		}
	}
	fn endpoint(&self) -> Endpoint {
		Endpoint::Unix(self.dir.path().join("e.sock"))
	}
	fn spawns(&self) -> usize {
		std::fs::read_to_string(self.dir.path().join("spawns")).map_or(0, |t| t.lines().count())
	}
	fn options(&self, mode: &str, extra: &[&str]) -> StartOptions {
		let mut args: Vec<std::ffi::OsString> = vec![
			"--mode".into(),
			mode.into(),
			"--spawn-log".into(),
			self.dir.path().join("spawns").into(),
		];
		args.extend(extra.iter().map(Into::into));
		StartOptions {
			binary: Some(PathBuf::from(FAKE)),
			log_file: Some(self.dir.path().join("relayd.log")),
			extra_args: args,
			idle_exit: None,
			ready_timeout: Duration::from_secs(5),
			startup_timeout: Duration::from_secs(20),
			lock_timeout: Duration::from_secs(10),
		}
	}
}

impl Drop for Home {
	fn drop(&mut self) {
		let _ = std::fs::remove_file(self.dir.path().join("e.sock"));
	}
}

#[tokio::test]
async fn the_launcher_waits_while_the_daemon_waits_for_the_keychain() {
	let home = Home::new();
	let options = home.options("starting", &["--starting-ms", "1500"]);
	let begun = Instant::now();
	let mut seen = Vec::new();
	let ensured = daemon::ensure_running_with(&home.endpoint(), &options, &mut |startup| {
		seen.push((startup.state(), startup.reason()));
	})
	.await;
	assert!(matches!(ensured, Ok(Ensure::Started { .. })), "{ensured:?}");
	assert!(
		begun.elapsed() >= Duration::from_millis(1400),
		"returned before the block ended"
	);
	assert_eq!(
		seen,
		[(StartupState::Starting, StartupReason::WaitingForKeychain)]
	);
	assert_eq!(home.spawns(), 1, "one daemon process, never restarted");
}

#[tokio::test]
async fn a_second_attempt_attaches_to_the_starting_daemon() {
	let home = Home::new();
	let options = home.options("starting", &["--starting-ms", "2000"]);
	let first = tokio::spawn({
		let (endpoint, options) = (home.endpoint(), options.clone());
		async move { daemon::ensure_running(&endpoint, &options).await }
	});
	// Wait until the first attempt's daemon answers, still starting.
	while !daemon::reachable(&home.endpoint()).await {
		tokio::time::sleep(Duration::from_millis(20)).await;
	}
	let second = daemon::ensure_running(&home.endpoint(), &options).await;
	assert_eq!(second, Ok(Ensure::AlreadyRunning));
	assert!(matches!(
		first.await.expect("join"),
		Ok(Ensure::Started { .. })
	));
	assert_eq!(home.spawns(), 1);
}

#[tokio::test]
async fn a_failed_start_reports_the_reason_and_leaves_the_daemon_up() {
	let home = Home::new();
	let options = home.options("failed", &[]);
	let error = daemon::ensure_running(&home.endpoint(), &options)
		.await
		.expect_err("failed");
	let EnsureError::NotReady(NotReady::Failed { reason, detail }) = &error else {
		panic!("{error:?}");
	};
	assert!(reason.contains("keychain"), "{reason}");
	assert_eq!(detail, "user denied");
	assert!(error.to_string().contains("relay daemon retry"));
	assert!(daemon::reachable(&home.endpoint()).await, "still up");

	// Retry (the user allowed it meanwhile): the same daemon becomes ready, no new process.
	let mut client = RelayClient::connect(&home.endpoint())
		.await
		.expect("connect");
	client.retry_keychain().await.expect("retry");
	assert_eq!(
		daemon::ensure_running(&home.endpoint(), &options).await,
		Ok(Ensure::AlreadyRunning)
	);
	assert_eq!(home.spawns(), 1);
}

#[tokio::test]
async fn running_out_of_patience_leaves_the_daemon_running() {
	let home = Home::new();
	let mut options = home.options("starting", &["--starting-ms", "60000"]);
	options.startup_timeout = Duration::from_millis(600);
	let error = daemon::ensure_running(&home.endpoint(), &options)
		.await
		.expect_err("times out");
	assert_eq!(error, EnsureError::NotReady(NotReady::TimedOut));
	assert!(daemon::reachable(&home.endpoint()).await, "not killed");
	assert_eq!(home.spawns(), 1);
}

#[tokio::test]
async fn a_client_cannot_register_while_the_daemon_is_starting() {
	let home = Home::new();
	let options = home.options("starting", &["--starting-ms", "60000"]);
	let mut short = options.clone();
	short.startup_timeout = Duration::from_millis(300);
	let _ = daemon::ensure_running(&home.endpoint(), &short).await;
	let mut client = RelayClient::connect(&home.endpoint())
		.await
		.expect("connect");
	let error = client.register("t", "0", &[]).await.expect_err("starting");
	assert!(
		matches!(error, relay_api::ApiError::Starting(_)),
		"{error:?}"
	);
}
