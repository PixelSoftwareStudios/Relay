//! The joiner: turns a password, an invite or just a channel id into a membership
//! by running the PAKE exchange with a linked, trusted admitter.

use std::time::Duration;

use relay_core::channel::{CHANNELS_MAX, ChannelId};
use relay_net::channel::limits::{PASSWORD_MAX_BYTES, PASSWORD_MIN_BYTES};
use relay_net::channel::store::StoreError;
use relay_net::channel::{Credential, InviteToken, JoinClient};
use relay_net::proto::ChannelMessage;
use tokio::sync::oneshot;
use zeroize::Zeroizing;

use super::failure::Failure;
use super::hub::ChannelHub;
use super::state::JoinSlot;

/// An admitter gets this long to answer the whole exchange.
const EXCHANGE_TIMEOUT: Duration = Duration::from_secs(10);
/// A join that finds no trusted link waits this long for one to come up.
const LINK_WAIT_STEP: Duration = Duration::from_millis(100);
const LINK_WAIT_TRIES: u32 = 10;
/// At most this many linked peers are asked per join.
const ADMITTERS_TRIED: usize = 8;

/// A request to join a channel. Secrets are wiped when it is dropped.
pub struct JoinRequest {
	/// The channel id (16 bytes), if known; an invite carries it too.
	pub channel_id: Vec<u8>,
	/// A listed channel's name seen in discovery, when no id is known.
	pub name: String,
	/// The channel password, if it has one.
	pub password: Zeroizing<String>,
	/// An `RLYI1...` invite token, if the channel is invite-only.
	pub invite: Zeroizing<String>,
}

impl ChannelHub {
	/// Joins a channel through any trusted linked device that can admit.
	///
	/// Every failure after local validation is [`Failure::Denied`]: an unknown channel,
	/// a wrong password, a bad or spent invite, an admitter that is offline or locked
	/// out us all look alike. Only this device's own pacing says `LockedOut`.
	pub async fn join(&self, request: JoinRequest) -> Result<(), Failure> {
		let invite = if request.invite.trim().is_empty() {
			None
		} else {
			Some(InviteToken::parse(&request.invite).map_err(|_| Failure::Denied)?)
		};
		let channel = self.resolve(&request, invite.as_ref())?;
		if self.state().memberships.contains(&channel) {
			return Ok(());
		}
		if self.state().memberships.len() >= CHANNELS_MAX {
			return Err(Failure::Limit);
		}
		self.pace(&channel)?;
		let credential = self.credential(channel, &request, invite.as_ref()).await?;
		let candidates = self.admitters_soon(invite.as_ref()).await;
		let invite_id = invite.as_ref().map(|t| t.id);
		for peer in candidates {
			match self
				.try_admitter(peer, channel, &credential, invite_id)
				.await
			{
				Ok(()) => {
					self.inner
						.outbound
						.lock()
						.unwrap_or_else(|p| p.into_inner())
						.record_success(&self.inner.local, &channel);
					return Ok(());
				}
				Err(Failure::Limit) => return Err(Failure::Limit),
				Err(_) => {}
			}
		}
		self.inner
			.outbound
			.lock()
			.unwrap_or_else(|p| p.into_inner())
			.record_failure(self.mono(), &self.inner.local, &channel);
		Err(Failure::Denied)
	}

	fn resolve(
		&self,
		request: &JoinRequest,
		invite: Option<&InviteToken>,
	) -> Result<ChannelId, Failure> {
		let given = match request.channel_id.len() {
			0 => None,
			16 => Some(ChannelId::from_bytes(
				request.channel_id.as_slice().try_into().expect("16 bytes"),
			)),
			_ => return Err(Failure::Invalid("channel id must be 16 bytes")),
		};
		match (given, invite) {
			(Some(id), Some(token)) if id != token.channel => Err(Failure::Denied),
			(Some(id), _) => Ok(id),
			(None, Some(token)) => Ok(token.channel),
			(None, None) => self
				.state()
				.listed_seen
				.get(request.name.trim())
				.copied()
				.flatten()
				.ok_or(Failure::Denied),
		}
	}

