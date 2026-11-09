//! Close-to-tray decision, tray facts and the shared background state.

use std::sync::{Arc, Mutex};

use relay_desktop::background::{Background, BackgroundSink, CloseAction, TrayFacts, close_action};
use relay_desktop::tray::availability_text;
use relay_ui::emitter::{Sink, TraySummary};
use relay_ui::mock;
use relay_ui::model::{AppState, Link, Meters};

#[test]
fn closing_hides_only_with_a_tray_the_setting_on_and_no_quit_under_way() {
	use CloseAction::{HideToTray, Quit};
	assert_eq!(close_action(true, true, false), HideToTray);
	assert_eq!(close_action(true, false, false), Quit, "setting off");
	assert_eq!(close_action(false, true, false), Quit, "no tray (Linux)");
	assert_eq!(close_action(true, true, true), Quit, "menu Quit");
}

fn state(link: Link, background: Option<bool>) -> AppState {
	let mut state = AppState {
		link,
		..AppState::default()
	};
	if let Some(on) = background {
		let mut settings = mock::seed().settings.expect("mock settings");
		settings.background_available = on;
		state.settings = Some(settings);
	}
	state
}

#[test]
fn facts_follow_the_link_and_the_setting_and_default_to_off() {
	let up = TrayFacts::from_state(&state(Link::Connected, Some(true)));
	assert_eq!(
		up,
		TrayFacts {
			available: true,
			background: true
		}
	);
	let off = TrayFacts::from_state(&state(Link::Connected, Some(false)));
	assert_eq!(
		off,
		TrayFacts {
			available: true,
			background: false
		}
	);
	let down = TrayFacts::from_state(&state(Link::Reconnecting, Some(true)));
	assert!(!down.available);
	let unknown = TrayFacts::from_state(&state(Link::Connecting, None));
	assert_eq!(
		unknown,
		TrayFacts {
			available: false,
			background: false
		}
	);
	assert!(
		!Background::default().enabled(),
		"off until the engine says otherwise"
	);
}

#[test]
fn the_status_line_says_available_or_not() {
	let on = TrayFacts {
		available: true,
		background: true,
	};
	let off = TrayFacts {
		available: false,
		background: true,
	};
	assert_eq!(availability_text(on), "Available");
	assert_eq!(availability_text(off), "Not available");
}

#[test]
fn stale_is_taken_once_per_change_or_click() {
	let background = Background::default();
	assert!(background.take_stale().is_some(), "first draw");
	assert!(background.take_stale().is_none());
	let facts = TrayFacts {
		available: true,
		background: false,
	};
	assert!(background.set(facts));
	assert!(!background.set(facts), "unchanged");
	assert_eq!(background.take_stale(), Some(facts));
	assert!(!background.enabled());
	background.mark_stale();
	assert_eq!(background.take_stale(), Some(facts), "a click redraws");
}

#[derive(Default)]
struct Count(Mutex<usize>);

impl Sink for Count {
	fn state(&self, _: &AppState) {}
	fn meters(&self, _: &Meters) {}
	fn tray(&self, _: &TraySummary) {}
}

#[test]
fn the_sink_updates_the_state_and_repaints_only_on_a_change() {
	let background = Arc::new(Background::default());
	let repaints = Arc::new(Count::default());
	let counter = repaints.clone();
	let sink = BackgroundSink::new(
		Arc::new(Count::default()),
		background.clone(),
		Arc::new(move || *counter.0.lock().unwrap() += 1),
	);
	sink.state(&state(Link::Connected, Some(false)));
	sink.state(&state(Link::Connected, Some(false)));
	assert_eq!(*repaints.0.lock().unwrap(), 1);
	assert!(!background.enabled());
	sink.state(&state(Link::Connected, Some(true)));
	assert!(background.enabled());
	assert_eq!(*repaints.0.lock().unwrap(), 2);
}
