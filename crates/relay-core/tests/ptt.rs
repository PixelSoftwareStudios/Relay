mod common;

use std::time::Duration;

use common::Fixture;
use relay_core::*;

fn ptt_fixture() -> Fixture {
	let mut f = Fixture::new();
	f.consent(TransmitRoute::push_to_talk());
	f
}

#[test]
fn ptt_gate_is_closed_until_pressed_and_closes_on_release() {
	let mut f = ptt_fixture();
	assert_eq!(f.view().gate.deny, Some(DenyReason::PttIdle));
	f.ptt(1, 1, true).unwrap();
	assert!(f.gate_open());
	f.ptt(1, 2, false).unwrap();
	assert!(!f.gate_open());
}

#[test]
fn press_requires_ptt_consent_and_active_call() {
	let mut f = Fixture::new();
	assert_eq!(f.ptt(1, 1, true), Err(Reject::RouteNotAuthorized));
	f.consent(TransmitRoute::open_mic());
	assert_eq!(f.ptt(1, 2, true), Err(Reject::RouteNotAuthorized));
}

#[test]
fn lease_expires_without_refresh_and_refresh_extends_it() {
	let mut f = ptt_fixture();
	f.ptt(1, 1, true).unwrap();
	f.node.advance(200);
	f.ptt(1, 2, true).unwrap();
	f.node.advance(200);
	assert!(
		f.gate_open(),
		"refreshed lease still alive 400ms after the first press"
	);
	f.node.advance(100);
	assert!(
		!f.gate_open(),
		"300ms after the last refresh the lease is dead"
	);
	assert!(
		f.node
			.effects
			.iter()
			.any(|e| matches!(e, Effect::GateUpdate { gate, .. } if !gate.open))
	);
	assert_eq!(f.node.state.next_deadline(), None);
}

#[test]
fn requested_ttl_is_clamped_to_the_hard_maximum() {
	let mut f = ptt_fixture();
	let update = PttUpdate {
		call: f.call,
		lease: f.lease,
		source: InputSourceId::new(1),
		sequence: 1,
		pressed: true,
		ttl: Some(Duration::from_secs(60)),
	};
	let Ok(Reply::Input { expires_at, .. }) = f.cmd(Command::Ptt(update)) else {
		panic!("press failed");
	};
	assert_eq!(expires_at.as_millis(), 500);
	let zero = PttUpdate {
		sequence: 2,
		ttl: Some(Duration::ZERO),
		..update
	};
	assert_eq!(f.cmd(Command::Ptt(zero)), Err(Reject::InvalidTtl));
}

#[test]
fn stale_and_reordered_sequences_cannot_revive_a_press() {
	let mut f = ptt_fixture();
	f.ptt(1, 5, true).unwrap();
	f.ptt(1, 6, false).unwrap();
	// A delayed press from before the release arrives late.
	assert_eq!(f.ptt(1, 5, true), Err(Reject::StaleSequence { last: 6 }));
	assert_eq!(f.ptt(1, 6, true), Err(Reject::StaleSequence { last: 6 }));
	assert!(!f.gate_open());
}

#[test]
fn sources_are_independent_and_bounded() {
	let mut f = ptt_fixture();
	f.ptt(1, 1, true).unwrap();
	f.ptt(2, 1, true).unwrap();
	f.ptt(1, 2, false).unwrap();
	assert!(f.gate_open(), "source 2 still holds the key");
	for source in 3..=8 {
		f.ptt(source, 1, false).unwrap();
	}
	assert_eq!(f.ptt(9, 1, true), Err(Reject::TooManyInputSources));
}

#[test]
fn held_ptt_does_not_survive_client_disconnect() {
	let mut f = ptt_fixture();
	f.ptt(1, 1, true).unwrap();
	f.node.feed(Input::Disconnected(f.client));
	assert!(!f.gate_open());
	assert!(f.node.state.call(f.call).unwrap().input_leases.is_empty());
}

#[test]
fn held_ptt_does_not_survive_takeover_and_old_press_never_revives() {
	let mut f = ptt_fixture();
	f.ptt(1, 1, true).unwrap();
	let other = f.node.attach_client();
	let call = f.call;
	f.node
		.command(
			other,
			Command::AcquireController {
				call,
				takeover: true,
			},
		)
		.unwrap();
	assert!(!f.gate_open());
	assert_eq!(
		f.ptt(1, 2, true),
		Err(Reject::StaleController {
			current: f.node.state.call(call).unwrap().controller.map(|c| c.id)
		})
	);
	// A release from the old holder is harmless and idempotent.
	assert_eq!(f.ptt(1, 3, false), Ok(Reply::Done));
}

#[test]
fn held_ptt_does_not_survive_route_generation_change() {
	let mut f = ptt_fixture();
	f.ptt(1, 1, true).unwrap();
	f.route(TransmitRoute::open_mic()).unwrap();
	f.route(TransmitRoute::push_to_talk()).unwrap();
	assert!(!f.gate_open(), "new route generation needs a fresh press");
	f.ptt(1, 2, true).unwrap();
	assert!(f.gate_open());
}

#[test]
fn held_ptt_does_not_survive_lock_suspend_or_device_loss() {
	for event in [
		SystemEvent::Session(SessionEvent::Locked),
		SystemEvent::Session(SessionEvent::Suspended),
	] {
		let mut f = ptt_fixture();
		f.ptt(1, 1, true).unwrap();
		f.node.feed(Input::System(event));
		assert!(f.node.state.call(f.call).unwrap().input_leases.is_empty());
		// Unlocking does not revive the press, and a new one needs the session.
		f.node
			.feed(Input::System(SystemEvent::Session(SessionEvent::Unlocked)));
		f.node
			.feed(Input::System(SystemEvent::Session(SessionEvent::Resumed)));
		assert!(!f.gate_open());
	}

	let mut f = ptt_fixture();
	f.ptt(1, 1, true).unwrap();
	let generation = f.node.device.opened[0];
	f.node.feed(f.node.device.unplugged(generation));
	assert!(f.node.state.call(f.call).unwrap().input_leases.is_empty());
}

#[test]
fn press_is_refused_while_the_session_is_locked() {
	let mut f = ptt_fixture();
	f.node
		.feed(Input::System(SystemEvent::Session(SessionEvent::Locked)));
	assert_eq!(f.ptt(1, 1, true), Err(Reject::SessionBlocked));
}

#[test]
fn global_mute_wins_over_a_held_press() {
	let mut f = ptt_fixture();
	f.ptt(1, 1, true).unwrap();
	assert!(f.gate_open());
	f.cmd(Command::SetGlobalControls {
		muted: Some(true),
		deafened: None,
		output_gain: None,
		expected: None,
	})
	.unwrap();
	assert_eq!(f.view().gate.deny, Some(DenyReason::GlobalMuted));
}

#[test]
fn ending_the_call_drops_leases_and_route() {
	let mut f = ptt_fixture();
	f.ptt(1, 1, true).unwrap();
	let (call, lease) = (f.call, f.lease);
	f.cmd(Command::EndCall { call, lease }).unwrap();
	assert!(f.node.state.call(call).unwrap().input_leases.is_empty());
	assert_eq!(f.view().route, TransmitRoute::RECEIVE_ONLY);
	assert_eq!(f.node.state.snapshot().global.capture, CaptureState::Closed);
}
