//! A background service that waits for the system's secure storage is shown calmly, never
//! restarted, and offers Try again / file storage when opening failed.

mod common;
mod phone;

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use common::ScriptedEngine;
use phone::{Drawn, Phone};
use relay_api::Revision;
use relay_client::refresh::{Daemon, Outcome, Policy, Probe, refresh};
use relay_ui::controller::SessionSlot;
use relay_ui::daemon_control::{LaunchRefresh, launch_result};
use relay_ui::engine::EngineError;
use relay_ui::hub::Hub;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::{Link, Startup};
use relay_ui::shell::{BoxFuture, FilePicker, PickedFile, Shell};
use relay_ui::supervisor::supervise;

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

fn rig(startup: Startup) -> Phone {
	let phone = Phone::desktop(720.0, 1400.0, Language::En);
	let mut state = phone::busy_state();
	state.calls.clear();
	state.link = Link::Starting;
	state.startup = Some(startup);
	phone.show_state(&state);
	phone
}

fn waiting() -> Startup {
	Startup {
		failed: false,
		unreadable: false,
		detail: String::new(),
	}
}

fn denied() -> Startup {
	Startup {
		failed: true,
		unreadable: false,
		detail: "user denied".into(),
	}
}

#[tokio::test]
async fn waiting_for_the_keychain_is_a_calm_card_without_error_text() {
	let mut phone = rig(waiting());
	let drawn = phone.page(Page::Calls);
	assert!(has(&drawn, "Waiting for permission to use secure storage"));
	assert!(drawn.labels.iter().any(|l| l.contains("system prompt")));
	// No failure text and nothing to press.
	assert!(!drawn.labels.iter().any(|l| l.contains("can't open")));
	assert!(!has(&drawn, "Try again"));
}

#[tokio::test]
async fn a_failure_says_why_and_offers_both_choices() {
	let mut phone = rig(denied());
	let drawn = phone.page(Page::Calls);
	assert!(has(
		&drawn,
		"Relay can't open secure storage: access was not allowed, or it is locked"
	));
	assert!(has(&drawn, "Try again") && has(&drawn, "Use file storage…"));

	phone.click_label("Try again");
	tokio::time::sleep(Duration::from_millis(50)).await;
	assert_eq!(*phone.shell.daemon_log.lock().unwrap(), ["retry"]);
}

#[tokio::test]
async fn file_storage_needs_a_confirmation_that_explains_the_risk() {
	let mut phone = rig(denied());
	phone.page(Page::Calls);
	phone.click_label("Use file storage…");
	let drawn = phone.settle();
	assert!(drawn.labels.iter().any(|l| l.contains("less secure")));
	tokio::time::sleep(Duration::from_millis(50)).await;
	assert!(phone.shell.daemon_log.lock().unwrap().is_empty(), "not yet");

	phone.click_label("Use file storage");
	tokio::time::sleep(Duration::from_millis(50)).await;
	assert_eq!(*phone.shell.daemon_log.lock().unwrap(), ["file-store"]);
}

#[tokio::test]
async fn an_unreadable_identity_offers_no_file_storage() {
	let mut phone = rig(Startup {
		failed: true,
		unreadable: true,
		detail: String::new(),
	});
	let drawn = phone.page(Page::Calls);
	assert!(drawn.labels.iter().any(|l| l.contains("cannot be read")));
	assert!(!has(&drawn, "Use file storage…"));
}

/// A desktop shell that counts how often the app starts the service.
#[derive(Default)]
struct Counting {
	starts: AtomicUsize,
}

impl FilePicker for Counting {
	fn pick_image(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(async { Ok(None) })
	}
}

impl Shell for Counting {
	fn copy_text(&self, _: &str) {}
	fn request_repaint(&self) {}
	fn window_visible(&self) -> bool {
		true
	}
	fn show_window(&self) {}
	fn hide_window(&self) {}
	fn quit(&self) {}
	fn hides_to_tray(&self) -> bool {
		false
	}
	fn can_manage_daemon(&self) -> bool {
		true
	}
	fn start_daemon(&self) -> BoxFuture<'_, Result<String, String>> {
		self.starts.fetch_add(1, Ordering::SeqCst);
		Box::pin(async { Ok(String::new()) })
	}
}

#[tokio::test(start_paused = true)]
async fn the_supervisor_shows_a_starting_service_and_never_starts_another() {
	let engine = Arc::new(ScriptedEngine::default());
	let hub = Arc::new(Hub::default());
	let shell = Arc::new(Counting::default());
	tokio::spawn(supervise(
		engine.clone(),
		hub.clone(),
		Arc::new(SessionSlot::default()),
		shell.clone(),
		Arc::new(relay_ui::autostart::AutoStart::new(false)),
	));
	for _ in 0..3 {
		engine.push(Err(EngineError::Starting(waiting())));
	}
	tokio::time::sleep(Duration::from_millis(100)).await;
	let state = hub.state();
	assert_eq!(state.link, Link::Starting);
	assert_eq!(state.startup, Some(waiting()));
	tokio::time::sleep(Duration::from_millis(1500)).await;
	assert_eq!(
		shell.starts.load(Ordering::SeqCst),
		0,
		"attached, not started"
	);
}

#[derive(Default)]
struct Fake {
	log: Vec<&'static str>,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
struct FakeError(String);

impl From<&str> for FakeError {
	fn from(value: &str) -> Self {
		Self(value.to_owned())
	}
}

impl Daemon for Fake {
	type Error = FakeError;
	async fn probe(&mut self) -> Result<Option<Probe>, Self::Error> {
		self.log.push("probe");
		Ok(Some(Probe {
			revision: Revision::local(),
			busy: false,
			starting: true,
		}))
	}
	async fn stop(&mut self) -> Result<(), Self::Error> {
		self.log.push("stop");
		Ok(())
	}
	async fn start(&mut self) -> Result<(), Self::Error> {
		self.log.push("start");
		Ok(())
	}
}

#[tokio::test]
async fn the_launch_refresh_leaves_a_starting_service_alone() {
	let mut daemon = Fake::default();
	let outcome = refresh(&mut daemon, Policy::AnyIdle).await;
	assert_eq!(outcome, Ok(Outcome::Starting));
	assert_eq!(launch_result(outcome), LaunchRefresh::KeptStarting);
	assert_eq!(daemon.log, ["probe"], "no stop, no start");
}
