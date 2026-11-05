mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::*;
use relay_api::pb;
use relay_ui::emitter::{self, Cadence, Sink, TraySummary, tray_summary};
use relay_ui::hub::Hub;
use relay_ui::model::{AppState, Link, Meters};

#[derive(Default)]
struct Recorder {
	states: Mutex<Vec<u64>>,
	meters: Mutex<Vec<u64>>,
	trays: Mutex<Vec<String>>,
}

impl Sink for Recorder {
	fn state(&self, state: &AppState) {
		self.states.lock().unwrap().push(state.seq);
	}
	fn meters(&self, meters: &Meters) {
		self.meters.lock().unwrap().push(meters.seq);
	}
	fn tray(&self, summary: &TraySummary) {
		self.trays.lock().unwrap().push(summary.tooltip.clone());
	}
}

fn start() -> (Arc<Hub>, Arc<Recorder>) {
	let hub = Arc::new(Hub::default());
	let rec = Arc::new(Recorder::default());
	tokio::spawn(emitter::run(hub.clone(), rec.clone(), Cadence::default()));
	(hub, rec)
}

#[tokio::test(start_paused = true)]
async fn bursts_of_changes_collapse_into_one_emission_per_tick() {
	let (hub, rec) = start();
	tokio::time::sleep(Duration::from_millis(10)).await;
	for revision in 0..500 {
		hub.update(|s| s.revision = revision);
	}
	tokio::time::sleep(Duration::from_millis(150)).await;
	assert_eq!(rec.states.lock().unwrap().len(), 1);
	assert_eq!(hub.state().revision, 499, "the latest value wins");
}

#[tokio::test(start_paused = true)]
async fn emission_rate_is_bounded_at_25_hertz() {
	let (hub, rec) = start();
	for _ in 0..1000 {
		hub.update(|s| s.revision += 1);
		hub.set_meters(Meters {
			microphone_dbfs: Some(-10.0),
			..Meters::default()
		});
		tokio::time::sleep(Duration::from_millis(1)).await;
	}
	let states = rec.states.lock().unwrap().len();
	let meters = rec.meters.lock().unwrap().len();
	assert!(
		(24..=26).contains(&states),
		"1 s of churn gave {states} state events"
	);
	assert!(
		(24..=26).contains(&meters),
		"1 s of churn gave {meters} meter events"
	);
}

#[tokio::test(start_paused = true)]
async fn sequence_numbers_increase_by_one_so_gaps_are_detectable() {
	let (hub, rec) = start();
	for _ in 0..4 {
		hub.update(|s| s.revision += 1);
		tokio::time::sleep(Duration::from_millis(120)).await;
	}
	assert_eq!(*rec.states.lock().unwrap(), vec![1, 2, 3, 4]);
}

#[tokio::test(start_paused = true)]
async fn quiet_hubs_emit_nothing() {
	let (_hub, rec) = start();
	tokio::time::sleep(Duration::from_secs(3)).await;
	assert!(rec.states.lock().unwrap().is_empty());
	assert!(rec.meters.lock().unwrap().is_empty());
}

#[tokio::test(start_paused = true)]
async fn tray_refreshes_at_most_once_a_second_and_only_on_change() {
	let (hub, rec) = start();
	tokio::time::sleep(Duration::from_millis(10)).await;
	assert_eq!(rec.trays.lock().unwrap().len(), 1, "initial text");
	for i in 0..20 {
		hub.update(|s| {
			s.link = if i % 2 == 0 {
				Link::Offline
			} else {
				Link::Reconnecting
			}
		});
		tokio::time::sleep(Duration::from_millis(40)).await;
	}
	let during = rec.trays.lock().unwrap().len();
	assert!(during <= 2, "got {during} tray updates in under a second");
	tokio::time::sleep(Duration::from_secs(3)).await;
	let settled = rec.trays.lock().unwrap().len();
	tokio::time::sleep(Duration::from_secs(3)).await;
	assert_eq!(
		rec.trays.lock().unwrap().len(),
		settled,
		"unchanged text is not resent"
	);
}

#[test]
fn tray_text_reflects_link_and_calls() {
	let mut state = AppState::default();
	assert!(tray_summary(&state).tooltip.contains("connecting"));
	state.link = Link::Offline;
	assert!(tray_summary(&state).tooltip.contains("not running"));
	state.link = Link::Connected;
	assert!(tray_summary(&state).tooltip.contains("idle"));
	state.load_snapshot(&snapshot(
		1,
		vec![pb_call(pb::CallState::IncomingRinging, None)],
	));
	let summary = tray_summary(&state);
	assert_eq!(summary.incoming, 1);
	assert!(summary.tooltip.contains("incoming"));
}

#[tokio::test(start_paused = true)]
async fn hidden_windows_receive_no_meters_but_still_get_state() {
	let (hub, rec) = start();
	hub.set_visible(false);
	hub.set_meters(Meters {
		microphone_dbfs: Some(-10.0),
		..Meters::default()
	});
	hub.update(|s| s.revision = 1);
	tokio::time::sleep(Duration::from_millis(200)).await;
	assert!(rec.meters.lock().unwrap().is_empty());
	assert_eq!(rec.states.lock().unwrap().len(), 1);
	hub.set_visible(true);
	hub.set_meters(Meters {
		microphone_dbfs: Some(-10.0),
		..Meters::default()
	});
	tokio::time::sleep(Duration::from_millis(200)).await;
	assert_eq!(rec.meters.lock().unwrap().len(), 1);
}

#[tokio::test(start_paused = true)]
async fn identical_updates_do_not_wake_the_ui() {
	let (hub, rec) = start();
	hub.update(|s| s.revision = 5);
	tokio::time::sleep(Duration::from_millis(100)).await;
	for _ in 0..10 {
		hub.update(|s| s.revision = 5);
		tokio::time::sleep(Duration::from_millis(100)).await;
	}
	assert_eq!(rec.states.lock().unwrap().len(), 1);
}

#[test]
fn tray_icon_follows_call_and_mute_state() {
	use relay_ui::emitter::TrayState;
	let mut state = AppState {
		link: Link::Connected,
		..AppState::default()
	};
	assert_eq!(tray_summary(&state).icon, TrayState::Idle);
	state.load_snapshot(&snapshot(1, vec![pb_call(pb::CallState::Connected, None)]));
	assert_eq!(tray_summary(&state).icon, TrayState::InCall);
	let mut snap = snapshot(2, vec![pb_call(pb::CallState::Connected, None)]);
	snap.settings = Some(pb::Settings {
		global_muted: true,
		..Default::default()
	});
	state.load_snapshot(&snap);
	assert_eq!(
		tray_summary(&state).icon,
		TrayState::Muted,
		"mute wins over in-call"
	);
	state.link = Link::Offline;
	assert_ne!(
		tray_summary(&state).icon,
		TrayState::Muted,
		"no stale mute while disconnected"
	);
}
