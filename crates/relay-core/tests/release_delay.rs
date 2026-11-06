mod common;

use std::time::Duration;

use common::{Fixture, gate_in};
use relay_core::*;

fn held(delay_ms: u64) -> Fixture {
	let mut f = Fixture::with_release_delay(Duration::from_millis(delay_ms));
	f.consent(TransmitRoute::push_to_talk());
	f.ptt(1, 1, true).unwrap();
	assert!(f.gate_open());
	f
}

fn global(f: &mut Fixture, muted: Option<bool>, deafened: Option<bool>) -> Outcome {
	let operation = f.node.ids.operation();
	f.node.feed(Input::Client(Request {
		client: f.client,
		operation,
		command: Command::SetGlobalControls {
			muted,
			deafened,
			output_gain: None,
			ptt_release_delay: None,
			expected: None,
		},
	}))
}

/// A held press that is released into a hold (key already up).
fn releasing(delay_ms: u64) -> Fixture {
	let mut f = held(delay_ms);
	f.ptt(1, 2, false).unwrap();
	assert!(f.gate_open());
	assert!(f.view().releasing);
	f
}

#[test]
fn release_keeps_the_gate_open_until_exactly_the_deadline() {
	let mut f = held(300);
	f.node.advance(50);
	f.ptt(1, 2, false).unwrap();
	assert!(f.gate_open() && f.view().releasing);
	let deadline = f.node.state.next_deadline().unwrap();
	assert_eq!(deadline.as_millis(), 350);
	f.node.advance(299);
	assert!(f.gate_open(), "still open 1 ms before the deadline");
	let out = f.node.advance(1);
	assert!(!f.gate_open(), "closed at the deadline");
	assert!(
		!gate_in(&out.effects, f.call).unwrap().open,
		"deny published in that outcome"
	);
	assert!(!f.view().ptt_held);
	assert_eq!(f.node.state.next_deadline(), None);
}

#[test]
fn zero_delay_closes_at_release() {
	let mut f = Fixture::new();
	f.consent(TransmitRoute::push_to_talk());
	f.ptt(1, 1, true).unwrap();
	f.ptt(1, 2, false).unwrap();
	assert!(!f.gate_open());
}

#[test]
fn immediate_release_skips_the_hold() {
	let mut f = held(500);
	let update = PttUpdate {
		call: f.call,
		lease: f.lease,
		source: InputSourceId::new(1),
		sequence: 2,
		pressed: false,
		ttl: None,
		immediate: true,
	};
	f.cmd(Command::Ptt(update)).unwrap();
	assert!(!f.gate_open());
}

#[test]
fn press_during_hold_cancels_it_and_keeps_transmitting() {
	let mut f = releasing(300);
	f.node.advance(200);
	f.ptt(1, 3, true).unwrap();
	assert!(!f.view().releasing);
	// Past the original hold deadline: still transmitting under the new lease.
	f.node.advance(150);
	assert!(f.gate_open() || f.node.state.call(f.call).unwrap().input_leases.is_empty());
	f.ptt(1, 4, true).unwrap();
	assert!(f.gate_open());
	f.ptt(1, 5, false).unwrap();
	f.node.advance(299);
	assert!(f.gate_open());
	f.node.advance(1);
	assert!(!f.gate_open());
}

#[test]
fn repeated_press_release_cycles_never_extend_a_hold() {
	let mut f = held(400);
	let mut seq = 1;
	for _ in 0..5 {
		seq += 1;
		f.ptt(1, seq, false).unwrap();
		f.node.advance(10);
		seq += 1;
		f.ptt(1, seq, true).unwrap();
		assert!(!f.view().releasing);
	}
	seq += 1;
	f.ptt(1, seq, false).unwrap();
	let deadline = f.node.state.next_deadline().unwrap();
	// A second key-up in the hold keeps the first deadline.
	f.node.advance(100);
	f.ptt(1, seq + 1, false).unwrap();
	assert_eq!(f.node.state.next_deadline(), Some(deadline));
	f.node.advance(300);
	assert!(!f.gate_open());
}

#[test]
fn delay_is_bounded_and_updates_live() {
	let mut f = Fixture::new();
	let set = |f: &mut Fixture, ms| {
		f.cmd(Command::SetGlobalControls {
			muted: None,
			deafened: None,
			output_gain: None,
			ptt_release_delay: Some(Duration::from_millis(ms)),
			expected: None,
		})
	};
	assert_eq!(set(&mut f, 2001), Err(Reject::InvalidReleaseDelay));
	set(&mut f, 2000).unwrap();
	assert_eq!(
		f.node.state.snapshot().global.ptt_release_delay,
		Duration::from_millis(2000)
	);
	f.consent(TransmitRoute::push_to_talk());
	f.ptt(1, 1, true).unwrap();
	f.ptt(1, 2, false).unwrap();
	assert_eq!(f.node.state.next_deadline().unwrap().as_millis(), 2000);
	let bad = Limits {
		ptt_release_delay: Duration::from_millis(2001),
		..Limits::default()
	};
	assert!(bad.validate().is_err());
	assert_eq!(
		Limits::default().ptt_release_delay,
		Duration::from_millis(200)
	);
}

#[test]
fn release_with_a_closed_gate_does_not_hold() {
	let mut f = held(500);
	global(&mut f, Some(true), None);
	f.ptt(1, 2, false).unwrap();
	global(&mut f, Some(false), None);
	assert!(!f.gate_open(), "no hold was created while muted");
}

