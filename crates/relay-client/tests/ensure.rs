#![cfg(unix)]
//! `ensure_running`: one start for many callers, honest failures.

use std::path::PathBuf;
use std::time::Duration;

use relay_api::Endpoint;
use relay_client::daemon::{self, Ensure, EnsureError, StartOptions};

const FAKE: &str = env!("CARGO_BIN_EXE_fake-relayd");

struct Home {
	dir: tempfile::TempDir,
}

impl Home {
	fn new() -> Self {
		Self {
			dir: tempfile::Builder::new()
				.prefix("rlye")
				.tempdir_in("/tmp")
				.expect("dir"),
		}
	}
	fn socket(&self) -> PathBuf {
		self.dir.path().join("e.sock")
	}
	fn endpoint(&self) -> Endpoint {
		Endpoint::Unix(self.socket())
	}
	fn spawn_log(&self) -> PathBuf {
		self.dir.path().join("spawns")
	}
	fn spawns(&self) -> usize {
		std::fs::read_to_string(self.spawn_log()).map_or(0, |text| text.lines().count())
	}
	fn options(&self, mode: &str) -> StartOptions {
		StartOptions {
			binary: Some(PathBuf::from(FAKE)),
			log_file: Some(self.dir.path().join("relayd.log")),
			extra_args: vec![
				"--mode".into(),
				mode.into(),
				"--spawn-log".into(),
				self.spawn_log().into(),
			],
			idle_exit: None,
			ready_timeout: Duration::from_secs(5),
			lock_timeout: Duration::from_secs(10),
		}
	}
}

impl Drop for Home {
	fn drop(&mut self) {
		let _ = std::fs::remove_file(self.socket());
	}
}

#[test]
fn many_clients_at_once_start_exactly_one_daemon() {
	let home = Home::new();
	let started: Vec<Ensure> = std::thread::scope(|scope| {
		let jobs: Vec<_> = (0..8)
			.map(|_| {
				scope.spawn(|| {
					tokio::runtime::Builder::new_current_thread()
						.enable_all()
						.build()
						.expect("runtime")
						.block_on(daemon::ensure_running(
							&home.endpoint(),
							&home.options("slow"),
						))
						.expect("ensured")
				})
			})
			.collect();
		jobs.into_iter()
			.map(|job| job.join().expect("thread"))
			.collect()
	});
	let launched = started
		.iter()
		.filter(|e| matches!(e, Ensure::Started { .. }))
		.count();
	assert_eq!(launched, 1, "{started:?}");
	assert_eq!(home.spawns(), 1, "exactly one process was spawned");
	assert!(daemon::lock_path(&home.endpoint()).exists());
}

#[tokio::test]
async fn a_running_daemon_is_left_alone() {
	let home = Home::new();
	let options = home.options("ok");
	assert!(matches!(
		daemon::ensure_running(&home.endpoint(), &options).await,
		Ok(Ensure::Started { .. })
	));
	assert_eq!(
		daemon::ensure_running(&home.endpoint(), &options).await,
		Ok(Ensure::AlreadyRunning)
	);
	assert_eq!(home.spawns(), 1);
}

#[tokio::test]
async fn a_lock_whose_holder_died_does_not_block_anyone() {
	let home = Home::new();
	let lock = daemon::lock_path(&home.endpoint());
	let mut holder = std::process::Command::new(FAKE)
		.args(["--mode", "holdlock", "--lock"])
		.arg(&lock)
		.stdout(std::process::Stdio::piped())
		.spawn()
		.expect("holder");
	// Wait until it really holds the lock.
	let mut line = String::new();
	std::io::BufRead::read_line(
		&mut std::io::BufReader::new(holder.stdout.take().expect("stdout")),
		&mut line,
	)
	.expect("line");
	assert!(line.contains("holding"));

	let mut options = home.options("ok");
	options.lock_timeout = Duration::from_millis(400);
	assert_eq!(
		daemon::ensure_running(&home.endpoint(), &options).await,
		Err(EnsureError::LockTimeout),
		"a live holder is waited for"
	);
	assert_eq!(home.spawns(), 0);

	holder.kill().expect("kill");
	holder.wait().expect("reap");
	options.lock_timeout = Duration::from_secs(5);
	assert!(matches!(
		daemon::ensure_running(&home.endpoint(), &options).await,
		Ok(Ensure::Started { .. })
	));
}

#[tokio::test]
async fn a_missing_binary_is_reported() {
	let home = Home::new();
	let mut options = home.options("ok");
	options.binary = Some(home.dir.path().join("no-such-relayd"));
	let error = daemon::ensure_running(&home.endpoint(), &options)
		.await
		.expect_err("no binary");
	let EnsureError::BinaryNotFound { searched } = &error else {
		panic!("{error:?}");
	};
	assert_eq!(searched[0], home.dir.path().join("no-such-relayd"));
	assert!(error.to_string().contains("RELAY_DAEMON_BIN"));
}

#[tokio::test]
async fn a_failed_start_carries_the_log_tail() {
	let home = Home::new();
	let error = daemon::ensure_running(&home.endpoint(), &home.options("die"))
		.await
		.expect_err("dies");
	let EnsureError::FailedToStart { reason, log_tail } = &error else {
		panic!("{error:?}");
	};
	assert!(reason.contains("exited early"), "{reason}");
	assert!(log_tail.contains("boom"), "{log_tail}");
	assert!(error.to_string().contains("boom"));
	// The lock was released: the next caller is not stuck behind the failure.
	let again = daemon::ensure_running(&home.endpoint(), &home.options("ok")).await;
	assert!(matches!(again, Ok(Ensure::Started { .. })), "{again:?}");
}

#[tokio::test]
async fn an_auto_started_daemon_is_told_to_exit_when_unused() {
	let home = Home::new();
	let mut options = home.options("ok");
	options.idle_exit = Some(Duration::from_secs(7));
	daemon::ensure_running(&home.endpoint(), &options)
		.await
		.expect("started");
	let log = std::fs::read_to_string(home.spawn_log()).expect("log");
	assert!(log.contains("idle=7"), "{log}");
}

#[test]
fn the_idle_exit_of_an_auto_started_daemon_can_be_overridden() {
	assert_eq!(
		daemon::idle_exit_from(None),
		Some(daemon::DEFAULT_IDLE_EXIT)
	);
	assert_eq!(
		daemon::idle_exit_from(Some("60")),
		Some(Duration::from_secs(60))
	);
	assert_eq!(daemon::idle_exit_from(Some("0")), None, "0 turns it off");
	assert_eq!(
		daemon::idle_exit_from(Some("soon")),
		Some(daemon::DEFAULT_IDLE_EXIT)
	);
}
