mod common;

use common::{Fixture, activate, peer};
use relay_core::testing::{Node, device_key, pump};
use relay_core::*;

#[test]
fn outgoing_call_reaches_active_without_transmitting() {
	let f = Fixture::new();
	let view = f.view();
	assert_eq!(view.state, CallState::Active);
	assert_eq!(view.route, TransmitRoute::RECEIVE_ONLY);
	assert_eq!(view.gate.deny, Some(DenyReason::CaptureInvalid));
	assert!(!f.gate_open());
}

#[test]
fn preparing_waits_for_link_then_invites() {
	let mut node = Node::new(1);
	let client = node.attach_client();
	let (call, _) = node.start_call(client, peer());
	assert_eq!(node.state.call(call).map(|c| c.state), Some(CallState::Preparing));
	assert_eq!(node.network.link_requests, vec![peer()]);
	node.link_up(peer());
	assert_eq!(node.state.call(call).map(|c| c.state), Some(CallState::OutgoingRinging));
	assert!(matches!(node.network.sent[0].1, PeerMessage::Invite { .. }));
}

#[test]
fn link_setup_ring_and_path_timeouts_name_their_phase() {
	let mut node = Node::new(1);
	let client = node.attach_client();
	let (a, _) = node.start_call(client, peer());
	node.advance(5_000);
	assert_eq!(node.state.call(a).unwrap().reason, Some(EndReason::LinkSetupTimeout));

	node.link_up(peer());
	let (b, _) = node.start_call(client, peer());
	node.advance(30_000);
	assert_eq!(node.state.call(b).unwrap().reason, Some(EndReason::RingTimeout));

	let (c, _) = node.start_call(client, peer());
	let session = node.state.call(c).unwrap().session;
	node.feed(Input::Peer(PeerEvent::Accepted { call: c, session }));
	node.advance(3_000);
	let call = node.state.call(c).unwrap();
	assert_eq!((call.state, call.reason), (CallState::Failed, Some(EndReason::PathTimeout)));
	assert!(node.effects.iter().any(|e| matches!(e, Effect::StopMedia { call, .. } if *call == c)));
}

#[test]
fn incoming_accept_decline_and_terminal_answers() {
	let mut node = Node::new(1);
	let client = node.attach_client();
	let (call, session) = (node.ids.call(), node.ids.session());
	node.feed(Input::Peer(PeerEvent::Invite {
		call,
		session,
		peer: peer(),
		allow_parallel_same_peer: false,
	}));
	assert_eq!(node.state.call(call).unwrap().state, CallState::IncomingRinging);
	// A duplicate invitation never rings again.
	node.feed(Input::Peer(PeerEvent::Invite {
		call,
		session,
		peer: peer(),
		allow_parallel_same_peer: false,
	}));
	assert_eq!(node.state.live_calls(), 1);

	let lease = match node.command(client, Command::AcquireController { call, takeover: false }) {
		Ok(Reply::Controller(lease)) => lease,
		other => panic!("{other:?}"),
	};
	node.command(client, Command::DeclineCall { call, lease }).unwrap();
	assert_eq!(node.state.call(call).unwrap().state, CallState::Declined);
	// Accept after decline returns the terminal result and never revives it.
	let late = node.command(client, Command::AcceptCall { call, lease });
	assert_eq!(late, Err(Reject::CallTerminal(CallState::Declined)));
	assert_eq!(node.command(client, Command::DeclineCall { call, lease }), Ok(Reply::Done));
}

#[test]
fn hangup_is_idempotent_and_waits_for_media_teardown() {
	let mut f = Fixture::new();
	let (call, lease) = (f.call, f.lease);
	f.cmd(Command::EndCall { call, lease }).unwrap();
	assert_eq!(f.view().state, CallState::Ending);
	assert_eq!(f.cmd(Command::EndCall { call, lease }), Ok(Reply::Done));
	let session = f.node.state.call(call).unwrap().session;
	f.node.feed(Input::System(SystemEvent::MediaStopped { call, session }));
	assert_eq!(f.view().state, CallState::Ended);
	assert_eq!(f.cmd(Command::EndCall { call, lease }), Ok(Reply::Done));
	assert_eq!(f.node.state.live_calls(), 0);
}

#[test]
fn teardown_timeout_still_finishes_the_call() {
	let mut f = Fixture::new();
	let (call, lease) = (f.call, f.lease);
	f.cmd(Command::EndCall { call, lease }).unwrap();
	f.node.advance(3_000);
	assert_eq!(f.view().state, CallState::Ended);
}

