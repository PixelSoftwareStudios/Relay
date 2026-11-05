//! Starting and quitting the standalone daemon from the UI: only on request,
//! confirmed for quitting, and never mixed up with quitting the app.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use relay_client::daemon::Started;
use relay_ui::actions::{Actions, Shared};
use relay_ui::controller::{Controller, SessionSlot};
use relay_ui::daemon_control::start_message;
use relay_ui::hub::Hub;
use relay_ui::shell::{BoxFuture, FilePicker, PickedFile, Shell};

#[derive(Default)]
struct FakeShell {
	starts: AtomicUsize,
	stops: AtomicUsize,
	quits: AtomicUsize,
	fail: std::sync::atomic::AtomicBool,
}

impl FilePicker for FakeShell {
	fn pick_image(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(async { Ok(None) })
	}
}

impl Shell for FakeShell {
	fn copy_text(&self, _: &str) {}
	fn request_repaint(&self) {}
	fn window_visible(&self) -> bool {
		true
	}
	fn show_window(&self) {}
	fn hide_window(&self) {}
	fn quit(&self) {
		self.quits.fetch_add(1, Ordering::SeqCst);
	}
	fn hides_to_tray(&self) -> bool {
		false
	}
	fn can_manage_daemon(&self) -> bool {
		true
	}
	fn start_daemon(&self) -> BoxFuture<'_, Result<String, String>> {
		Box::pin(async {
			self.starts.fetch_add(1, Ordering::SeqCst);
			tokio::time::sleep(Duration::from_millis(50)).await;
			if self.fail.load(Ordering::SeqCst) {
				Err("Cannot find the Relay daemon".to_owned())
			} else {
				Ok("The Relay daemon started.".to_owned())
			}
		})
	}
	fn stop_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
		Box::pin(async {
			self.stops.fetch_add(1, Ordering::SeqCst);
			Ok(())
		})
	}
}

fn rig() -> (Arc<FakeShell>, Actions) {
	let hub = Arc::new(Hub::default());
	let shell = Arc::new(FakeShell::default());
	let actions = Actions {
		controller: Arc::new(Controller::new(
			hub.clone(),
			Arc::new(SessionSlot::default()),
		)),
		hub,
		shared: Arc::new(Shared::default()),
		runtime: tokio::runtime::Handle::current(),
		shell: shell.clone(),
	};
	(shell, actions)
}

async fn settled(actions: &Actions) {
	for _ in 0..200 {
		if !actions.shared.daemon_busy.load(Ordering::SeqCst) {
			return;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	panic!("the daemon job never finished");
}

#[tokio::test]
async fn starting_runs_once_however_often_it_is_clicked_and_reports_back() {
	let (shell, actions) = rig();
	actions.start_daemon();
	actions.start_daemon();
	actions.start_daemon();
	settled(&actions).await;
	assert_eq!(shell.starts.load(Ordering::SeqCst), 1);
	assert_eq!(
		actions.hub.state().notice.as_deref(),
		Some("The Relay daemon started.")
	);
	assert_eq!(shell.quits.load(Ordering::SeqCst), 0, "the app stays open");
}

#[tokio::test]
async fn a_failed_start_shows_why() {
	let (shell, actions) = rig();
	shell.fail.store(true, Ordering::SeqCst);
	actions.start_daemon();
	settled(&actions).await;
	assert_eq!(
		actions.shared.error().as_deref(),
		Some("Cannot find the Relay daemon")
	);
}

#[tokio::test]
async fn quitting_the_daemon_is_not_quitting_the_app_and_back() {
	let (shell, actions) = rig();
	actions.quit_daemon();
	settled(&actions).await;
	assert_eq!(shell.stops.load(Ordering::SeqCst), 1);
	assert_eq!(shell.quits.load(Ordering::SeqCst), 0);

	actions.quit_app();
	assert_eq!(shell.quits.load(Ordering::SeqCst), 1);
	assert_eq!(
		shell.stops.load(Ordering::SeqCst),
		1,
		"closing the app leaves the daemon"
	);
}

#[test]
fn start_results_become_plain_messages() {
	use std::path::PathBuf;
	let launched = Started::Launched {
		pid: 1,
		binary: PathBuf::from("relayd"),
		log: PathBuf::from("relayd.log"),
	};
	assert!(start_message(launched).is_ok());
	assert!(start_message(Started::AlreadyRunning).is_ok());
	let missing = start_message(Started::BinaryNotFound { searched: vec![] }).expect_err("error");
	assert!(
		missing.contains("relayd") && missing.contains("RELAY_DAEMON_BIN"),
		"{missing}"
	);
	let failed = start_message(Started::FailedToStart {
		reason: "the daemon exited early (exit status: 3)".to_owned(),
		log_tail: "line one\nboom: cannot bind".to_owned(),
	})
	.expect_err("error");
	assert!(
		failed.contains("exited early") && failed.contains("boom"),
		"{failed}"
	);
}
