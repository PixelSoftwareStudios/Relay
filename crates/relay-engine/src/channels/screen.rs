//! The only place channels touch calls: whether an inbound invite may ring.

use relay_core::channel::{InboundPolicy as CorePolicy, RingDecision};
use relay_core::{PeerEvent, PeerId, PeerMessage};

use super::hub::ChannelHub;
use crate::service::ports::InboundPolicy;

impl ChannelHub {
	/// Whether an invite from `peer` may ring. Only `Evidence::Proven` counts; a
	/// recognised advertisement tag does not. The answer has no accept variant:
	/// consent and acceptance stay with the user and the engine's own rules.
	pub fn ring_decision(&self, peer: PeerId) -> RingDecision {
		let policy = match self.inner.settings.load().inbound_policy {
			InboundPolicy::Anyone => CorePolicy::Anyone,
			InboundPolicy::SharedChannelOnly => CorePolicy::SharedChannelOnly,
		};
		let state = self.state();
		match state.scopes.get(&peer) {
			Some(scope) => policy.decide(&state.memberships, scope),
			None => policy.decide(&state.memberships, &relay_core::channel::PeerScope::new()),
		}
	}

	/// Filters a domain event on its way to the control actor. A dropped invite is
	/// answered as a plain decline (the caller cannot tell it from a user's decline)
	/// and never reaches call state. Returns whether the event may continue.
	pub fn screen(&self, event: &PeerEvent) -> bool {
		let PeerEvent::Invite {
			call,
			session,
			peer,
			..
		} = event
		else {
			return true;
		};
		if self.ring_decision(*peer) == RingDecision::Ring {
			return true;
		}
		let decline = PeerMessage::Decline {
			call: *call,
			session: *session,
			busy: false,
		};
		if let Err(error) = self.inner.peers.send(*peer, decline) {
			tracing::debug!(%error, "decline for a screened invite not sent");
		}
		false
	}
}
