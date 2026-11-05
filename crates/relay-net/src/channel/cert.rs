//! Owner-signed membership certificates, admin delegations and revocations (Ed25519).
//!
//! A certificate names the member's **Noise static key fingerprint**, so copying it is useless:
//! the link handshake already proved who holds the key, and verification compares the subject
//! with that authenticated fingerprint. Expiry and revocation semantics are in
//! `docs/channels.md`; there is no online revocation list.

use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use relay_core::channel::ChannelId;
use thiserror::Error;
use zeroize::Zeroizing;

use super::ids::channel_id;
use super::limits::{CERT_TTL_MAX, DELEGATION_TTL_MAX};
use crate::identity::Fingerprint;

const CERT_LABEL: &[u8] = b"relay-net 2026 membership cert v1\0";
const DELEGATION_LABEL: &[u8] = b"relay-net 2026 admin delegation v1\0";
const REVOCATION_LABEL: &[u8] = b"relay-net 2026 membership revocation v1\0";

#[derive(Debug, Error, PartialEq, Eq)]
pub enum CertError {
	#[error("malformed certificate")]
	Malformed,
	#[error("certificate is for another channel")]
	WrongChannel,
	#[error("certificate names another device")]
	WrongSubject,
	#[error("certificate has expired")]
	Expired,
	#[error("issuer is not the owner or a valid delegate")]
	BadIssuer,
	#[error("signature does not verify")]
	BadSignature,
}

/// Owner-signed authority for one admin key to sign certificates until `expires_at`.
#[derive(Clone, PartialEq, Eq)]
pub struct Delegation {
	pub admin: [u8; 32],
	pub expires_at: u64,
	signature: [u8; 64],
}

pub const DELEGATION_BYTES: usize = 32 + 8 + 64;

impl Delegation {
	fn payload(channel: &ChannelId, admin: &[u8; 32], expires_at: u64) -> Vec<u8> {
		let mut out = DELEGATION_LABEL.to_vec();
		out.extend_from_slice(channel.as_bytes());
		out.extend_from_slice(admin);
		out.extend_from_slice(&expires_at.to_le_bytes());
		out
	}

	fn verify(&self, owner: &[u8; 32], channel: &ChannelId, now: u64) -> Result<(), CertError> {
		if now >= self.expires_at {
			return Err(CertError::BadIssuer);
		}
		verify(
			owner,
			&Self::payload(channel, &self.admin, self.expires_at),
			&self.signature,
		)
		.map_err(|_| CertError::BadIssuer)
	}

	pub fn to_bytes(&self) -> [u8; DELEGATION_BYTES] {
		let mut out = [0u8; DELEGATION_BYTES];
		out[..32].copy_from_slice(&self.admin);
		out[32..40].copy_from_slice(&self.expires_at.to_le_bytes());
		out[40..].copy_from_slice(&self.signature);
		out
	}

	pub fn from_bytes(bytes: &[u8]) -> Result<Self, CertError> {
		let bytes: &[u8; DELEGATION_BYTES] = bytes.try_into().map_err(|_| CertError::Malformed)?;
		Ok(Self {
			admin: bytes[..32].try_into().expect("fixed slice"),
			expires_at: u64::from_le_bytes(bytes[32..40].try_into().expect("fixed slice")),
			signature: bytes[40..].try_into().expect("fixed slice"),
		})
	}
}

impl std::fmt::Debug for Delegation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Delegation(expires {})", self.expires_at)
	}
}

/// A signing authority: the owner, or an admin carrying its delegation.
#[derive(Clone)]
pub struct Authority {
	seed: Zeroizing<[u8; 32]>,
	delegation: Option<Delegation>,
}

impl Authority {
	pub fn owner(seed: [u8; 32]) -> Self {
		Self {
			seed: Zeroizing::new(seed),
			delegation: None,
		}
	}

	pub fn admin(seed: [u8; 32], delegation: Delegation) -> Self {
		Self {
			seed: Zeroizing::new(seed),
			delegation: Some(delegation),
		}
	}

	fn key(&self) -> SigningKey {
		SigningKey::from_bytes(&self.seed)
	}

	pub fn public(&self) -> [u8; 32] {
		self.key().verifying_key().to_bytes()
	}

	pub fn seed(&self) -> &[u8; 32] {
		&self.seed
	}

	pub fn delegation(&self) -> Option<&Delegation> {
		self.delegation.as_ref()
	}

	pub fn is_owner(&self) -> bool {
		self.delegation.is_none()
	}

	/// Owner only: authorises `admin` to issue certificates until `now + ttl` (capped).
	pub fn delegate(
		&self,
		channel: &ChannelId,
		admin: [u8; 32],
		now: u64,
		ttl_secs: u64,
	) -> Option<Delegation> {
		if !self.is_owner() {
			return None;
		}
		let expires_at = now.saturating_add(ttl_secs.min(DELEGATION_TTL_MAX.as_secs()));
		let payload = Delegation::payload(channel, &admin, expires_at);
		Some(Delegation {
			admin,
			expires_at,
			signature: self.key().sign(&payload).to_bytes(),
		})
	}

