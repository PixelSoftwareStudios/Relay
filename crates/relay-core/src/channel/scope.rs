use std::collections::BTreeMap;

use super::{ChannelId, Memberships, PEER_CHANNELS_MAX};

/// How strongly the local device believes a peer shares a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Evidence {
	/// The peer's advertisement carried a tag recognised for this channel. Tags are
	/// unauthenticated (anyone who knows the id can forge one), so this is a hint only.
	Candidate,
	/// The peer proved membership on an authenticated link.
	Proven,
}

/// Channels a remote peer is believed to share with us, with the strength of evidence.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PeerScope {
	shared: BTreeMap<ChannelId, Evidence>,
}

impl PeerScope {
	pub fn new() -> Self {
		Self::default()
	}

	/// Records evidence for a channel the local device has joined. Evidence never weakens: a
	/// later `Candidate` does not undo a `Proven`. Channels we have not joined, and entries
	/// beyond the bound, are ignored so an advertisement cannot grow state.
	pub fn note(&mut self, local: &Memberships, id: ChannelId, evidence: Evidence) {
		if !local.contains(&id) {
			return;
		}
		if !self.shared.contains_key(&id) && self.shared.len() >= PEER_CHANNELS_MAX {
			return;
		}
		let slot = self.shared.entry(id).or_insert(evidence);
		*slot = (*slot).max(evidence);
	}

	/// Drops proof of one channel, e.g. after a revocation or leaving it.
	pub fn forget(&mut self, id: &ChannelId) {
		self.shared.remove(id);
	}

	/// Channels shared with proof. Only these count for any authorization decision.
	pub fn proven(&self) -> impl Iterator<Item = ChannelId> + '_ {
		self.shared
			.iter()
			.filter(|(_, e)| **e == Evidence::Proven)
			.map(|(id, _)| *id)
	}

	pub fn candidates(&self) -> impl Iterator<Item = ChannelId> + '_ {
		self.shared.keys().copied()
	}

	pub fn shares_proven(&self, local: &Memberships) -> bool {
		self.proven().any(|id| local.contains(&id))
	}
}

/// Who may place an inbound invite in front of the user.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InboundPolicy {
	/// Today's behaviour: any authenticated peer may ring.
	#[default]
	Anyone,
	/// Only peers that proved a shared channel. Fingerprint trust is orthogonal.
	SharedChannelOnly,
	/// Nobody: every inbound invite is dropped, whatever the sender proved.
	Nobody,
}

/// Outcome of the channel check for an inbound invite. There is deliberately no accept
/// variant: the user (or the engine's consent rules) decides acceptance, never a channel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RingDecision {
	/// The invite may be shown or rung under the normal rules.
	Ring,
	/// The invite is dropped silently as if it were declined.
	Drop,
}

impl InboundPolicy {
	pub fn decide(self, local: &Memberships, peer: &PeerScope) -> RingDecision {
		match self {
			Self::Anyone => RingDecision::Ring,
			Self::SharedChannelOnly if peer.shares_proven(local) => RingDecision::Ring,
			Self::SharedChannelOnly | Self::Nobody => RingDecision::Drop,
		}
	}
}

/// Whether a peer may appear in a listing. A peer that advertised nothing channel related
/// (`scope` empty, `public` true) is a normal visible device; a hidden-mode peer is shown only
/// when we share at least one channel with it, proven or recognised.
pub fn visible(local: &Memberships, scope: &PeerScope, advertises_publicly: bool) -> bool {
	advertises_publicly || scope.candidates().any(|id| local.contains(&id))
}
