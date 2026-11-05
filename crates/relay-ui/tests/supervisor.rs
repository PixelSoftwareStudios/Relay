mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use relay_api::{WatchItem, pb};
use relay_ui::controller::SessionSlot;
use relay_ui::engine::EngineError;
use relay_ui::hub::Hub;
use relay_ui::model::{Link, Phase};
use relay_ui::supervisor::{backoff, supervise};

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

struct Rig {
	engine: Arc<ScriptedEngine>,
	hub: Arc<Hub>,
	slot: Arc<SessionSlot>,
}

fn rig() -> Rig {
	let engine = Arc::new(ScriptedEngine::default());
	let hub = Arc::new(Hub::default());
	let slot = Arc::new(SessionSlot::default());
	tokio::spawn(supervise(
		engine.clone(),
		hub.clone(),
		slot.clone(),
		Arc::new(Passive),
		Arc::new(relay_ui::autostart::AutoStart::new(true)),
	));
	Rig { engine, hub, slot }
}

async fn settle() {
	tokio::time::sleep(Duration::from_millis(10)).await;
}

#[test]
fn backoff_doubles_then_caps() {
	assert_eq!(backoff(0), Duration::from_millis(250));
	assert_eq!(backoff(2), Duration::from_secs(1));
	assert_eq!(backoff(10), Duration::from_secs(5));
	assert_eq!(backoff(500), Duration::from_secs(5));
}

#[tokio::test(start_paused = true)]
async fn reports_offline_when_the_daemon_is_absent() {
	let r = rig();
	settle().await;
	let state = r.hub.state();
	assert_eq!(state.link, Link::Offline);
	assert!(state.link_error.unwrap().contains("not reachable"));
	assert!(!r.slot.is_connected());
}

#[tokio::test(start_paused = true)]
async fn mirrors_snapshot_events_and_meters() {
	let r = rig();
	let (conn, feed) = connection(
		snapshot(10, vec![pb_call(pb::CallState::IncomingRinging, None)]),
		1,
	);
	r.engine.push(Ok(conn));
	tokio::time::sleep(Duration::from_millis(300)).await;

	let state = r.hub.state();
	assert_eq!(state.link, Link::Connected);
	assert_eq!(state.engine.as_ref().unwrap().version, "9.9");
	assert_eq!(state.calls[0].phase, Phase::IncomingRinging);

	let updated = pb_call(pb::CallState::Connected, Some((&ME, 4)));
	feed.watch
		.send(Ok(Some(event(11, pb::state_event::Event::Call(updated)))))
		.unwrap();
	feed.telemetry
		.send(pb::TelemetrySnapshot {
			microphone_dbfs: Some(-20.0),
			calls: vec![pb::CallTelemetry {
				call_id: CALL.to_vec(),
				speaking: true,
				receive_dbfs: Some(f32::NAN),
				..Default::default()
			}],
			..Default::default()
		})
		.unwrap();
	settle().await;

	let state = r.hub.state();
	assert_eq!(state.revision, 11);
	assert_eq!(state.calls[0].phase, Phase::Connected);
	assert!(state.calls[0].controlled_by_me);
	let meters = r.hub.meters();
	assert_eq!(meters.microphone_dbfs, Some(-20.0));
	assert!(meters.calls[0].speaking);
	assert_eq!(
		meters.calls[0].receive_dbfs, None,
		"non-finite readings become no data"
	);
}

#[tokio::test(start_paused = true)]
async fn resync_after_gap_replaces_state_from_a_fresh_snapshot() {
	let r = rig();
	let (first, feed) = connection(
		snapshot(10, vec![pb_call(pb::CallState::Connected, None)]),
		1,
	);
	r.engine.push(Ok(first));
	tokio::time::sleep(Duration::from_millis(300)).await;

	let (second, _feed2) = connection(snapshot(50, vec![]), 1);
	r.engine.push(Ok(second));
	feed.watch
		.send(Err(EngineError::Broken("revision gap".into())))
		.unwrap();
	settle().await;
	assert_eq!(r.hub.state().link, Link::Reconnecting);
	assert!(
		r.hub.state().calls.is_empty(),
		"stale calls are not shown while down"
	);
	assert!(!r.slot.is_connected(), "no commands while disconnected");

	tokio::time::sleep(Duration::from_secs(1)).await;
	let state = r.hub.state();
	assert_eq!(state.link, Link::Connected);
	assert_eq!(state.revision, 50);
	assert!(state.calls.is_empty());
}

#[tokio::test(start_paused = true)]
async fn daemon_restart_reconnects_and_engine_resync_request_is_honoured() {
	let r = rig();
	let (first, feed) = connection(snapshot(1, vec![]), 1);
	r.engine.push(Ok(first));
	tokio::time::sleep(Duration::from_millis(300)).await;
	// Daemon dies: stream closes, then connects fail for a while.
	drop(feed);
	settle().await;
	assert_eq!(r.hub.state().link, Link::Reconnecting);
	tokio::time::sleep(Duration::from_millis(600)).await;
	assert_eq!(r.hub.state().link, Link::Offline);

	let (second, feed2) = connection(snapshot(2, vec![]), 1);
	r.engine.push(Ok(second));
	tokio::time::sleep(Duration::from_secs(6)).await;
	assert_eq!(r.hub.state().link, Link::Connected);

	let resync = WatchItem::Resync(pb::ResyncRequired {
		current_revision: 9,
		reason: "behind".into(),
	});
	let (third, _f3) = connection(snapshot(9, vec![]), 1);
	r.engine.push(Ok(third));
	feed2.watch.send(Ok(Some(resync))).unwrap();
	tokio::time::sleep(Duration::from_secs(1)).await;
	assert_eq!(r.hub.state().revision, 9);
}

struct Starter {
	starts: std::sync::atomic::AtomicUsize,
}

impl relay_ui::shell::FilePicker for Starter {
	fn pick_image(
		&self,
	) -> relay_ui::shell::BoxFuture<'_, Result<Option<relay_ui::shell::PickedFile>, String>> {
		Box::pin(async { Ok(None) })
	}
}

impl relay_ui::shell::Shell for Starter {
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
	fn start_daemon(&self) -> relay_ui::shell::BoxFuture<'_, Result<String, String>> {
		Box::pin(async {
			self.starts
				.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
			Ok("started".to_owned())
		})
	}
}

#[tokio::test(start_paused = true)]
async fn an_unreachable_daemon_is_started_but_never_after_a_user_quit_or_a_crash_loop() {
	let engine = Arc::new(ScriptedEngine::default());
	let hub = Arc::new(Hub::default());
	let slot = Arc::new(SessionSlot::default());
	let shell = Arc::new(Starter {
		starts: Default::default(),
	});
	let policy = Arc::new(relay_ui::autostart::AutoStart::new(false));
	tokio::spawn(supervise(
		engine.clone(),
		hub.clone(),
		slot,
		shell.clone(),
		policy.clone(),
	));
	// The scripted engine reports the daemon down forever: three starts, then
	// the supervisor gives up and says why.
	tokio::time::sleep(Duration::from_secs(30)).await;
	let starts = || shell.starts.load(std::sync::atomic::Ordering::SeqCst);
	assert_eq!(starts(), 3);
	assert!(policy.paused());
	let state = hub.state();
	assert_eq!(state.link, Link::Offline);
	assert!(
		state
			.link_error
			.as_deref()
			.is_some_and(|e| e.contains("keeps stopping"))
	);
	tokio::time::sleep(Duration::from_secs(120)).await;
	assert_eq!(starts(), 3, "no further automatic starts");
}