#[test]
fn admission_caps_calls_and_same_peer() {
	let mut node = Node::new(1);
	let client = node.attach_client();
	for byte in 0..8u8 {
		node.link_up(device_key(100 + byte));
		node.start_call(client, device_key(100 + byte));
	}
	let (call, session) = (node.ids.call(), node.ids.session());
	let over = node.command(
		client,
		Command::StartCall { call, session, peer: device_key(200), allow_parallel_same_peer: false },
	);
	assert_eq!(over, Err(Reject::CallCapReached));

	// Incoming over capacity is declined as busy.
	let remote = (node.ids.call(), node.ids.session());
	node.feed(Input::Peer(PeerEvent::Invite {
		call: remote.0,
		session: remote.1,
		peer: device_key(201),
		allow_parallel_same_peer: false,
	}));
	let busy = node.network.sent.last().unwrap().1;
	assert!(matches!(busy, PeerMessage::Decline { busy: true, .. }));
	assert!(node.state.call(remote.0).is_none());

	let mut node = Node::new(1);
	let client = node.attach_client();
	node.start_call(client, peer());
	let (call, session) = (node.ids.call(), node.ids.session());
	let dup = node.command(
		client,
		Command::StartCall { call, session, peer: peer(), allow_parallel_same_peer: false },
	);
	assert!(matches!(dup, Err(Reject::SamePeerCallExists(_))));
	let parallel = node.command(
		client,
		Command::StartCall { call, session, peer: peer(), allow_parallel_same_peer: true },
	);
	assert!(parallel.is_ok());
}

#[test]
fn simultaneous_invites_elect_one_surviving_call_on_both_sides() {
	let mut a = Node::new(0x11);
	let mut b = Node::new(0x22);
	let (ca, cb) = (a.attach_client(), b.attach_client());
	a.link_up(b.key);
	b.link_up(a.key);
	let (call_a, _) = a.start_call(ca, b.key);
	let (call_b, _) = b.start_call(cb, a.key);
	pump(&mut a, &mut b);

	let live = |n: &Node| -> Vec<(CallId, Direction)> {
		n.state
			.snapshot()
			.calls
			.values()
			.filter(|c| !c.state.is_terminal())
			.map(|c| (c.id, c.direction))
			.collect()
	};
	let (la, lb) = (live(&a), live(&b));
	assert_eq!((la.len(), lb.len()), (1, 1));
	assert_eq!(la[0].0, lb[0].0, "both sides keep the same invitation");
	assert_ne!(la[0].1, lb[0].1, "one outgoing, one incoming");
	let survivor = la[0].0;
	assert!(survivor == call_a || survivor == call_b);
	// The losing local id is an alias of the survivor and the loser's
	// controller can still act on the incoming call it was folded into.
	let (loser_node, loser_id) =
		if survivor == call_a { (&b, call_b) } else { (&a, call_a) };
	assert_eq!(loser_node.state.call(loser_id).unwrap().id, survivor);
	// Never auto-accepted.
	assert_eq!(
		[a.state.call(survivor).unwrap().state, b.state.call(survivor).unwrap().state]
			.iter()
			.filter(|s| **s == CallState::IncomingRinging)
			.count(),
		1
	);
}

#[test]
fn explicit_parallel_calls_are_not_coalesced() {
	let mut a = Node::new(0x11);
	let mut b = Node::new(0x22);
	let (ca, _) = (a.attach_client(), b.attach_client());
	a.link_up(b.key);
	b.link_up(a.key);
	for _ in 0..2 {
		let (call, session) = (a.ids.call(), a.ids.session());
		a.command(
			ca,
			Command::StartCall { call, session, peer: b.key, allow_parallel_same_peer: true },
		)
		.unwrap();
	}
	pump(&mut a, &mut b);
	assert_eq!(b.state.live_calls(), 2);
}

#[test]
fn link_loss_interrupts_active_and_fails_others() {
	let mut f = Fixture::new();
	f.consent(TransmitRoute::open_mic());
	assert!(f.gate_open());
	let out = f.node.feed(Input::Peer(PeerEvent::LinkDown(peer())));
	assert_eq!(f.view().state, CallState::Interrupted);
	assert_eq!(f.view().route, TransmitRoute::RECEIVE_ONLY, "consent never revives");
	assert!(!common::gate_in(&out.effects, f.call).unwrap().open);
	// Reconnect resumes the call but not the route.
	f.node.link_up(peer());
	assert_eq!(f.view().state, CallState::Connecting);
	f.node.complete_media();
	assert_eq!(f.view().state, CallState::Active);
	assert!(!f.gate_open());
}

#[test]
fn lifecycle_table_has_no_exits_from_terminal_states() {
	use CallState::*;
	let all = [
		Preparing, OutgoingRinging, IncomingRinging, Connecting, Active, Interrupted, Ending,
		Ended, Declined, Cancelled, Failed,
	];
	for from in all.into_iter().filter(|s| s.is_terminal()) {
		assert!(all.iter().all(|to| !from.can_transition_to(*to)));
	}
	assert!(!Active.can_transition_to(Connecting));
	assert!(Interrupted.can_transition_to(Connecting));
}

#[test]
fn activate_helper_is_deterministic() {
	let run = || {
		let mut node = Node::new(7);
		let client = node.attach_client();
		activate(&mut node, client, peer());
		node.state.snapshot()
	};
	assert_eq!(run(), run());
}
