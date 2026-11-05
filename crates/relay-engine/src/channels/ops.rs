//! Local channel management: create, leave, invite, revoke.

use relay_core::PeerId;
use relay_core::channel::{CHANNELS_MAX, ChannelId, ChannelName, Policy};
use relay_net::channel::limits::{
	CERT_TTL_MAX, INVITE_TTL_DEFAULT, INVITE_TTL_MAX, INVITE_USES_DEFAULT, INVITE_USES_MAX,
	PASSWORD_MAX_BYTES, PASSWORD_MIN_BYTES,
};
use relay_net::channel::store::StoreError;
use relay_net::channel::{CreateError, InviteError, InviteToken, new_channel};
use relay_net::identity::Fingerprint;
use zeroize::Zeroizing;

use super::failure::Failure;
use super::hub::ChannelHub;
use crate::peers::peer_id;

fn id_of(bytes: &[u8]) -> Result<ChannelId, Failure> {
	<[u8; 16]>::try_from(bytes)
		.map(ChannelId::from_bytes)
		.map_err(|_| Failure::Invalid("channel id must be 16 bytes"))
}

fn stored(error: StoreError) -> Failure {
	match error {
		StoreError::Full => Failure::Limit,
		StoreError::NotFound => Failure::Denied,
		StoreError::Backend(cause) => {
			tracing::warn!(%cause, "channel storage failed");
			Failure::Storage
		}
	}
}

impl ChannelHub {
	/// Creates a channel owned by this device and starts advertising it.
	pub async fn create(
		&self,
		name: &str,
		policy: Policy,
		password: Zeroizing<String>,
	) -> Result<ChannelId, Failure> {
		ChannelName::new(name).map_err(|_| Failure::NameInvalid)?;
		if policy.password != !password.is_empty() {
			return Err(Failure::Invalid("password does not match the policy"));
		}
		if policy.password && !(PASSWORD_MIN_BYTES..=PASSWORD_MAX_BYTES).contains(&password.len()) {
			return Err(Failure::Invalid("password must be 8 to 128 bytes"));
		}
		if self.state().memberships.len() >= CHANNELS_MAX {
			return Err(Failure::Limit);
		}
		let (name, local, now) = (name.to_owned(), self.inner.local, self.now());
		// Argon2id stretches the password; it is slow on purpose.
		let record = tokio::task::spawn_blocking(move || {
			let secret = policy.password.then_some(password.as_str());
			new_channel(&name, policy, secret, &local, now)
		})
		.await
		.map_err(|_| Failure::Storage)?
		.map_err(|error| match error {
			CreateError::Name(_) => Failure::NameInvalid,
			CreateError::Password(_) | CreateError::PasswordMismatch => {
				Failure::Invalid("password must be 8 to 128 bytes")
			}
		})?;
		let id = record.id;
		self.inner.store.put(record).map_err(stored)?;
		self.changed().await?;
		self.announce_channel(id);
		Ok(id)
	}

	/// Leaves a channel: its secrets and certificate are deleted and its tag stops
	/// being advertised.
	pub async fn leave(&self, channel_id: &[u8]) -> Result<(), Failure> {
		let id = id_of(channel_id)?;
		if !self.inner.store.remove(&id).map_err(stored)? {
			return Err(Failure::Denied);
		}
		self.changed().await
	}

	/// Mints an invite for an invite-only channel this device can admit for.
	/// A non-owner, an unknown channel and a channel without invites all answer
	/// `Denied`. Returns the secret token and its expiry (unix seconds).
	pub async fn invite(
		&self,
		channel_id: &[u8],
		ttl_seconds: u32,
		uses: u32,
	) -> Result<(String, u64), Failure> {
		let id = id_of(channel_id)?;
		let ttl = if ttl_seconds == 0 {
			INVITE_TTL_DEFAULT.as_secs()
		} else {
			u64::from(ttl_seconds)
		};
		let uses = if uses == 0 {
			INVITE_USES_DEFAULT
		} else {
			u8::try_from(uses).map_err(|_| Failure::Invalid("uses out of range"))?
		};
		if ttl > INVITE_TTL_MAX.as_secs() || uses > INVITE_USES_MAX {
			return Err(Failure::Invalid("invite lifetime or uses out of range"));
		}
		let (local, now) = (self.inner.local, self.now());
		let mut made: Option<Result<InviteToken, InviteError>> = None;
		self.inner
			.store
			.update(&id, &mut |record| {
				made = Some(record.new_invite(&local, now, ttl, uses));
			})
			.map_err(stored)?;
		match made {
			Some(Ok(token)) => Ok((token.encode(), now + ttl)),
			Some(Err(InviteError::TooMany)) => Err(Failure::Limit),
			Some(Err(InviteError::OutOfRange)) => Err(Failure::Invalid("invite out of range")),
			_ => Err(Failure::Denied),
		}
	}

	/// Revokes one member: the revocation is stored, pushed to the linked members
	/// that prove the channel, and the member's proof is no longer accepted here.
	pub async fn revoke(&self, channel_id: &[u8], fingerprint: &[u8]) -> Result<(), Failure> {
		let id = id_of(channel_id)?;
		let subject = <[u8; 32]>::try_from(fingerprint)
			.map(Fingerprint::from_bytes)
			.map_err(|_| Failure::Invalid("fingerprint must be 32 bytes"))?;
		let now = self.now();
		let revocation = {
			let state = self.state();
			let record = state.records.iter().find(|r| r.id == id);
			match record.and_then(|r| r.authority.as_ref()) {
				Some(authority) if subject != self.inner.local => {
					authority.revoke(&id, &subject, now + CERT_TTL_MAX.as_secs())
				}
				_ => return Err(Failure::Denied),
			}
		};
		self.inner
			.store
			.update(&id, &mut |record| {
				record.add_revocation(revocation.clone());
				record.issued.retain(|i| i.subject != subject);
			})
			.map_err(stored)?;
		self.reload()?;
		let revoked = peer_id(&subject);
		let audience: Vec<PeerId> = {
			let mut state = self.state();
			if let Some(scope) = state.scopes.get_mut(&revoked) {
				scope.forget(&id);
			}
			state
				.links
				.keys()
				.filter(|p| **p != revoked && state.proven(p).contains(&id))
				.copied()
				.collect()
		};
		for peer in audience {
			self.send_proofs(peer, true, Some(id));
		}
		self.publish().await;
		Ok(())
	}

	/// After any change to the joined set: reload, re-advertise, tell clients.
	async fn changed(&self) -> Result<(), Failure> {
		self.reload()?;
		self.refresh_advert();
		self.publish().await;
		Ok(())
	}
}
