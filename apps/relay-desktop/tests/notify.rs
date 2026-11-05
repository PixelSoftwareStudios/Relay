//! Incoming-call notifications: only for a new ringing call, only when the window is out of
//! sight and the switch is on, caller name only, and honest when the system refuses.

use std::sync::{Arc, Mutex};

use relay_desktop::notify::{Dispatcher, IncomingTracker, Notifier, NotifySink, caller_name};
use relay_ui::emitter::{Sink, TraySummary};
use relay_ui::model::{AppState, Call, Link, Meters, Peer, Phase, Transmit, Trust};

fn peer(name: &str) -> Peer {
	Peer {
		peer_id: "aa".into(),
		display_name: name.into(),
		addresses: Vec::new(),
		trust: Trust::Unknown,
		reachable: true,
		avatar_hash: String::new(),
		fingerprint_short: "A1B2-C3D4".into(),
		shared_channels: Vec::new(),
	}
}

fn call(id: &str, phase: Phase) -> Call {
	Call {
		call_id: id.into(),
		peer_id: "aa".into(),
		phase,
		transmit: Transmit {
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
		controlled_by_me: false,
		controller_generation: 0,
	}
}

fn state(name: &str, calls: Vec<Call>) -> AppState {
	AppState {
		link: Link::Connected,
		peers: vec![peer(name)],
		calls,
		..AppState::default()
	}
}

#[test]
fn a_new_ringing_call_is_announced_once_with_the_caller_name_only() {
	let mut tracker = IncomingTracker::default();
	let ringing = state("Ada", vec![call("c1", Phase::IncomingRinging)]);
	assert_eq!(tracker.observe(&ringing, true, false), ["Ada"]);
	assert!(
		tracker.observe(&ringing, true, false).is_empty(),
		"the same call never twice"
	);
	let next = state("Ada", vec![call("c2", Phase::IncomingRinging)]);
	assert_eq!(tracker.observe(&next, true, false), ["Ada"]);
}

#[test]
fn nothing_while_the_window_is_in_front_or_the_switch_is_off() {
	let mut tracker = IncomingTracker::default();
	let ringing = state("Ada", vec![call("c1", Phase::IncomingRinging)]);
	assert!(tracker.observe(&ringing, true, true).is_empty(), "in front");
	assert!(
		tracker.observe(&ringing, true, false).is_empty(),
		"a call the user already saw is not announced after they look away"
	);
	let other = state("Ada", vec![call("c2", Phase::IncomingRinging)]);
	assert!(
		tracker.observe(&other, false, false).is_empty(),
		"switch off"
	);
	assert!(tracker.observe(&other, true, false).is_empty());
}

#[test]
fn calls_that_are_not_ringing_or_a_link_that_is_down_announce_nothing() {
	let mut tracker = IncomingTracker::default();
	for phase in [Phase::OutgoingRinging, Phase::Connected, Phase::Ended] {
		let s = state("Ada", vec![call("c", phase)]);
		assert!(tracker.observe(&s, true, false).is_empty());
	}
	let mut down = state("Ada", vec![call("d", Phase::IncomingRinging)]);
	down.link = Link::Reconnecting;
	assert!(tracker.observe(&down, true, false).is_empty());
}

#[test]
fn caller_names_are_clean_bounded_and_never_empty() {
	assert_eq!(caller_name("  Ada\u{7}\n "), "Ada");
	assert_eq!(caller_name(""), "Unknown caller");
	assert_eq!(caller_name("\u{1b}[31m").len(), "[31m".len());
	assert_eq!(caller_name(&"x".repeat(200)).chars().count(), 64);
	let unknown = state("", vec![call("c", Phase::IncomingRinging)]);
	let mut tracker = IncomingTracker::default();
	assert_eq!(tracker.observe(&unknown, true, false), ["Unknown caller"]);
}

#[derive(Default)]
struct Fake {
	shown: Mutex<Vec<String>>,
	refuse: Mutex<Option<String>>,
}

impl Notifier for Fake {
	fn incoming_call(&self, caller: &str) -> Result<(), String> {
		self.shown.lock().unwrap().push(caller.to_owned());
		match self.refuse.lock().unwrap().clone() {
			Some(reason) => Err(reason),
			None => Ok(()),
		}
	}
}

#[test]
fn a_refusal_is_kept_for_the_settings_text_until_a_later_one_succeeds() {
	let fake = Arc::new(Fake::default());
	let dispatcher = Dispatcher::new(fake.clone());
	assert_eq!(dispatcher.failure(), None);
	*fake.refuse.lock().unwrap() = Some("notifications are blocked".into());
	dispatcher.deliver("Ada");
	assert_eq!(
		dispatcher.failure().as_deref(),
		Some("notifications are blocked")
	);
	*fake.refuse.lock().unwrap() = None;
	dispatcher.deliver("Bo");
	assert_eq!(dispatcher.failure(), None);
	assert_eq!(*fake.shown.lock().unwrap(), ["Ada", "Bo"]);
}

#[derive(Default)]
struct Recording {
	states: Mutex<usize>,
}

impl Sink for Recording {
	fn state(&self, _: &AppState) {
		*self.states.lock().unwrap() += 1;
	}
	fn meters(&self, _: &Meters) {}
	fn tray(&self, _: &TraySummary) {}
}

#[test]
fn the_sink_forwards_state_and_notifies_off_the_emitter_thread() {
	let fake = Arc::new(Fake::default());
	let inner = Arc::new(Recording::default());
	let front = Arc::new(Mutex::new(true));
	let front_flag = front.clone();
	let sink = NotifySink::new(
		inner.clone(),
		Arc::new(Dispatcher::new(fake.clone())),
		Arc::new(|| true),
		Arc::new(move || *front_flag.lock().unwrap()),
	);
	let ringing = state("Ada", vec![call("c1", Phase::IncomingRinging)]);
	sink.state(&ringing);
	assert_eq!(*inner.states.lock().unwrap(), 1);
	*front.lock().unwrap() = false;
	sink.state(&state("Ada", vec![call("c2", Phase::IncomingRinging)]));
	for _ in 0..200 {
		if !fake.shown.lock().unwrap().is_empty() {
			break;
		}
		std::thread::sleep(std::time::Duration::from_millis(10));
	}
	assert_eq!(*fake.shown.lock().unwrap(), ["Ada"]);
	assert_eq!(*inner.states.lock().unwrap(), 2);
}
