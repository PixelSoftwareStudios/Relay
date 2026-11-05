mod common;

use common::{Fixture, gate_in, peer};
use relay_core::testing::{Node, device_key};
use relay_core::*;

#[test]
fn receive_only_by_default_and_no_capture_before_consent() {
	let f = Fixture::new();
	assert!(
		f.node.device.opened.is_empty(),
		"no capture without consent"
	);
	assert_eq!(f.view().gate.deny, Some(DenyReason::CaptureInvalid));
	assert_eq!(f.node.state.snapshot().global.capture, CaptureState::Closed);
}

#[test]
fn consent_opens_capture_and_gate_opens_only_after_ready() {
	let mut f = Fixture::new();
	f.route(TransmitRoute::open_mic()).unwrap();
	assert_eq!(f.node.device.opened.len(), 1);
	assert!(!f.gate_open(), "capture still opening");
	f.node.complete_capture();
	assert!(f.gate_open());
}

#[test]
fn withdrawal_stops_transmit_and_publishes_deny_first() {
	let mut f = Fixture::new();
	f.consent(TransmitRoute::open_mic());
	let (call, lease) = (f.call, f.lease);
	let operation = f.node.ids.operation();
	let out = f.node.feed(Input::Client(Request {
		client: f.client,
		operation,
		command: Command::SetTransmitRoute {
			call,
			lease,
			route: TransmitRoute::RECEIVE_ONLY,
		},
	}));
	assert!(out.effects[0].is_gate(), "deny precedes every other effect");
	assert!(!gate_in(&out.effects, call).unwrap().open);
	assert!(
		out.effects
			.iter()
			.any(|e| matches!(e, Effect::CloseCapture { .. }))
	);
	assert_eq!(f.node.state.snapshot().global.capture, CaptureState::Closed);
}

#[test]
fn route_generation_advances_on_every_consent_change() {
	let mut f = Fixture::new();
	let g0 = f.view().route_generation;
	f.consent(TransmitRoute::open_mic());
	let g1 = f.view().route_generation;
	f.route(TransmitRoute::open_mic()).unwrap();
	assert_eq!(f.view().route_generation, g1, "no-op keeps the generation");
	f.route(TransmitRoute::push_to_talk()).unwrap();
	assert!(g0 < g1 && g1 < f.view().route_generation);
}

#[test]
fn invalid_route_and_not_active_call_are_rejected() {
	let mut f = Fixture::new();
	let bad = TransmitRoute {
		mode: RouteMode::PushToTalk,
		background: true,
	};
	assert_eq!(f.route(bad), Err(Reject::InvalidRoute));

	let mut node = Node::new(1);
	let client = node.attach_client();
	node.link_up(peer());
	let (call, lease) = node.start_call(client, peer());
	let route = TransmitRoute::open_mic();
	let result = node.command(client, Command::SetTransmitRoute { call, lease, route });
	assert_eq!(
		result,
		Err(Reject::CallNotActive),
		"ringing calls cannot be given mic consent"
	);
	assert!(node.device.opened.is_empty());
}

#[test]
fn every_deny_wins_over_open_mic() {
	let mut f = Fixture::new();
	f.consent(TransmitRoute::open_mic());
	let (call, lease) = (f.call, f.lease);

	f.cmd(Command::SetGlobalControls {
		muted: Some(true),
		deafened: None,
		output_gain: None,
		ptt_release_delay: None,
		expected: None,
	})
	.unwrap();
	assert_eq!(f.view().gate.deny, Some(DenyReason::GlobalMuted));
	// Clearing per-call mute cannot clear the global kill switch.
	f.cmd(Command::SetCallControls {
		call,
		lease,
		muted: Some(false),
		deafened: None,
		gain: None,
	})
	.unwrap();
	assert!(!f.gate_open());
	f.cmd(Command::SetGlobalControls {
		muted: Some(false),
		deafened: None,
		output_gain: None,
		ptt_release_delay: None,
		expected: None,
	})
	.unwrap();
	assert!(f.gate_open());

	f.cmd(Command::SetCallControls {
		call,
		lease,
		muted: Some(true),
		deafened: None,
		gain: None,
	})
	.unwrap();
	assert_eq!(f.view().gate.deny, Some(DenyReason::CallMuted));
}

#[test]
fn deafen_suppresses_receive_and_transmit_and_preserves_mute() {
	let mut f = Fixture::new();
	f.consent(TransmitRoute::open_mic());
	let global = |muted, deafened| Command::SetGlobalControls {
		muted,
		deafened,
		output_gain: None,
		ptt_release_delay: None,
		expected: None,
	};
	f.cmd(global(Some(true), None)).unwrap();
	f.cmd(global(None, Some(true))).unwrap();
	f.cmd(global(None, Some(false))).unwrap();
	let g = f.node.state.snapshot().global;
	assert!(g.muted && !g.deafened, "undeafen must not unmute");
	f.cmd(global(Some(false), Some(true))).unwrap();
	assert_eq!(f.view().gate.deny, Some(DenyReason::GlobalDeafened));
	let mix = f.node.effects.iter().rev().find_map(|e| match e {
		Effect::ReceiveMix(m) => Some(*m),
		_ => None,
	});
	assert_eq!(mix.map(|m| m.audible), Some(false));
}

#[test]
fn per_call_gain_and_deafen_are_independent_per_call() {
	let mut node = Node::new(1);
	let client = node.attach_client();
	let other = device_key(0xC0);
	let (a, lease_a) = common::activate(&mut node, client, peer());
	let (b, _) = common::activate(&mut node, client, other);
	node.command(
		client,
		Command::SetCallControls {
			call: a,
			lease: lease_a,
			muted: None,
			deafened: Some(true),
			gain: Some(Gain::from_permille(500)),
		},
	)
	.unwrap();
	let snap = node.state.snapshot();
	assert!(snap.calls[&a].controls.deafened);
	assert_eq!(snap.calls[&a].controls.gain.permille(), 500);
	assert!(!snap.calls[&b].controls.deafened);
	assert_eq!(Gain::from_permille(9_999).permille(), Gain::MAX_PERMILLE);
}

