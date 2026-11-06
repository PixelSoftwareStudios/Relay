//! The decision to replace a stale or idle daemon, with a fake daemon.

use relay_api::Revision;
use relay_client::refresh::{Daemon, Outcome, Policy, Probe, RefreshError, refresh};

#[derive(Default)]
struct Fake {
	probe: Option<Probe>,
	stop_error: Option<String>,
	start_error: Option<String>,
	log: Vec<&'static str>,
}

impl Daemon for Fake {
	async fn probe(&mut self) -> Result<Option<Probe>, String> {
		self.log.push("probe");
		Ok(self.probe.clone())
	}
	async fn stop(&mut self) -> Result<(), String> {
		self.log.push("stop");
		self.stop_error.clone().map_or(Ok(()), Err)
	}
	async fn start(&mut self) -> Result<(), String> {
		self.log.push("start");
		self.start_error.clone().map_or(Ok(()), Err)
	}
}

fn current(busy: bool) -> Probe {
	Probe {
		revision: Revision::local(),
		busy,
		starting: false,
	}
}

fn stale(busy: bool) -> Probe {
	Probe {
		revision: Revision {
			build: "0ld0ld0ld0ld".into(),
			api: 1,
		},
		busy,
		starting: false,
	}
}

fn daemon(probe: Option<Probe>) -> Fake {
	Fake {
		probe,
		..Fake::default()
	}
}

#[tokio::test]
async fn nothing_running_is_left_to_the_normal_start() {
	let mut d = daemon(None);
	assert_eq!(
		refresh(&mut d, Policy::AnyIdle).await,
		Ok(Outcome::NotRunning)
	);
	assert_eq!(d.log, ["probe"]);
}

#[tokio::test]
async fn an_idle_stale_daemon_is_replaced_by_the_cli_policy() {
	let mut d = daemon(Some(stale(false)));
	let outcome = refresh(&mut d, Policy::StaleOnly).await.unwrap();
	assert!(matches!(outcome, Outcome::Restarted { .. }));
	assert_eq!(d.log, ["probe", "stop", "start"]);
}

#[tokio::test]
async fn a_matching_idle_daemon_is_kept_by_the_cli_policy() {
	let mut d = daemon(Some(current(false)));
	assert_eq!(
		refresh(&mut d, Policy::StaleOnly).await,
		Ok(Outcome::Current)
	);
	assert_eq!(d.log, ["probe"]);
}

#[tokio::test]
async fn the_desktop_policy_replaces_any_idle_daemon() {
	let mut d = daemon(Some(current(false)));
	let outcome = refresh(&mut d, Policy::AnyIdle).await.unwrap();
	assert_eq!(
		outcome,
		Outcome::Restarted {
			from: Revision::local()
		}
	);
	assert_eq!(d.log, ["probe", "stop", "start"]);
}

#[tokio::test]
async fn a_daemon_with_a_call_is_never_touched() {
	for (probe, policy) in [
		(stale(true), Policy::StaleOnly),
		(current(true), Policy::AnyIdle),
	] {
		let mut d = daemon(Some(probe));
		let outcome = refresh(&mut d, policy).await.unwrap();
		assert!(matches!(outcome, Outcome::KeptBusy { .. }), "{outcome:?}");
		assert_eq!(d.log, ["probe"], "no stop, no start");
	}
}

#[tokio::test]
async fn a_busy_current_daemon_is_not_even_reported_by_the_cli_policy() {
	let mut d = daemon(Some(current(true)));
	assert_eq!(
		refresh(&mut d, Policy::StaleOnly).await,
		Ok(Outcome::Current)
	);
}

#[tokio::test]
async fn a_daemon_that_will_not_stop_is_reported_and_no_new_one_is_started() {
	let mut d = daemon(Some(current(false)));
	d.stop_error = Some("operation timed out".into());
	let error = refresh(&mut d, Policy::AnyIdle).await.unwrap_err();
	assert!(matches!(error, RefreshError::StopTimedOut(_)));
	assert_eq!(d.log, ["probe", "stop"], "it does not loop or start twice");
}

#[tokio::test]
async fn a_failed_start_is_reported() {
	let mut d = daemon(Some(stale(false)));
	d.start_error = Some("no relayd".into());
	let error = refresh(&mut d, Policy::StaleOnly).await.unwrap_err();
	assert_eq!(error, RefreshError::Start("no relayd".into()));
}

#[cfg(unix)]
#[tokio::test]
async fn a_client_holding_the_start_lock_makes_the_refresh_wait_then_give_way() {
	use relay_api::Endpoint;
	use relay_client::daemon::{StartOptions, lock_path};
	use relay_io::lock::ProcessLock;
	use std::time::Duration;

	let dir = tempfile::tempdir().unwrap();
	let endpoint = Endpoint::Unix(dir.path().join("e.sock"));
	let _held = ProcessLock::acquire_beside(&lock_path(&endpoint)).unwrap();
	let options = StartOptions {
		lock_timeout: Duration::from_millis(150),
		..StartOptions::default()
	};
	let outcome = relay_client::refresh::refresh_endpoint(
		&endpoint,
		&options,
		"test",
		Policy::AnyIdle,
		Duration::from_secs(1),
	)
	.await
	.unwrap();
	assert_eq!(outcome, Outcome::Contended);
}
