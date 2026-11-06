//! Which call a push-to-talk press is sent for.

use relay_ui::model::{AppState, Call, Link, Mode, Phase, Transmit};
use relay_ui::ptt_target::{current_call, target};

fn call(id: &str) -> Call {
	Call {
		call_id: format!("{id:0>32}"),
		phase: Phase::Connected,
		controlled_by_me: true,
		controller_generation: 5,
		peer_id: String::new(),
		transmit: Transmit {
			mode: Mode::Muted,
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
		connected_at_unix_ms: 0,
	}
}

fn ptt(mut c: Call) -> Call {
	c.transmit.mode = Mode::PushToTalk;
	c
}

fn state(calls: Vec<Call>) -> AppState {
	AppState {
		link: Link::Connected,
		client_id: vec![9; 16],
		calls,
		..AppState::default()
	}
}

#[test]
fn a_controlled_push_to_talk_call_is_a_target() {
	let c = ptt(call("ab"));
	let s = state(vec![c.clone()]);
	let t = target(&s, &c.call_id).expect("target");
	assert_eq!(t.controller_generation, 5);
	assert_eq!(t.client_id, vec![9; 16]);
	assert_eq!(current_call(&s), Some(c.call_id));
}

#[test]
fn other_modes_other_controllers_ended_calls_and_a_lost_link_are_not() {
	let id = call("ab").call_id;
	let mut muted = call("ab");
	muted.transmit.mode = Mode::Muted;
	let mut open = call("ab");
	open.transmit.mode = Mode::Unmuted;
	let mut other = ptt(call("ab"));
	other.controlled_by_me = false;
	let mut ended = ptt(call("ab"));
	ended.phase = Phase::Ended;
	for c in [muted, open, other, ended] {
		let s = state(vec![c]);
		assert!(target(&s, &id).is_none());
		assert!(current_call(&s).is_none());
	}
	let mut s = state(vec![ptt(call("ab"))]);
	s.link = Link::Reconnecting;
	assert!(target(&s, &id).is_none(), "a detached client sends nothing");
}

#[test]
fn several_push_to_talk_calls_are_never_guessed() {
	let s = state(vec![ptt(call("ab")), ptt(call("cd"))]);
	assert!(current_call(&s).is_none());
	assert!(target(&s, &call("cd").call_id).is_some());
}
