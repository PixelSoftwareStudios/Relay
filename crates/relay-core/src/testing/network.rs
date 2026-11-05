use crate::command::{Input, PeerEvent};
use crate::effect::{Effect, PeerMessage};
use crate::id::PeerId;

/// Fake peer network: collects outgoing control messages and translates
/// them into the events the receiving engine would see.
#[derive(Debug, Default)]
pub struct FakeNetwork {
	/// Messages sent, in order, with their destination.
	pub sent: Vec<(PeerId, PeerMessage)>,
	/// Peers a link was requested for.
	pub link_requests: Vec<PeerId>,
	outbox: Vec<(PeerId, PeerMessage)>,
}

impl FakeNetwork {
	/// Observes an effect; returns whether it was a network effect.
	pub fn observe(&mut self, effect: &Effect) -> bool {
		match *effect {
			Effect::Peer { peer, message } => {
				self.sent.push((peer, message));
				self.outbox.push((peer, message));
				true
			}
			Effect::EnsureLink { peer } => {
				self.link_requests.push(peer);
				true
			}
			_ => false,
		}
	}

	/// Takes messages not yet delivered.
	pub fn take_outbox(&mut self) -> Vec<(PeerId, PeerMessage)> {
		std::mem::take(&mut self.outbox)
	}

	/// The event the remote engine (identified by `from`) observes for a message.
	pub fn delivered(from: PeerId, message: PeerMessage) -> Input {
		Input::Peer(match message {
			PeerMessage::Invite {
				call,
				session,
				allow_parallel_same_peer,
			} => PeerEvent::Invite {
				call,
				session,
				peer: from,
				allow_parallel_same_peer,
			},
			PeerMessage::Accept { call, session } => PeerEvent::Accepted { call, session },
			PeerMessage::Decline { call, session, .. } => PeerEvent::Declined { call, session },
			PeerMessage::End { call, session } => PeerEvent::Ended { call, session },
		})
	}
}