	fn bound(&self) -> u64 {
		self.delegation.as_ref().map_or(u64::MAX, |d| d.expires_at)
	}

	/// Issues a certificate for `subject`; the lifetime is capped by policy and by the
	/// delegation, if any.
	pub fn issue(
		&self,
		channel: &ChannelId,
		subject: &Fingerprint,
		now: u64,
		ttl_secs: u64,
	) -> MembershipCert {
		let expires_at = now
			.saturating_add(ttl_secs.min(CERT_TTL_MAX.as_secs()))
			.min(self.bound());
		let issuer = self.public();
		let payload = MembershipCert::payload(channel, subject.as_bytes(), expires_at, &issuer);
		MembershipCert {
			channel: *channel,
			subject: *subject.as_bytes(),
			expires_at,
			issuer,
			delegation: self.delegation.clone(),
			signature: self.key().sign(&payload).to_bytes(),
		}
	}

	/// Signs a revocation of `subject`, relevant until `forget_at`.
	pub fn revoke(&self, channel: &ChannelId, subject: &Fingerprint, forget_at: u64) -> Revocation {
		let issuer = self.public();
		let payload = Revocation::payload(channel, subject.as_bytes(), forget_at, &issuer);
		Revocation {
			channel: *channel,
			subject: *subject.as_bytes(),
			forget_at,
			issuer,
			delegation: self.delegation.clone(),
			signature: self.key().sign(&payload).to_bytes(),
		}
	}
}

impl std::fmt::Debug for Authority {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str(if self.is_owner() {
			"Authority(owner)"
		} else {
			"Authority(admin)"
		})
	}
}

fn verify(public: &[u8; 32], payload: &[u8], signature: &[u8; 64]) -> Result<(), CertError> {
	let key = VerifyingKey::from_bytes(public).map_err(|_| CertError::BadSignature)?;
	key.verify_strict(payload, &Signature::from_bytes(signature))
		.map_err(|_| CertError::BadSignature)
}

/// Checks that `issuer` is the owner or holds a valid delegation; returns the time until which
/// the issuer's authority lasts.
fn check_issuer(
	owner: &[u8; 32],
	channel: &ChannelId,
	issuer: &[u8; 32],
	delegation: Option<&Delegation>,
	now: u64,
) -> Result<u64, CertError> {
	if channel_id(owner) != *channel {
		return Err(CertError::WrongChannel);
	}
	if issuer == owner {
		return Ok(u64::MAX);
	}
	let delegation = delegation.ok_or(CertError::BadIssuer)?;
	if &delegation.admin != issuer {
		return Err(CertError::BadIssuer);
	}
	delegation.verify(owner, channel, now)?;
	Ok(delegation.expires_at)
}

#[derive(Clone, PartialEq, Eq)]
pub struct MembershipCert {
	pub channel: ChannelId,
	pub subject: [u8; 32],
	pub expires_at: u64,
	pub issuer: [u8; 32],
	pub delegation: Option<Delegation>,
	signature: [u8; 64],
}

impl MembershipCert {
	fn payload(
		channel: &ChannelId,
		subject: &[u8; 32],
		expires_at: u64,
		issuer: &[u8; 32],
	) -> Vec<u8> {
		let mut out = CERT_LABEL.to_vec();
		out.extend_from_slice(channel.as_bytes());
		out.extend_from_slice(subject);
		out.extend_from_slice(&expires_at.to_le_bytes());
		out.extend_from_slice(issuer);
		out
	}

	/// Verifies for membership proof: the signature, the issuer chain to `owner`, the subject
	/// against the fingerprint authenticated by the link, and expiry at `now`.
	pub fn verify(
		&self,
		owner: &[u8; 32],
		subject: &Fingerprint,
		now: u64,
	) -> Result<(), CertError> {
		self.verify_with_grace(owner, subject, now, 0)
	}

	/// Like [`Self::verify`] but accepts a certificate expired by at most `grace_secs`. Used
	/// only to authorise a renewal, never to prove membership.
	pub fn verify_with_grace(
		&self,
		owner: &[u8; 32],
		subject: &Fingerprint,
		now: u64,
		grace_secs: u64,
	) -> Result<(), CertError> {
		let bound = check_issuer(
			owner,
			&self.channel,
			&self.issuer,
			self.delegation.as_ref(),
			now,
		)?;
		if &self.subject != subject.as_bytes() {
			return Err(CertError::WrongSubject);
		}
		if now >= self.expires_at.saturating_add(grace_secs) || self.expires_at > bound {
			return Err(CertError::Expired);
		}
		verify(
			&self.issuer,
			&Self::payload(&self.channel, &self.subject, self.expires_at, &self.issuer),
			&self.signature,
		)
	}

