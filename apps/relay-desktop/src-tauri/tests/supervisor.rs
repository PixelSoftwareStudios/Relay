mod common;

use std::sync::Arc;
use std::time::Duration;

use common::*;
use relay_api::{WatchItem, pb};
use relay_desktop_lib::controller::SessionSlot;
use relay_desktop_lib::engine::EngineError;
use relay_desktop_lib::hub::Hub;
use relay_desktop_lib::model::{Link, Phase};
use relay_desktop_lib::supervisor::{backoff, supervise};

struct Rig {
	engine: Arc<ScriptedEngine>,
	hub: Arc<Hub>,
	slot: Arc<SessionSlot>,
}

fn rig() -> Rig {
	let engine = Arc::new(ScriptedEngine::default());
	let hub = Arc::new(Hub::default());
	let slot = Arc::new(SessionSlot::default());
	tokio::spawn(supervise(engine.clone(), hub.clone(), slot.clone()));
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
