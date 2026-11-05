//! With no client attached, an inbound invitation is declined as busy: it
//! never rings and never reaches the microphone.

use proptest::prelude::*;
use relay_core::testing::{SeqIds, device_key};
use relay_core::{
	Effect, Input, Limits, MonoTime, PeerEvent, PeerMessage, Reject, State, SystemEvent,
};

fn state() -> (State, SeqIds) {
	let mut ids = SeqIds::new();
	let state = State::new(device_key(1), ids.incarnation(), Limits::default()).unwrap();
	(state, ids)
}

fn presence(state: &mut State, attached: bool) {
	let outcome = state.apply(
		Input::System(SystemEvent::PresenceChanged { attached }),
		MonoTime::ZERO,
	);
	assert!(outcome.result.is_ok());
}

fn invite(state: &mut State, ids: &mut SeqIds) -> relay_core::Outcome {
	state.apply(
		Input::Peer(PeerEvent::Invite {
			call: ids.call(),
			session: ids.session(),
			peer: device_key(2),
			allow_parallel_same_peer: false,
		}),
		MonoTime::ZERO,
	)
}

fn opens_capture(effects: &[Effect]) -> bool {
	effects
		.iter()
		.any(|e| matches!(e, Effect::OpenCapture { .. }))
}

#[test]
fn an_invite_without_a_client_is_declined_busy_and_never_rings() {
	let (mut state, mut ids) = state();
	presence(&mut state, false);
	let outcome = invite(&mut state, &mut ids);
	assert_eq!(outcome.result, Err(Reject::NotAttached));
	assert!(outcome.effects.iter().any(|e| matches!(
		e,
		Effect::Peer {
			message: PeerMessage::Decline { busy: true, .. },
			..
		}
	)));
	assert!(!opens_capture(&outcome.effects));
	assert!(state.snapshot().calls.is_empty());
}

#[test]
fn attaching_again_lets_invites_ring() {
	let (mut state, mut ids) = state();
	presence(&mut state, false);
	invite(&mut state, &mut ids);
	presence(&mut state, true);
	let outcome = invite(&mut state, &mut ids);
	assert!(outcome.result.is_ok());
	assert_eq!(state.snapshot().calls.len(), 1);
}

#[test]
fn a_call_connected_before_the_last_client_left_keeps_running() {
	let (mut state, mut ids) = state();
	invite(&mut state, &mut ids);
	let before = state.snapshot();
	presence(&mut state, false);
	let after = state.snapshot();
	assert_eq!(before.calls, after.calls, "leaving changes no call");
}

proptest! {
	#[test]
	fn an_invite_while_unattached_never_creates_a_call_or_opens_capture(
		steps in proptest::collection::vec((any::<bool>(), 0u8..3), 1..40)
	) {
		let (mut state, mut ids) = state();
		let mut attached = true;
		for (toggle, kind) in steps {
			if kind == 0 {
				attached = toggle;
				presence(&mut state, attached);
				continue;
			}
			let calls = state.snapshot().calls.len();
			let outcome = invite(&mut state, &mut ids);
			if attached {
				continue;
			}
			prop_assert_eq!(outcome.result, Err(Reject::NotAttached));
			prop_assert_eq!(state.snapshot().calls.len(), calls);
			prop_assert!(!opens_capture(&outcome.effects));
		}
	}
}
