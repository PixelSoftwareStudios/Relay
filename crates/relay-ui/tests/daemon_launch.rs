//! Launch-time replacement of an idle daemon, with fakes.

use relay_api::Revision;
use relay_client::refresh::{Daemon, Policy, Probe, refresh};
use relay_ui::actions::Shared;
use relay_ui::autostart::AutoStart;
use relay_ui::daemon_control::{LaunchRefresh, launch_result, restart_idle_at_launch};
use relay_ui::hub::Hub;

#[derive(Default)]
struct Fake {
	probe: Option<Probe>,
	stop_error: Option<String>,
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
		Ok(())
	}
}

fn running(busy: bool) -> Fake {
	Fake {
		probe: Some(Probe {
			revision: Revision::local(),
			busy,
			starting: false,
		}),
		..Fake::default()
	}
}

#[tokio::test]
async fn an_idle_daemon_of_the_same_build_is_still_restarted() {
	let mut d = running(false);
	let result = launch_result(refresh(&mut d, Policy::AnyIdle).await);
	assert_eq!(result, LaunchRefresh::Restarted);
	assert_eq!(d.log, ["probe", "stop", "start"]);
}

#[tokio::test]
async fn a_daemon_with_a_call_is_left_alone() {
	let mut d = running(true);
	let result = launch_result(refresh(&mut d, Policy::AnyIdle).await);
	assert_eq!(result, LaunchRefresh::KeptBusy);
	assert_eq!(d.log, ["probe"]);
}

#[tokio::test]
async fn a_daemon_that_refuses_to_stop_is_reported_once_without_looping() {
	let mut d = running(false);
	d.stop_error = Some("operation timed out".into());
	let result = launch_result(refresh(&mut d, Policy::AnyIdle).await);
	assert!(matches!(&result, LaunchRefresh::Failed(m) if m.contains("did not stop")));
	assert_eq!(d.log, ["probe", "stop"]);
}

#[tokio::test]
async fn no_daemon_means_nothing_to_replace() {
	let mut d = Fake::default();
	let result = launch_result(refresh(&mut d, Policy::AnyIdle).await);
	assert_eq!(result, LaunchRefresh::Nothing);
}

#[cfg(unix)]
#[tokio::test]
async fn automatic_start_off_or_a_quit_leaves_the_daemon_untouched() {
	let dir = tempfile::tempdir().unwrap();
	let endpoint = relay_api::Endpoint::Unix(dir.path().join("e.sock"));
	let (hub, shared) = (Hub::default(), Shared::default());
	for off in [true, false] {
		let autostart = AutoStart::new(off);
		if !off {
			autostart.user_quit();
		}
		let result = restart_idle_at_launch(&endpoint, &autostart, &hub, &shared).await;
		assert_eq!(result, LaunchRefresh::Skipped);
	}
}

#[cfg(unix)]
#[tokio::test]
async fn the_start_lock_held_by_a_cli_makes_the_launch_refresh_do_nothing() {
	use relay_client::daemon::{StartOptions, lock_path};
	use relay_client::refresh::refresh_endpoint;
	use relay_io::lock::ProcessLock;
	use std::time::Duration;

	let dir = tempfile::tempdir().unwrap();
	let endpoint = relay_api::Endpoint::Unix(dir.path().join("e.sock"));
	let _cli = ProcessLock::acquire_beside(&lock_path(&endpoint)).unwrap();
	let options = StartOptions {
		lock_timeout: Duration::from_millis(100),
		..StartOptions::default()
	};
	let result = refresh_endpoint(
		&endpoint,
		&options,
		"t",
		Policy::AnyIdle,
		Duration::from_secs(1),
	)
	.await;
	assert_eq!(launch_result(result), LaunchRefresh::Nothing);
}

#[cfg(unix)]
#[tokio::test(start_paused = true)]
async fn a_kept_daemon_is_replaced_by_itself_when_the_call_ends() {
	use relay_ui::daemon_control::restart_when_idle;
	use relay_ui::model::Phase;
	use std::sync::Arc;
	use std::time::Duration;

	let dir = tempfile::tempdir().unwrap();
	let endpoint = relay_api::Endpoint::Unix(dir.path().join("e.sock"));
	let (hub, shared) = (Arc::new(Hub::default()), Arc::new(Shared::default()));
	hub.update(|s| *s = relay_ui::mock::seed());
	assert!(hub.state().calls.iter().any(|c| !c.phase.is_terminal()));
	let task = {
		let (hub, shared) = (hub.clone(), shared.clone());
		tokio::spawn(async move {
			let autostart = AutoStart::new(false);
			restart_when_idle(&endpoint, &autostart, &hub, &shared).await
		})
	};
	tokio::time::sleep(Duration::from_secs(20)).await;
	assert!(
		!task.is_finished(),
		"a call is still active: nothing happens"
	);
	hub.update(|s| {
		for call in &mut s.calls {
			call.phase = Phase::Ended;
		}
	});
	tokio::time::sleep(Duration::from_secs(5)).await;
	assert_eq!(task.await.unwrap(), LaunchRefresh::Nothing);
}

#[test]
fn the_api_revision_and_release_decide_whether_a_daemon_is_current() {
	let same_api_other_text = Revision {
		build: "abcdef123456-dirty".into(),
		..Revision::local()
	};
	assert!(
		same_api_other_text.is_current(),
		"build text is display only"
	);
	let older = Revision {
		api: Revision::local().api - 1,
		..Revision::local()
	};
	assert!(!older.is_current());
	let ancient = Revision {
		build: String::new(),
		api: 0,
		version: String::new(),
	};
	assert!(
		!ancient.is_current(),
		"a daemon that reports nothing is stale"
	);
}