	pub fn to_bytes(&self) -> Vec<u8> {
		let mut out = Vec::with_capacity(16 + 32 + 8 + 32 + 1 + DELEGATION_BYTES + 64);
		out.extend_from_slice(self.channel.as_bytes());
		out.extend_from_slice(&self.subject);
		out.extend_from_slice(&self.expires_at.to_le_bytes());
		out.extend_from_slice(&self.issuer);
		encode_delegation(&mut out, self.delegation.as_ref());
		out.extend_from_slice(&self.signature);
		out
	}

	pub fn from_bytes(bytes: &[u8]) -> Result<Self, CertError> {
		let mut r = Reader(bytes);
		let channel = ChannelId::from_bytes(r.array::<16>()?);
		let subject = r.array::<32>()?;
		let expires_at = u64::from_le_bytes(r.array::<8>()?);
		let issuer = r.array::<32>()?;
		let delegation = r.delegation()?;
		let signature = r.array::<64>()?;
		r.finish()?;
		Ok(Self {
			channel,
			subject,
			expires_at,
			issuer,
			delegation,
			signature,
		})
	}
}

impl std::fmt::Debug for MembershipCert {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(
			f,
			"MembershipCert({}, expires {})",
			self.channel, self.expires_at
		)
	}
}

/// A signed statement that `subject` is no longer a member. Valid until `forget_at`, after
/// which any certificate it could have covered has expired anyway.
#[derive(Clone, PartialEq, Eq)]
pub struct Revocation {
	pub channel: ChannelId,
	pub subject: [u8; 32],
	pub forget_at: u64,
	pub issuer: [u8; 32],
	pub delegation: Option<Delegation>,
	signature: [u8; 64],
}

impl Revocation {
	fn payload(
		channel: &ChannelId,
		subject: &[u8; 32],
		forget_at: u64,
		issuer: &[u8; 32],
	) -> Vec<u8> {
		let mut out = REVOCATION_LABEL.to_vec();
		out.extend_from_slice(channel.as_bytes());
		out.extend_from_slice(subject);
		out.extend_from_slice(&forget_at.to_le_bytes());
		out.extend_from_slice(issuer);
		out
	}

	pub fn verify(&self, owner: &[u8; 32], now: u64) -> Result<(), CertError> {
		check_issuer(
			owner,
			&self.channel,
			&self.issuer,
			self.delegation.as_ref(),
			now,
		)?;
		verify(
			&self.issuer,
			&Self::payload(&self.channel, &self.subject, self.forget_at, &self.issuer),
			&self.signature,
		)
	}

	pub fn covers(&self, subject: &Fingerprint, now: u64) -> bool {
		&self.subject == subject.as_bytes() && now < self.forget_at
	}

	pub fn to_bytes(&self) -> Vec<u8> {
		let mut out = Vec::with_capacity(16 + 32 + 8 + 32 + 1 + DELEGATION_BYTES + 64);
		out.extend_from_slice(self.channel.as_bytes());
		out.extend_from_slice(&self.subject);
		out.extend_from_slice(&self.forget_at.to_le_bytes());
		out.extend_from_slice(&self.issuer);
		encode_delegation(&mut out, self.delegation.as_ref());
		out.extend_from_slice(&self.signature);
		out
	}

	pub fn from_bytes(bytes: &[u8]) -> Result<Self, CertError> {
		let mut r = Reader(bytes);
		let channel = ChannelId::from_bytes(r.array::<16>()?);
		let subject = r.array::<32>()?;
		let forget_at = u64::from_le_bytes(r.array::<8>()?);
		let issuer = r.array::<32>()?;
		let delegation = r.delegation()?;
		let signature = r.array::<64>()?;
		r.finish()?;
		Ok(Self {
			channel,
			subject,
			forget_at,
			issuer,
			delegation,
			signature,
		})
	}
}

impl std::fmt::Debug for Revocation {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		write!(f, "Revocation({})", self.channel)
	}
}

fn encode_delegation(out: &mut Vec<u8>, delegation: Option<&Delegation>) {
	match delegation {
		Some(d) => {
			out.push(1);
			out.extend_from_slice(&d.to_bytes());
		}
		None => out.push(0),
	}
}

struct Reader<'a>(&'a [u8]);

impl Reader<'_> {
	fn take(&mut self, n: usize) -> Result<&[u8], CertError> {
		if self.0.len() < n {
			return Err(CertError::Malformed);
		}
		let (head, rest) = self.0.split_at(n);
		self.0 = rest;
		Ok(head)
	}

	fn array<const N: usize>(&mut self) -> Result<[u8; N], CertError> {
		self.take(N)?.try_into().map_err(|_| CertError::Malformed)
	}

	fn delegation(&mut self) -> Result<Option<Delegation>, CertError> {
		match self.take(1)?[0] {
			0 => Ok(None),
			1 => Delegation::from_bytes(self.take(DELEGATION_BYTES)?).map(Some),
			_ => Err(CertError::Malformed),
		}
	}

	fn finish(&self) -> Result<(), CertError> {
		if self.0.is_empty() {
			Ok(())
		} else {
			Err(CertError::Malformed)
		}
	}
}