fn assert_closed_at_once(name: &str, f: &mut Fixture, out: &Outcome) {
	// Switching to open mic is a new consent, not a fail-closed cause.
	let open_mic = name == "route change";
	assert!(open_mic || !f.gate_open(), "{name}: gate must be closed");
	assert!(
		open_mic
			|| !gate_in(&out.effects, f.call)
				.expect("gate update in same outcome")
				.open
	);
	assert!(!f.view().releasing && !f.view().ptt_held, "hold cancelled");
}

#[test]
fn every_fail_closed_cause_ends_a_hold_immediately() {
	type Cause = fn(&mut Fixture) -> Outcome;
	let causes: Vec<(&str, Cause)> = vec![
		("disconnect", |f| f.node.feed(Input::Disconnected(f.client))),
		("takeover", |f| {
			let other = f.node.attach_client();
			let call = f.call;
			let operation = f.node.ids.operation();
			f.node.feed(Input::Client(Request {
				client: other,
				operation,
				command: Command::AcquireController {
					call,
					takeover: true,
				},
			}))
		}),
		("release controller", |f| {
			let (call, lease) = (f.call, f.lease);
			let operation = f.node.ids.operation();
			f.node.feed(Input::Client(Request {
				client: f.client,
				operation,
				command: Command::ReleaseController { call, lease },
			}))
		}),
		("route change", |f| {
			let (call, lease) = (f.call, f.lease);
			let operation = f.node.ids.operation();
			f.node.feed(Input::Client(Request {
				client: f.client,
				operation,
				command: Command::SetTransmitMode {
					call,
					lease,
					route: TransmitRoute::unmuted(),
				},
			}))
		}),
		("consent withdrawal", |f| {
			let (call, lease) = (f.call, f.lease);
			let operation = f.node.ids.operation();
			f.node.feed(Input::Client(Request {
				client: f.client,
				operation,
				command: Command::SetTransmitMode {
					call,
					lease,
					route: TransmitRoute::MUTED,
				},
			}))
		}),
		("global mute", |f| global(f, Some(true), None)),
		("global deafen", |f| global(f, None, Some(true))),
		("call mute", |f| {
			let (call, lease) = (f.call, f.lease);
			let operation = f.node.ids.operation();
			f.node.feed(Input::Client(Request {
				client: f.client,
				operation,
				command: Command::SetCallControls {
					call,
					lease,
					muted: Some(true),
					deafened: None,
					gain: None,
				},
			}))
		}),
		("call deafen", |f| {
			let (call, lease) = (f.call, f.lease);
			let operation = f.node.ids.operation();
			f.node.feed(Input::Client(Request {
				client: f.client,
				operation,
				command: Command::SetCallControls {
					call,
					lease,
					muted: None,
					deafened: Some(true),
					gain: None,
				},
			}))
		}),
		("lock", |f| {
			f.node
				.feed(Input::System(SystemEvent::Session(SessionEvent::Locked)))
		}),
		("suspend", |f| {
			f.node
				.feed(Input::System(SystemEvent::Session(SessionEvent::Suspended)))
		}),
		("device loss", |f| {
			let generation = f.node.device.opened[0];
			f.node.feed(f.node.device.unplugged(generation))
		}),
		("link down", |f| {
			f.node
				.feed(Input::Peer(PeerEvent::LinkDown(common::peer())))
		}),
		("call end", |f| {
			let (call, lease) = (f.call, f.lease);
			let operation = f.node.ids.operation();
			f.node.feed(Input::Client(Request {
				client: f.client,
				operation,
				command: Command::EndCall { call, lease },
			}))
		}),
		("peer hangup", |f| {
			let call = f.call;
			let session = f.node.state.call(call).unwrap().session;
			f.node.feed(Input::Peer(PeerEvent::Ended { call, session }))
		}),
	];
	for (name, cause) in causes {
		let mut f = releasing(1_000);
		let out = cause(&mut f);
		assert_closed_at_once(name, &mut f, &out);
		// And the hold must not come back when the cause is cleared later.
		f.node.advance(10);
		assert!(!f.gate_open() || name == "route change", "{name}: revived");
		assert_eq!(
			f.node.state.next_deadline().is_some(),
			name == "call end" || name == "peer hangup",
			"{name}"
		);
	}
}

#[test]
fn unmuting_after_a_cancelled_hold_does_not_resume_transmission() {
	let mut f = releasing(1_000);
	global(&mut f, Some(true), None);
	let out = global(&mut f, Some(false), None);
	assert!(!gate_in(&out.effects, f.call).is_some_and(|g| g.open));
	assert!(!f.gate_open());
}

#[test]
fn a_stalled_press_expiring_during_another_sources_hold_cancels_the_hold() {
	let mut f = Fixture::with_release_delay(Duration::from_millis(400));
	f.consent(TransmitRoute::push_to_talk());
	f.ptt(1, 1, true).unwrap(); // source 1 stalls: expires at 300
	f.node.advance(100);
	f.ptt(2, 1, true).unwrap(); // expires at 400
	f.ptt(2, 2, false).unwrap(); // hold until 500
	assert_eq!(f.node.state.next_deadline().unwrap().as_millis(), 300);
	let out = f.node.advance(200);
	assert!(!gate_in(&out.effects, f.call).unwrap().open);
	assert!(!f.gate_open());
}

#[test]
fn a_hold_that_expires_is_not_reported_as_a_fault() {
	let mut f = releasing(100);
	let before = f.node.effects.len();
	f.node.advance(100);
	assert!(!f.gate_open());
	assert!(f.node.effects.len() > before);
	let snap = f.node.state.snapshot();
	assert!(!snap.calls[&f.call].ptt_held);
}