	/// This device's own pacing, so a script cannot hammer one channel.
	fn pace(&self, channel: &ChannelId) -> Result<(), Failure> {
		self.inner
			.outbound
			.lock()
			.unwrap_or_else(|p| p.into_inner())
			.check(self.mono(), &self.inner.local, channel)
			.map_err(|locked| Failure::LockedOut {
				retry_after_secs: locked.retry_after.as_secs().max(1),
			})
	}

	async fn credential(
		&self,
		channel: ChannelId,
		request: &JoinRequest,
		invite: Option<&InviteToken>,
	) -> Result<Credential, Failure> {
		let password = (!request.password.is_empty()).then(|| request.password.clone());
		if let Some(p) = &password
			&& !(PASSWORD_MIN_BYTES..=PASSWORD_MAX_BYTES).contains(&p.len())
		{
			return Err(Failure::Denied);
		}
		let from_invite = invite.map(InviteToken::credential);
		let from_password = match password {
			// Argon2id is slow and memory-hard by design; keep it off the reactor.
			Some(p) => Some(
				tokio::task::spawn_blocking(move || Credential::from_password(&p, &channel))
					.await
					.map_err(|_| Failure::Storage)?
					.map_err(|_| Failure::Denied)?,
			),
			None => None,
		};
		Ok(match (from_password, from_invite) {
			(Some(p), Some(i)) => p.combine(&i),
			(Some(p), None) => p,
			(None, Some(i)) => i,
			(None, None) => Credential::open(&channel),
		})
	}

	/// Like [`Self::admitters`], but gives a link that is just coming up a moment.
	async fn admitters_soon(&self, invite: Option<&InviteToken>) -> Vec<relay_core::PeerId> {
		for _ in 0..LINK_WAIT_TRIES {
			let found = self.admitters(invite);
			if !found.is_empty() {
				return found;
			}
			tokio::time::sleep(LINK_WAIT_STEP).await;
		}
		self.admitters(invite)
	}

	/// Trusted linked devices that are not already busy joining, the invite's issuer first.
	fn admitters(&self, invite: Option<&InviteToken>) -> Vec<relay_core::PeerId> {
		let state = self.state();
		let mut peers: Vec<_> = state
			.links
			.iter()
			.filter(|(_, l)| l.join.is_none())
			.map(|(p, _)| *p)
			.collect();
		if let Some(token) = invite {
			peers.sort_by_key(|p| !token.issuer_matches(&crate::peers::fingerprint(*p)));
		}
		peers.truncate(ADMITTERS_TRIED);
		peers
	}

	async fn try_admitter(
		&self,
		peer: relay_core::PeerId,
		channel: ChannelId,
		credential: &Credential,
		invite_id: Option<[u8; 8]>,
	) -> Result<(), Failure> {
		let (done, result) = oneshot::channel();
		let start: ChannelMessage = {
			let mut state = self.state();
			let Some(link) = state.links.get_mut(&peer) else {
				return Err(Failure::Denied);
			};
			if link.join.is_some() {
				return Err(Failure::Denied);
			}
			let (client, start) = JoinClient::start(&link.binding, channel, credential, invite_id);
			link.join = Some(JoinSlot { client, done });
			start
		};
		self.send(peer, start);
		let outcome = tokio::time::timeout(EXCHANGE_TIMEOUT, result).await;
		if let Some(link) = self.state().links.get_mut(&peer) {
			link.join = None;
		}
		match outcome {
			Ok(Ok(result)) => result,
			_ => Err(Failure::Denied),
		}
	}

	/// Stores the record a grant produced and starts advertising the new channel.
	pub(super) fn accept_grant(
		&self,
		client: JoinClient,
		grant: &ChannelMessage,
	) -> Result<(), Failure> {
		let record = client
			.on_grant(grant, &self.inner.local, self.now())
			.map_err(|_| Failure::Denied)?;
		if self.state().memberships.contains(&record.id) {
			return Ok(());
		}
		let id = record.id;
		self.inner.store.put(record).map_err(|error| match error {
			StoreError::Full => Failure::Limit,
			other => {
				tracing::warn!(error = %other, "joined channel could not be stored");
				Failure::Storage
			}
		})?;
		self.reload()?;
		self.refresh_advert();
		self.announce_channel(id);
		Ok(())
	}
}
