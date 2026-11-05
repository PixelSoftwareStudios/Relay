//! Key and identifier derivations shared by tags, references and certificates.

use relay_core::channel::ChannelId;

const ID_DOMAIN: &str = "relay-net 2026 channel id v1";
const TAG_KEY_DOMAIN: &str = "relay-net 2026 channel tag key v1";
const REF_DOMAIN: &str = "relay-net 2026 channel reference v1";

/// Self-certifying id: derived from the owner's signing public key, so an admitter cannot swap
/// the owner key of a channel the joiner already knows by id.
pub fn channel_id(owner_public: &[u8; 32]) -> ChannelId {
	let digest = blake3::derive_key(ID_DOMAIN, owner_public);
	let mut id = [0u8; 16];
	id.copy_from_slice(&digest[..16]);
	ChannelId::from_bytes(id)
}

pub(crate) fn tag_key(id: &ChannelId) -> [u8; 32] {
	blake3::derive_key(TAG_KEY_DOMAIN, id.as_bytes())
}

/// Proof that the sender knows the channel id, bound to this session and the sender's role.
/// It reveals nothing about the id to a peer that does not already know it.
pub(crate) fn reference(
	id: &ChannelId,
	binding_digest: &[u8; 32],
	sender_is_initiator: bool,
) -> [u8; 16] {
	let mut hasher = blake3::Hasher::new_keyed(&tag_key(id));
	hasher.update(REF_DOMAIN.as_bytes());
	hasher.update(binding_digest);
	hasher.update(&[u8::from(sender_is_initiator)]);
	let mut out = [0u8; 16];
	out.copy_from_slice(&hasher.finalize().as_bytes()[..16]);
	out
}

/// Constant-time equality for equal-length byte strings.
pub(crate) fn ct_eq(a: &[u8], b: &[u8]) -> bool {
	a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

pub(crate) fn random<const N: usize>() -> [u8; N] {
	use rand::RngExt;
	let mut out = [0u8; N];
	rand::rng().fill(&mut out[..]);
	out
}
