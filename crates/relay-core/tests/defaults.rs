//! The default transmit mode applied when a call connects, and `ptt.enabled`.

mod common;

use relay_core::testing::Node;
use relay_core::*;

fn defaults(node: &mut Node, mode: RouteMode, ptt_enabled: bool) {
	node.feed(Input::System(SystemEvent::CallDefaults { mode, ptt_enabled }));
}

/// A node whose next call connects under the given defaults.
fn connect(mode: RouteMode, ptt_enabled: bool) -> (Node, ClientId, CallId, ControllerLeaseId) {
	let mut node = Node::new(0xA0);
	let client = node.attach_client();
	defaults(&mut node, mode, ptt_enabled);
	let (call, lease) = common::activate(&mut node, client, common::peer());
	(node, client, call, lease)
}

fn route_of(node: &Node, call: CallId) -> TransmitRoute {
	node.state.snapshot().calls[&call].route
}

#[test]
fn a_call_that_connects_starts_in_the_default_mode() {
	for (mode, expected) in [
		(RouteMode::Muted, TransmitRoute::MUTED),
		(RouteMode::Unmuted, TransmitRoute::unmuted()),
		(RouteMode::PushToTalk, TransmitRoute::push_to_talk()),
	] {
		let (node, _, call, _) = connect(mode, true);
		assert_eq!(route_of(&node, call), expected, "{mode:?}");
	}
}

#[test]
fn the_default_is_applied_with_a_new_route_generation_so_older_audio_cannot_pass() {
	let (node, _, call, _) = connect(RouteMode::Unmuted, true);
	let view = node.state.snapshot().calls[&call].clone();
	assert_ne!(view.route_generation, RouteGeneration::ZERO);
	assert_eq!(view.route, TransmitRoute::unmuted());
}

#[test]
fn an_explicit_choice_before_the_call_connects_beats_the_default() {
	let mut node = Node::new(0xA1);
	let client = node.attach_client();
	defaults(&mut node, RouteMode::Unmuted, true);
	node.link_up(common::peer());
	let (call, lease) = node.start_call(client, common::peer());
	let session = node.state.call(call).unwrap().session;
	node.feed(Input::Peer(PeerEvent::Accepted { call, session }));
	// Connecting: the user picks muted before the path is up.
	node.command(
		client,
		Command::SetTransmitMode {
			call,
			lease,
			route: TransmitRoute::MUTED,
		},
	)
	.unwrap();
	node.complete_media();
	assert_eq!(route_of(&node, call), TransmitRoute::MUTED);
}

#[test]
fn ptt_off_rejects_choosing_it_and_falls_back_to_muted() {
	let (mut node, client, call, lease) = connect(RouteMode::PushToTalk, false);
	assert_eq!(route_of(&node, call), TransmitRoute::MUTED, "muted fallback");
	let result = node.command(
		client,
		Command::SetTransmitMode {
			call,
			lease,
			route: TransmitRoute::push_to_talk(),
		},
	);
	assert_eq!(result, Err(Reject::PttDisabled));
	// The other modes still work.
	node.command(
		client,
		Command::SetTransmitMode {
			call,
			lease,
			route: TransmitRoute::unmuted(),
		},
	)
	.unwrap();
}

#[test]
fn turning_ptt_off_mutes_calls_in_ptt_and_closes_their_gate() {
	let (mut node, client, call, lease) = connect(RouteMode::PushToTalk, true);
	node.complete_capture();
	node.command(
		client,
		Command::Ptt(PttUpdate {
			call,
			lease,
			source: InputSourceId::new(1),
			sequence: 1,
			pressed: true,
			ttl: None,
			immediate: false,
		}),
	)
	.unwrap();
	assert!(node.state.snapshot().calls[&call].gate.open);
	defaults(&mut node, RouteMode::PushToTalk, false);
	let view = node.state.snapshot().calls[&call].clone();
	assert_eq!(view.route, TransmitRoute::MUTED);
	assert!(!view.gate.open);
}

#[test]
fn turning_ptt_off_leaves_unmuted_calls_alone() {
	let (mut node, _, call, _) = connect(RouteMode::Unmuted, true);
	defaults(&mut node, RouteMode::Unmuted, false);
	assert_eq!(route_of(&node, call), TransmitRoute::unmuted());
}
