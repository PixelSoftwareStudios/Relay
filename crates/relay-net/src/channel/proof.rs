//! Membership proof between two devices that are already members of a channel.
//!
//! Two steps, so that a peer who is not in a channel learns nothing about it:
//!
//! 1. Each side sends a [`ChannelRef`](crate::proto::ChannelRef) per channel: a keyed hash of
//!    the session binding and the sender's role under the channel key. It proves knowledge of
//!    the channel id and reveals nothing to a peer without it.
//! 2. A side sends its certificate ([`make_proof`]) only to a peer whose reference for that
//!    channel it has verified ([`check_ref`]). The verifier then checks the certificate against
//!    the owner key, the **authenticated** fingerprint of the link, expiry and revocations
//!    ([`check_proof`]).
//!
//! A peer that has never held the id never sends a matching reference, so it never receives a
//! certificate, a channel id or a revocation. A peer that merely learned the id (a leak) can
//! obtain our certificate, which is not secret and is useless without our static key.

use relay_core::channel::ChannelId;

use super::binding::SessionBinding;
use super::cert::{MembershipCert, Revocation};
use super::ids::{ct_eq, reference};
use super::limits::REVOCATIONS_PER_PROOF_MAX;
use super::store::ChannelRecord;
use crate::identity::Fingerprint;
use crate::proto::channel_message::Body;
use crate::proto::{ChannelMessage, ChannelProof, ChannelRef};

/// Step 1: our reference for `record`. Send one per channel we hold a certificate for.
pub fn make_ref(record: &ChannelRecord, binding: &SessionBinding) -> ChannelMessage {
	ChannelMessage {
		body: Some(Body::MemberRef(ChannelRef {
			channel_ref: reference(&record.id, binding.digest(), binding.local_is_initiator())
				.to_vec(),
		})),
	}
}

fn matching<'a>(
	records: &'a [ChannelRecord],
	binding: &SessionBinding,
	channel_ref: &[u8],
) -> Option<&'a ChannelRecord> {
	// The peer computed its reference with its own role; ours would be the other value, so a
	// reflected reference of ours never matches.
	let sender_is_initiator = !binding.local_is_initiator();
	records.iter().find(|r| {
		ct_eq(
			&reference(&r.id, binding.digest(), sender_is_initiator),
			channel_ref,
		)
	})
}

/// Step 1 on receipt: the channel the peer's reference proves it knows, if we hold it too.
/// Only then may [`make_proof`] be sent for that channel on this link.
pub fn check_ref(
	records: &[ChannelRecord],
	binding: &SessionBinding,
	message: &ChannelMessage,
) -> Option<ChannelId> {
	let Some(Body::MemberRef(r)) = &message.body else {
		return None;
	};
	matching(records, binding, &r.channel_ref).map(|r| r.id)
}

/// Step 2: our certificate for `record`. Call only after [`check_ref`] matched this channel on
/// this link. `None` when we hold no unexpired certificate.
pub fn make_proof(
	record: &ChannelRecord,
	binding: &SessionBinding,
	now: u64,
) -> Option<ChannelMessage> {
	let cert = record.cert.as_ref().filter(|c| now < c.expires_at)?;
	let mut revocations: Vec<&Revocation> = record
		.revocations
		.iter()
		.filter(|r| now < r.forget_at)
		.collect();
	revocations.sort_by_key(|r| std::cmp::Reverse(r.forget_at));
	Some(ChannelMessage {
		body: Some(Body::Proof(ChannelProof {
			channel_ref: reference(&record.id, binding.digest(), binding.local_is_initiator())
				.to_vec(),
			cert: cert.to_bytes(),
			revocations: revocations
				.into_iter()
				.take(REVOCATIONS_PER_PROOF_MAX)
				.map(Revocation::to_bytes)
				.collect(),
		})),
	})
}

/// Result of checking a received proof.
#[derive(Debug, Default)]
pub struct ProofOutcome {
	/// The channel proven, when the proof was valid. Only this counts as shared.
	pub proven: Option<ChannelId>,
	/// The channel the reference matched, valid or not.
	pub matched: Option<ChannelId>,
	/// Verified revocations we did not have; the caller persists them.
	pub new_revocations: Vec<Revocation>,
	/// Expiry of the proven certificate, so callers can demote the evidence when it lapses.
	pub expires_at: Option<u64>,
}

/// Checks `message` received from `remote` (the link-authenticated fingerprint) against our
/// channels. Never reveals why a proof failed.
pub fn check_proof(
	records: &[ChannelRecord],
	binding: &SessionBinding,
	remote: &Fingerprint,
	message: &ChannelMessage,
	now: u64,
) -> ProofOutcome {
	let Some(Body::Proof(proof)) = &message.body else {
		return ProofOutcome::default();
	};
	let Some(record) = matching(records, binding, &proof.channel_ref) else {
		return ProofOutcome::default();
	};
	let mut outcome = ProofOutcome {
		matched: Some(record.id),
		..ProofOutcome::default()
	};
	for bytes in proof.revocations.iter().take(REVOCATIONS_PER_PROOF_MAX) {
		if let Ok(revocation) = Revocation::from_bytes(bytes)
			&& revocation.channel == record.id
			&& revocation.verify(&record.owner_public, now).is_ok()
			&& !record.revocations.contains(&revocation)
		{
			outcome.new_revocations.push(revocation);
		}
	}
	let revoked = record.is_revoked(remote, now)
		|| outcome
			.new_revocations
			.iter()
			.any(|r| r.covers(remote, now));
	let cert = MembershipCert::from_bytes(&proof.cert).ok().filter(|cert| {
		cert.channel == record.id && cert.verify(&record.owner_public, remote, now).is_ok()
	});
	if let Some(cert) = cert
		&& !revoked
	{
		outcome.proven = Some(record.id);
		outcome.expires_at = Some(cert.expires_at);
	}
	outcome
}
