#![allow(dead_code)]

use std::time::Duration;

use relay_core::testing::{Node, device_key};
use relay_core::*;

pub fn peer() -> PeerId {
	device_key(0xB0)
}

/// A node with one attached client and one Active, receive-only call.
pub struct Fixture {
	pub node: Node,
	pub client: ClientId,
	pub call: CallId,
	pub lease: ControllerLeaseId,
}

impl Fixture {
	pub fn new() -> Self {
		Self::with_release_delay(Duration::ZERO)
	}

	/// A fixture whose push-to-talk release delay is `delay`.
	pub fn with_release_delay(delay: Duration) -> Self {
		let limits = Limits {
			ptt_release_delay: delay,
			..Limits::default()
		};
		let mut node = Node::with_limits(0xA0, limits);
		let client = node.attach_client();
		let (call, lease) = activate(&mut node, client, peer());
		Self {
			node,
			client,
			call,
			lease,
		}
	}

	pub fn cmd(&mut self, command: Command) -> OpResult {
		self.node.command(self.client, command)
	}

	pub fn route(&mut self, route: TransmitRoute) -> OpResult {
		let (call, lease) = (self.call, self.lease);
		self.cmd(Command::SetTransmitMode { call, lease, route })
	}

	/// Consent to a route and complete the capture open.
	pub fn consent(&mut self, route: TransmitRoute) {
		self.route(route).expect("route accepted");
		self.node.complete_capture();
	}

	pub fn ptt(&mut self, source: u64, sequence: u64, pressed: bool) -> OpResult {
		let update = PttUpdate {
			call: self.call,
			lease: self.lease,
			source: InputSourceId::new(source),
			sequence,
			pressed,
			ttl: None,
			immediate: false,
		};
		self.cmd(Command::Ptt(update))
	}

	pub fn view(&self) -> CallView {
		self.node.state.snapshot().calls[&self.call].clone()
	}

	pub fn gate_open(&self) -> bool {
		self.view().gate.open
	}
}

/// Drives a new outgoing call to Active (no transmit consent).
pub fn activate(node: &mut Node, client: ClientId, peer: PeerId) -> (CallId, ControllerLeaseId) {
	node.link_up(peer);
	let (call, lease) = node.start_call(client, peer);
	let session = node.state.call(call).expect("call exists").session;
	node.feed(Input::Peer(PeerEvent::Accepted { call, session }));
	node.complete_media();
	assert_eq!(
		node.state.call(call).map(|c| c.state),
		Some(CallState::Active)
	);
	(call, lease)
}

/// Last gate update published for a call in an outcome's effects.
pub fn gate_in(effects: &[Effect], call: CallId) -> Option<GateSnapshot> {
	effects.iter().rev().find_map(|effect| match effect {
		Effect::GateUpdate { gate, .. } if gate.call == call => Some(*gate),
		_ => None,
	})
}
