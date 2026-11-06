//! The call history follows the engine: it is read when the app connects and again whenever
//! the engine's history revision moves, and not otherwise.

mod common;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Duration;

use common::*;
use relay_api::pb;
use relay_ui::controller::SessionSlot;
use relay_ui::hub::Hub;
use relay_ui::model::{HistoryDirection, HistoryEntry, HistoryOutcome};
use relay_ui::supervisor::supervise;

struct Passive;

impl relay_ui::shell::FilePicker for Passive {
	fn pick_image(
		&self,
	) -> relay_ui::shell::BoxFuture<'_, Result<Option<relay_ui::shell::PickedFile>, String>> {
		Box::pin(async { Ok(None) })
	}
}

impl relay_ui::shell::Shell for Passive {
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
}

fn entry(id: u64, outcome: HistoryOutcome) -> HistoryEntry {
	HistoryEntry {
		id,
		call_id: hex::encode([id as u8; 16]),
		peer_id: hex::encode(PEER),
		peer_name: "Ada".into(),
		fingerprint_short: String::new(),
		direction: HistoryDirection::Incoming,
		outcome,
		started_at_ms: 1_793_880_000_000,
		duration_ms: 0,
	}
}

fn settings(revision: u64) -> pb::Settings {
	pb::Settings {
		history_enabled: true,
		history_revision: revision,
		..Default::default()
	}
}

async fn settle() {
	tokio::time::sleep(Duration::from_millis(50)).await;
}

#[tokio::test(start_paused = true)]
async fn the_list_is_read_on_connect_and_after_each_change_of_revision() {
	let engine = Arc::new(ScriptedEngine::default());
	let hub = Arc::new(Hub::default());
	let slot = Arc::new(SessionSlot::default());
	tokio::spawn(supervise(
		engine.clone(),
		hub.clone(),
		slot,
		Arc::new(Passive),
		Arc::new(relay_ui::autostart::AutoStart::new(true)),
	));
	let mut first = snapshot(1, vec![pb_call(pb::CallState::Connected, Some((&ME, 1)))]);
	first.settings = Some(settings(3));
	let (conn, feed) = connection(first, 1);
	*feed.ops.history.lock().unwrap() = vec![entry(1, HistoryOutcome::Completed)];
	engine.push(Ok(conn));
	settle().await;
	assert_eq!(hub.state().history.len(), 1, "read once on connect");
	assert_eq!(feed.ops.history_reads.load(Ordering::SeqCst), 1);

	// The call ends: the engine records it and moves the revision with a settings event.
	*feed.ops.history.lock().unwrap() = vec![
		entry(2, HistoryOutcome::Missed),
		entry(1, HistoryOutcome::Completed),
	];
	let ended = pb_call(pb::CallState::Ended, Some((&ME, 1)));
	feed.watch
		.send(Ok(Some(event(2, pb::state_event::Event::Call(ended)))))
		.unwrap();
	settle().await;
	assert_eq!(
		feed.ops.history_reads.load(Ordering::SeqCst),
		1,
		"a call event alone does not read it again"
	);
	feed.watch
		.send(Ok(Some(event(
			3,
			pb::state_event::Event::Settings(settings(4)),
		))))
		.unwrap();
	settle().await;
	let state = hub.state();
	assert_eq!(state.history.len(), 2);
	assert_eq!(state.history[0].outcome, HistoryOutcome::Missed);
	assert_eq!(feed.ops.history_reads.load(Ordering::SeqCst), 2);
	assert_eq!(state.settings.unwrap().history_revision, 4);

	// A clear moves the revision too and empties the list.
	feed.ops.history.lock().unwrap().clear();
	feed.watch
		.send(Ok(Some(event(
			4,
			pb::state_event::Event::Settings(settings(5)),
		))))
		.unwrap();
	settle().await;
	assert!(hub.state().history.is_empty());
}

#[tokio::test(start_paused = true)]
async fn the_list_is_kept_when_the_connection_drops() {
	let engine = Arc::new(ScriptedEngine::default());
	let hub = Arc::new(Hub::default());
	tokio::spawn(supervise(
		engine.clone(),
		hub.clone(),
		Arc::new(SessionSlot::default()),
		Arc::new(Passive),
		Arc::new(relay_ui::autostart::AutoStart::new(true)),
	));
	let mut first = snapshot(1, vec![]);
	first.settings = Some(settings(1));
	let (conn, feed) = connection(first, 1);
	*feed.ops.history.lock().unwrap() = vec![entry(1, HistoryOutcome::Completed)];
	engine.push(Ok(conn));
	settle().await;
	feed.watch.send(Ok(None)).unwrap();
	settle().await;
	assert_eq!(hub.state().history.len(), 1);
}

#[tokio::test]
async fn clearing_and_the_switch_go_through_the_controller() {
	let (controller, _hub, ops) = controller_with(snapshot(1, vec![]), 1);
	controller.clear_call_history().await.unwrap();
	assert_eq!(
		ops.recorded(),
		[relay_ui::engine::Command::ClearCallHistory]
	);
	let results = controller.set_history_enabled(false).await.unwrap();
	assert_eq!(results.len(), 1);
	assert_eq!(results[0].key, "history.enabled");
	assert_eq!(
		*ops.settings_sent.lock().unwrap(),
		[vec![("history.enabled".to_owned(), "false".to_owned())]]
	);
}
