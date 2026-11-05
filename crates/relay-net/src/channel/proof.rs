//! Membership proof between two devices that are already members of a channel.
//!
//! The initiator of the proof sends a session-bound reference (shows it knows the channel id),
//! its certificate and any revocations it holds. The verifier checks the certificate against
//! the owner key, the **authenticated** fingerprint of the link, expiry and revocations, and
//! only then answers with its own proof. A party that merely knows the id gets nothing.

use relay_core::channel::ChannelId;

use super::binding::SessionBinding;
use super::cert::{MembershipCert, Revocation};
use super::ids::{ct_eq, reference};
use super::limits::REVOCATIONS_PER_PROOF_MAX;
use super::store::ChannelRecord;
use crate::identity::Fingerprint;
use crate::proto::channel_message::Body;
use crate::proto::{ChannelMessage, ChannelProof};

/// Builds our proof for `record`. `None` when we hold no unexpired certificate.
pub fn make_proof(
	record: &ChannelRecord,
	binding: &SessionBinding,
	is_reply: bool,
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
			is_reply,
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
	let sender_is_initiator = !binding.local_is_initiator();
	let Some(record) = records.iter().find(|r| {
		ct_eq(
			&reference(&r.id, binding.digest(), sender_is_initiator),
			&proof.channel_ref,
		)
	}) else {
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
	let valid = MembershipCert::from_bytes(&proof.cert).is_ok_and(|cert| {
		cert.channel == record.id && cert.verify(&record.owner_public, remote, now).is_ok()
	});
	if valid && !revoked {
		outcome.proven = Some(record.id);
	}
	outcome
}