#[test]
fn consent_is_per_call() {
	let mut node = Node::new(1);
	let client = node.attach_client();
	let (a, lease_a) = common::activate(&mut node, client, peer());
	let (b, _) = common::activate(&mut node, client, device_key(0xC0));
	node.command(
		client,
		Command::SetTransmitRoute {
			call: a,
			lease: lease_a,
			route: TransmitRoute::open_mic(),
		},
	)
	.unwrap();
	node.complete_capture();
	let snap = node.state.snapshot();
	assert!(snap.calls[&a].gate.open);
	assert!(!snap.calls[&b].gate.open);
	assert_eq!(snap.calls[&b].gate.deny, Some(DenyReason::NotAuthorized));
}

#[test]
fn stale_capture_completion_is_rejected_and_loss_needs_explicit_resume() {
	let mut f = Fixture::new();
	f.route(TransmitRoute::open_mic()).unwrap();
	let gen1 = f.node.device.opened[0];
	f.node.complete_capture();
	assert!(f.gate_open());

	f.node.feed(f.node.device.unplugged(gen1));
	assert!(!f.gate_open());
	assert_eq!(f.node.state.snapshot().global.capture, CaptureState::Lost);
	// Duplicate failure and a late success from before the failure are ignored.
	let dup = f.node.feed(f.node.device.unplugged(gen1));
	assert_eq!(dup.result, Err(Reject::StaleGeneration));
	let late = f.node.feed(f.node.device.ready(gen1));
	assert_eq!(late.result, Err(Reject::StaleGeneration));
	assert!(!f.gate_open());
	assert_eq!(
		f.node.device.opened.len(),
		1,
		"no automatic reopen or device fallback"
	);

	f.cmd(Command::ResumeCapture).unwrap();
	assert_eq!(f.node.device.opened.len(), 2);
	let gen2 = f.node.device.opened[1];
	assert!(gen2 > gen1);
	// A second failure during recovery, then completion for the new generation.
	f.node.complete_capture();
	assert!(f.gate_open());
	assert_eq!(f.cmd(Command::ResumeCapture), Err(Reject::NothingToResume));
}

#[test]
fn open_mic_follows_controller_unless_background() {
	let mut f = Fixture::new();
	f.consent(TransmitRoute::open_mic());
	f.node.feed(Input::Disconnected(f.client));
	assert_eq!(f.view().route, TransmitRoute::RECEIVE_ONLY);
	assert!(!f.gate_open());

	let mut f = Fixture::new();
	f.consent(TransmitRoute::open_mic_background());
	f.node.feed(Input::Disconnected(f.client));
	assert_eq!(f.view().route, TransmitRoute::open_mic_background());
	assert!(
		f.gate_open(),
		"background authority survives closing the client"
	);
}

#[test]
fn lock_and_suspend_close_the_gate_and_close_capture() {
	for (down, up) in [
		(SessionEvent::Locked, SessionEvent::Unlocked),
		(SessionEvent::Suspended, SessionEvent::Resumed),
	] {
		let mut f = Fixture::new();
		f.consent(TransmitRoute::open_mic());
		f.node.feed(Input::System(SystemEvent::Session(down)));
		assert_eq!(f.view().gate.deny, Some(DenyReason::SessionBlocked));
		assert_eq!(f.node.state.snapshot().global.capture, CaptureState::Closed);
		f.node.feed(Input::System(SystemEvent::Session(up)));
		assert_eq!(
			f.node.state.snapshot().global.capture,
			CaptureState::Opening
		);
		assert!(!f.gate_open(), "must wait for a fresh capture generation");
	}
}

#[test]
fn global_settings_conflict_is_reported_with_current_revision() {
	let mut f = Fixture::new();
	let stale = f.node.state.snapshot().global.settings;
	f.cmd(Command::SetGlobalControls {
		muted: Some(true),
		deafened: None,
		output_gain: None,
		ptt_release_delay: None,
		expected: Some(stale),
	})
	.unwrap();
	let result = f.cmd(Command::SetGlobalControls {
		muted: Some(false),
		deafened: None,
		output_gain: None,
		ptt_release_delay: None,
		expected: Some(stale),
	});
	assert!(matches!(result, Err(Reject::SettingsConflict { current }) if current > stale));
}

#[test]
fn controller_takeover_requires_intent_and_expires_old_authority() {
	let mut f = Fixture::new();
	f.consent(TransmitRoute::open_mic());
	let other = f.node.attach_client();
	let call = f.call;
	let denied = f.node.command(
		other,
		Command::AcquireController {
			call,
			takeover: false,
		},
	);
	assert_eq!(denied, Err(Reject::ControllerHeld { holder: f.client }));

	let Ok(Reply::Controller(new_lease)) = f.node.command(
		other,
		Command::AcquireController {
			call,
			takeover: true,
		},
	) else {
		panic!("takeover failed");
	};
	assert_ne!(new_lease, f.lease);
	// The previous client's lease is stale; it cannot change routing any more.
	let stale = f.route(TransmitRoute::RECEIVE_ONLY);
	assert_eq!(
		stale,
		Err(Reject::StaleController {
			current: Some(new_lease)
		})
	);
	// Takeover does not carry the old client's consent over.
	assert_eq!(f.view().route, TransmitRoute::RECEIVE_ONLY);
	assert!(!f.gate_open());
}
