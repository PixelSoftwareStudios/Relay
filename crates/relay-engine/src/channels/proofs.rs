//! Membership proofs and certificate renewal, always on an already trusted link.

use relay_core::PeerId;
use relay_core::channel::ChannelId;
use relay_net::channel::join::renewal_request;
use relay_net::channel::{ProofOutcome, check_proof, make_proof};
use relay_net::proto::ChannelMessage;
use relay_net::proto::channel_message::Body;

use super::hub::ChannelHub;
use super::state::State;
use crate::peers::fingerprint;

impl ChannelHub {
	pub(super) fn send(&self, peer: PeerId, message: ChannelMessage) {
		if let Err(error) = self.inner.peers.send_channel(peer, message) {
			tracing::debug!(%error, "channel message not sent");
		}
	}

	/// Sends our proof for `only`, or for every channel we hold a certificate for.
	pub(super) fn send_proofs(&self, peer: PeerId, is_reply: bool, only: Option<ChannelId>) {
		let now = self.now();
		let messages: Vec<ChannelMessage> = {
			let state = self.state();
			let Some(link) = state.links.get(&peer) else {
				return;
			};
			state
				.records
				.iter()
				.filter(|r| only.is_none_or(|id| id == r.id))
				.filter_map(|r| make_proof(r, &link.binding, is_reply, now))
				.collect()
		};
		messages.into_iter().for_each(|m| self.send(peer, m));
	}

	/// A channel was gained: tell every trusted linked device so it can check us.
	/// Members that already answered on a link are asked again only by the answer's
	/// own rule (one reply per channel per link).
	pub(super) fn announce_channel(&self, id: ChannelId) {
		let peers: Vec<PeerId> = self.state().links.keys().copied().collect();
		for peer in peers {
			self.send_proofs(peer, false, Some(id));
		}
	}

	/// Checks a proof the peer sent on its live link. Returns whether anything
	/// visible (the proven sets) changed.
	pub(super) fn on_proof(&self, peer: PeerId, message: &ChannelMessage) -> bool {
		let Some(Body::Proof(proof)) = &message.body else {
			return false;
		};
		let (now, remote) = (self.now(), fingerprint(peer));
		let outcome: ProofOutcome = {
			let state = self.state();
			let Some(link) = state.links.get(&peer) else {
				return false;
			};
			check_proof(&state.records, &link.binding, &remote, message, now)
		};
		let learned = !outcome.new_revocations.is_empty();
		if let Some(id) = outcome.matched {
			for revocation in outcome.new_revocations {
				let _ = self
					.inner
					.store
					.update(&id, &mut |r| r.add_revocation(revocation.clone()));
			}
			if learned {
				let _ = self.reload();
			}
		}
		let mut changed = learned;
		let mut reply = None;
		{
			let mut state = self.state();
			match (outcome.proven, outcome.matched) {
				(Some(id), _) => {
					let before = state.proven(&peer);
					state.note(peer, id, relay_core::channel::Evidence::Proven);
					changed |= state.proven(&peer) != before;
					if !proof.is_reply
						&& let Some(link) = state.links.get_mut(&peer)
						&& link.replied.insert(id)
					{
						reply = Some(id);
					}
				}
				(None, Some(id)) => {
					if let Some(scope) = state.scopes.get_mut(&peer) {
						scope.forget(&id);
					}
					changed = true;
				}
				(None, None) => {}
			}
			if learned {
				changed |= drop_revoked(&mut state, now);
			}
		}
		if let Some(id) = reply {
			self.send_proofs(peer, true, Some(id));
		}
		if let Some(id) = outcome.proven {
			self.request_renewal(peer, id);
		}
		changed
	}

	/// Channels whose certificate is due and a linked peer that proved the same.
	pub(super) fn due_renewals(&self) -> Vec<(PeerId, ChannelId)> {
		let now = self.now();
		let state = self.state();
		let mut due = Vec::new();
		for (peer, link) in &state.links {
			for id in state.proven(peer) {
				let needs = state
					.records
					.iter()
					.any(|r| r.id == id && renewal_due(r, now));
				if needs && !link.renew.contains(&id) {
					due.push((*peer, id));
				}
			}
		}
		due
	}

	/// Asks `peer` (which proved the channel) to renew our certificate, if it is due.
	/// The peer answers only if it can admit; otherwise the answer is a uniform denial.
	pub(super) fn request_renewal(&self, peer: PeerId, id: ChannelId) {
		let now = self.now();
		let message = {
			let mut state = self.state();
			let Some(record) = state.records.iter().find(|r| r.id == id) else {
				return;
			};
			if !renewal_due(record, now) {
				return;
			}
			let Some(link) = state.links.get(&peer) else {
				return;
			};
			if link.renew.contains(&id) {
				return;
			}
			let Some(message) = renewal_request(record, &link.binding) else {
				return;
			};
			if let Some(link) = state.links.get_mut(&peer) {
				link.renew.insert(id);
			}
			message
		};
		self.send(peer, message);
	}
}

/// A member (not an authority holder) whose certificate is near or past expiry.
fn renewal_due(record: &relay_net::channel::ChannelRecord, now: u64) -> bool {
	record.authority.is_none()
		&& record
			.cert
			.as_ref()
			.is_some_and(|c| c.expires_at.saturating_sub(now) < ChannelHub::renew_window())
}

/// Forgets proof of peers a stored revocation now covers. True if any was dropped.
pub(super) fn drop_revoked(state: &mut State, now: u64) -> bool {
	let mut changed = false;
	let records = &state.records;
	for (peer, scope) in &mut state.scopes {
		let remote = fingerprint(*peer);
		let revoked: Vec<ChannelId> = scope
			.proven()
			.filter(|id| {
				records
					.iter()
					.any(|r| r.id == *id && r.is_revoked(&remote, now))
			})
			.collect();
		for id in revoked {
			scope.forget(&id);
			changed = true;
		}
	}
	changed
}
