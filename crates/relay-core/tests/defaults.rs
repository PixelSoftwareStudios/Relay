//! The default transmit mode applied when a call connects, .

mod common;

use relay_core::testing::Node;
use relay_core::*;

fn defaults(node: &mut Node, mode: RouteMode) {
	node.feed(Input::System(SystemEvent::CallDefaults { mode }));
}

/// A node whose next call connects under the given defaults.
fn connect(mode: RouteMode) -> (Node, ClientId, CallId, ControllerLeaseId) {
	let mut node = Node::new(0xA0);
	let client = node.attach_client();
	defaults(&mut node, mode);
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
		let (node, _, call, _) = connect(mode);
		assert_eq!(route_of(&node, call), expected, "{mode:?}");
	}
}

#[test]
fn the_default_is_applied_with_a_new_route_generation_so_older_audio_cannot_pass() {
	let (node, _, call, _) = connect(RouteMode::Unmuted);
	let view = node.state.snapshot().calls[&call].clone();
	assert_ne!(view.route_generation, RouteGeneration::ZERO);
	assert_eq!(view.route, TransmitRoute::unmuted());
}

#[test]
fn an_explicit_choice_before_the_call_connects_beats_the_default() {
	let mut node = Node::new(0xA1);
	let client = node.attach_client();
	defaults(&mut node, RouteMode::Unmuted);
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
fn push_to_talk_can_always_be_chosen() {
	let (mut node, client, call, lease) = connect(RouteMode::Muted);
	node.command(
		client,
		Command::SetTransmitMode {
			call,
			lease,
			route: TransmitRoute::push_to_talk(),
		},
	)
	.unwrap();
	assert_eq!(route_of(&node, call), TransmitRoute::push_to_talk());
}
