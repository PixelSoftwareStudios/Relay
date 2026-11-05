//! Membership proofs and certificate renewal, always on an already trusted link.

use relay_core::PeerId;
use relay_core::channel::ChannelId;
use relay_net::channel::join::renewal_request;
use relay_net::channel::{
	ChannelRecord, ProofOutcome, check_proof, check_ref, make_proof, make_ref,
};
use relay_net::proto::ChannelMessage;
use relay_net::proto::channel_message::Body;

use super::hub::ChannelHub;
use super::state::{ProofEntry, REPROOF_AFTER, REPROOF_LEAD, State};
use crate::peers::fingerprint;

impl ChannelHub {
	pub(super) fn send(&self, peer: PeerId, message: ChannelMessage) {
		if let Err(error) = self.inner.peers.send_channel(peer, message) {
			tracing::debug!(%error, "channel message not sent");
		}
	}

	/// Step one of the proof: sends our keyed reference for `only`, or for every channel
	/// we hold a certificate for. A reference reveals nothing to a non-member; the
	/// certificate follows only once the peer's own reference verified.
	pub(super) fn send_refs(&self, peer: PeerId, only: Option<ChannelId>) {
		let now = self.now();
		let messages: Vec<ChannelMessage> = {
			let mut state = self.state();
			let State { records, links, .. } = &mut *state;
			let Some(link) = links.get_mut(&peer) else {
				return;
			};
			records
				.iter()
				.filter(|r| only.is_none_or(|id| id == r.id))
				.filter(|r| r.cert.as_ref().is_some_and(|c| now < c.expires_at))
				.filter(|r| link.sent_ref.insert(r.id) || only.is_some())
				.map(|r| make_ref(r, &link.binding))
				.collect()
		};
		messages.into_iter().for_each(|m| self.send(peer, m));
	}

	/// A channel was gained: tell every trusted linked device so it can check us.
	pub(super) fn announce_channel(&self, id: ChannelId) {
		let peers: Vec<PeerId> = self.state().links.keys().copied().collect();
		for peer in peers {
			self.send_refs(peer, Some(id));
		}
	}

	/// Sends our certificate (with the revocations we hold) to a peer that already proved
	/// the channel, so it learns a fresh revocation without waiting for a new reference.
	pub(super) fn send_proof_to_member(&self, peer: PeerId, id: ChannelId) {
		let now = self.now();
		let message = {
			let state = self.state();
			let Some(link) = state.links.get(&peer) else {
				return;
			};
			state
				.records
				.iter()
				.find(|r| r.id == id)
				.and_then(|r| make_proof(r, &link.binding, now))
		};
		message.into_iter().for_each(|m| self.send(peer, m));
	}

	/// The peer's reference: if it matches a channel we hold too, answer with our
	/// certificate (once per `REPROOF_AFTER`) and our own reference (once per link and
	/// channel, in reply to the first reference received for it).
	pub(super) fn on_member_ref(&self, peer: PeerId, message: &ChannelMessage) {
		let (now, mono) = (self.now(), self.mono());
		let (proof, own_ref) = {
			let mut state = self.state();
			let State { records, links, .. } = &mut *state;
			let Some(link) = links.get_mut(&peer) else {
				return;
			};
			let Some(id) = check_ref(records, &link.binding, message) else {
				return;
			};
			let Some(record) = records.iter().find(|r| r.id == id) else {
				return;
			};
			let due = link
				.replied
				.get(&id)
				.is_none_or(|at| mono.saturating_sub(*at) >= REPROOF_AFTER);
			let proof = if due {
				let proof = make_proof(record, &link.binding, now);
				if proof.is_some() {
					link.replied.insert(id, mono);
				}
				proof
			} else {
				None
			};
			// Our reference may have gone out before the peer held this channel, so the
			// first reference we get for it is answered with ours once more.
			link.sent_ref.insert(id);
			let own = (link.got_ref.insert(id) && record.cert.is_some())
				.then(|| make_ref(record, &link.binding));
			(proof, own)
		};
		proof.into_iter().for_each(|m| self.send(peer, m));
		own_ref.into_iter().for_each(|m| self.send(peer, m));
	}

	/// Checks a proof the peer sent on its live link. Returns whether anything
	/// visible (the proven sets) changed.
	pub(super) fn on_proof(&self, peer: PeerId, message: &ChannelMessage) -> bool {
		if !matches!(message.body, Some(Body::Proof(_))) {
			return false;
		}
		let (now, remote) = (self.now(), fingerprint(peer));
		// Signatures are checked on a stripped copy, outside the state lock.
		let (binding, views) = {
			let state = self.state();
			let Some(link) = state.links.get(&peer) else {
				return false;
			};
			(
				link.binding,
				state.records.iter().map(verifier_view).collect::<Vec<_>>(),
			)
		};
		let outcome: ProofOutcome = check_proof(&views, &binding, &remote, message, now);
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
		{
			let mut state = self.state();
			// The link may have been replaced while the proof was checked.
			if state.links.get(&peer).map(|l| l.binding) != Some(binding) {
				return changed;
			}
			match (outcome.proven, outcome.matched) {
				(Some(id), _) => {
					let before = state.proven(&peer);
					state.note(peer, id, relay_core::channel::Evidence::Proven);
					changed |= state.proven(&peer) != before;
					if let Some(expires_at) = outcome.expires_at {
						state.proofs.insert(
							(peer, id),
							ProofEntry {
								expires_at,
								asked: false,
							},
						);
					}
				}
				(None, Some(id)) => {
					if let Some(scope) = state.scopes.get_mut(&peer) {
						scope.forget(&id);
					}
					state.proofs.remove(&(peer, id));
					changed = true;
				}
				(None, None) => {}
			}
			if learned {
				changed |= drop_revoked(&mut state, now);
			}
		}
		if let Some(id) = outcome.proven {
			self.request_renewal(peer, id);
		}
		changed
	}

	/// Demotes proofs whose certificate lapsed and, shortly before that, asks the peer
	/// again (it sends a fresh certificate if it renewed). True if a peer was demoted.
	pub(super) fn expire_proofs(&self) -> bool {
		let now = self.now();
		let mut asks: Vec<(PeerId, ChannelId)> = Vec::new();
		let mut demoted = false;
		{
			let mut state = self.state();
			let State { proofs, scopes, .. } = &mut *state;
			proofs.retain(|(peer, id), entry| {
				if entry.expires_at <= now {
					if let Some(scope) = scopes.get_mut(peer) {
						scope.forget(id);
					}
					demoted = true;
					return false;
				}
				if !entry.asked && entry.expires_at.saturating_sub(now) <= REPROOF_LEAD {
					entry.asked = true;
					asks.push((*peer, *id));
				}
				true
			});
		}
		for (peer, id) in asks {
			self.send_refs(peer, Some(id));
		}
		demoted
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
	let State {
		records,
		scopes,
		proofs,
		..
	} = state;
	for (peer, scope) in scopes {
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
			proofs.remove(&(*peer, id));
			changed = true;
		}
	}
	changed
}

/// What proof checking needs from a record, without authority keys, password
/// credentials, issued certificates or invites.
fn verifier_view(record: &ChannelRecord) -> ChannelRecord {
	ChannelRecord {
		id: record.id,
		name: record.name.clone(),
		policy: record.policy.clone(),
		role: record.role,
		owner_public: record.owner_public,
		authority: None,
		password: None,
		cert: None,
		issued: Vec::new(),
		revocations: record.revocations.clone(),
		invites: Vec::new(),
	}
}
