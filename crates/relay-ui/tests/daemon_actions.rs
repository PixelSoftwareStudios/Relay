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
				Ok(String::new())
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
		actions.hub.state().notice,
		None,
		"starting on the user's behalf is silent"
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
	assert_eq!(start_message(launched), Ok(String::new()));
	assert_eq!(start_message(Started::AlreadyRunning), Ok(String::new()));
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

fn call(id: &str, phase: relay_ui::model::Phase, mine: bool) -> relay_ui::model::Call {
	relay_ui::model::Call {
		call_id: id.to_owned(),
		peer_id: "p".to_owned(),
		phase,
		transmit: relay_ui::model::Transmit {
			mode: relay_ui::model::Mode::ReceiveOnly,
			consent_granted: false,
			effective_transmitting: false,
			blocked_reason: String::new(),
			releasing: false,
		},
		user_muted: false,
		user_deafened: false,
		receive_gain: 1.0,
		profile: String::new(),
		problem: None,
		controlled_by_me: mine,
		controller_generation: 1,
		connected_at_unix_ms: 0,
	}
}

#[tokio::test]
async fn quitting_the_app_without_calls_quits_at_once() {
	let (shell, actions) = rig();
	actions.quit_app();
	assert_eq!(shell.quits.load(Ordering::SeqCst), 1);
}

#[tokio::test]
async fn quitting_the_app_hangs_up_its_calls_first_then_quits() {
	use relay_ui::model::Phase;
	let (shell, actions) = rig();
	actions.hub.update(|s| {
		s.calls.push(call("a", Phase::Connected, true));
		s.calls.push(call("b", Phase::Connected, false));
	});
	actions.quit_app();
	for _ in 0..300 {
		if shell.quits.load(Ordering::SeqCst) > 0 {
			break;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	assert_eq!(shell.quits.load(Ordering::SeqCst), 1);
}

#[test]
fn only_this_apps_live_calls_count_and_the_texts_mention_them() {
	use relay_ui::format::{close_note, own_live_calls, quit_prompt};
	use relay_ui::model::Phase;
	let calls = vec![
		call("a", Phase::Connected, true),
		call("b", Phase::Connected, false),
		call("c", Phase::Ended, true),
		call("d", Phase::IncomingRinging, true),
	];
	assert_eq!(own_live_calls(&calls), vec!["a", "d"]);
	assert!(quit_prompt(2).contains("2 active calls"));
	assert!(quit_prompt(1).contains("hung up"));
	assert_eq!(quit_prompt(0), "Quit this app?");
	assert!(close_note(true, false, 0).contains("hides it to the tray"));
	assert!(close_note(false, false, 1).contains("quits this app"));
	assert!(close_note(true, false, 3).contains("3 calls are active"));
	assert!(close_note(true, false, 0).contains("hangs up the calls"));
}

#[tokio::test]
async fn a_quit_daemon_pauses_automatic_starts_until_the_user_starts_it() {
	let (_shell, actions) = rig();
	actions.quit_daemon();
	settled(&actions).await;
	assert!(actions.shared.autostart.paused());
	actions.start_daemon();
	settled(&actions).await;
	assert!(!actions.shared.autostart.paused());
}
