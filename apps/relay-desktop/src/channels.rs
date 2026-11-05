//! Controller operations for channels, privacy, the connection string and
//! avatar download. Validation lives here; the engine has the final say.

use relay_client::connection::{self, ConnectionStringProblem};
use relay_client::{ChannelOptions, Secret};

use crate::controller::{ControlError, Controller, decode_id};
use crate::engine::Command;
use crate::model::{Advert, Channel, ConnectionView, Inbound};

const CHANNEL_ID_LEN: usize = 16;
const PEER_ID_LEN: usize = 32;
const NAME_MAX_CHARS: usize = 32;
const SECRET_MAX_CHARS: usize = 256;
pub const INVITE_PREFIX: &str = "RLYI1";
/// Invite lifetime bounds offered to the user.
pub const INVITE_TTL_MIN: u32 = 60;
pub const INVITE_TTL_MAX: u32 = 7 * 24 * 3600;
pub const INVITE_USES_MAX: u32 = 100;

impl From<ConnectionStringProblem> for ControlError {
	fn from(problem: ConnectionStringProblem) -> Self {
		Self::Invalid(problem.message())
	}
}

/// What a pasted join target means.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JoinTarget {
	Invite(String),
	ChannelId(Vec<u8>),
}

/// Classify the join field: an `RLYI1…` token, or a 32-hex channel id.
pub fn parse_join_target(text: &str) -> Result<JoinTarget, ControlError> {
	let text = text.trim();
	if text.starts_with(INVITE_PREFIX) {
		return if text.chars().count() <= SECRET_MAX_CHARS && !text.chars().any(char::is_whitespace)
		{
			Ok(JoinTarget::Invite(text.to_owned()))
		} else {
			Err(ControlError::Invalid("that invite token is malformed"))
		};
	}
	decode_id(
		text,
		CHANNEL_ID_LEN,
		"enter an invite token or a 32-character channel id",
	)
	.map(JoinTarget::ChannelId)
}

fn valid_text(text: &str, max: usize) -> bool {
	!text.is_empty() && text.chars().count() <= max && !text.chars().any(char::is_control)
}

impl Controller {
	fn channel(&self, id: &str) -> Result<(Vec<u8>, Channel), ControlError> {
		let bytes = decode_id(id, CHANNEL_ID_LEN, "malformed channel id")?;
		let channel = self
			.hub
			.read(|s| s.channels.iter().find(|c| c.channel_id == id).cloned())
			.ok_or(ControlError::Invalid("unknown channel"))?;
		Ok((bytes, channel))
	}

	pub async fn create_channel(
		&self,
		name: &str,
		options: ChannelOptions,
		password: &str,
	) -> Result<(), ControlError> {
		let name = name.trim();
		if !valid_text(name, NAME_MAX_CHARS) {
			return Err(ControlError::Invalid(
				"channel name must be 1 to 32 printable characters",
			));
		}
		if options.password && !valid_text(password, SECRET_MAX_CHARS) {
			return Err(ControlError::Invalid(
				"a password-protected channel needs a password",
			));
		}
		let password = if options.password {
			Secret::new(password)
		} else {
			Secret::new("")
		};
		let name = name.to_owned();
		self.run(Command::CreateChannel {
			name,
			options,
			password,
		})
		.await
		.map(drop)
	}

	/// Join with an invite token, or with a channel id plus password.
	pub async fn join_channel(&self, target: &str, password: &str) -> Result<(), ControlError> {
		let command = match parse_join_target(target)? {
			JoinTarget::Invite(token) => Command::JoinChannel {
				channel_id: Vec::new(),
				password: Secret::new(""),
				invite: Secret::new(token),
			},
			JoinTarget::ChannelId(channel_id) => {
				if !valid_text(password, SECRET_MAX_CHARS) {
					return Err(ControlError::Invalid("enter the channel password"));
				}
				Command::JoinChannel {
					channel_id,
					password: Secret::new(password),
					invite: Secret::new(""),
				}
			}
		};
		self.run(command).await.map(drop)
	}

	pub async fn leave_channel(&self, channel_id: &str) -> Result<(), ControlError> {
		let (id, _) = self.channel(channel_id)?;
		self.run(Command::LeaveChannel { channel_id: id })
			.await
			.map(drop)
	}

	/// Mint an invite. The token is a secret; the caller shows it once.
	pub async fn create_invite(
		&self,
		channel_id: &str,
		ttl_seconds: u32,
		uses: u32,
	) -> Result<Secret, ControlError> {
		let (id, channel) = self.channel(channel_id)?;
		if !channel.can_admin() {
			return Err(ControlError::Invalid(
				"only an owner or admin can create invites",
			));
		}
		if !(INVITE_TTL_MIN..=INVITE_TTL_MAX).contains(&ttl_seconds)
			|| !(1..=INVITE_USES_MAX).contains(&uses)
		{
			return Err(ControlError::Invalid(
				"invite lifetime or use count is out of range",
			));
		}
		let outcome = self
			.run(Command::CreateInvite {
				channel_id: id,
				ttl_seconds,
				uses,
			})
			.await?;
		outcome
			.invite
			.ok_or_else(|| ControlError::Engine("the engine returned no invite".into()))
	}

	/// Revoke a peer that proved membership of the channel. The peer id is the
	/// identity fingerprint the engine knows the member by.
	pub async fn revoke_member(&self, channel_id: &str, peer_id: &str) -> Result<(), ControlError> {
		let (id, channel) = self.channel(channel_id)?;
		if !channel.can_admin() {
			return Err(ControlError::Invalid(
				"only an owner or admin can revoke members",
			));
		}
		let fingerprint = decode_id(peer_id, PEER_ID_LEN, "malformed peer id")?;
		let member = self.hub.read(|s| {
			s.peers
				.iter()
				.any(|p| p.peer_id == peer_id && p.shared_channels.iter().any(|c| c == channel_id))
		});
		if !member {
			return Err(ControlError::Invalid(
				"that peer is not a proven member of the channel",
			));
		}
		self.run(Command::RevokeMember {
			channel_id: id,
			fingerprint,
		})
		.await
		.map(drop)
	}

	pub async fn set_privacy(
		&self,
		inbound: Option<Inbound>,
		advert: Option<Advert>,
	) -> Result<(), ControlError> {
		if inbound.is_none() && advert.is_none() {
			return Err(ControlError::Invalid("nothing to change"));
		}
		self.run(Command::SetPrivacy { inbound, advert })
			.await
			.map(drop)
	}

	/// Call a pasted `relay://` string after a local syntax check.
	pub async fn start_call_to(&self, text: &str) -> Result<(), ControlError> {
		connection::check(text)?;
		self.run(Command::StartCallConnection {
			text: text.trim().to_owned(),
		})
		.await
		.map(drop)
	}

	pub async fn connection_info(&self) -> Result<ConnectionView, ControlError> {
		let session = self.slot.get().ok_or(ControlError::NotConnected)?;
		Ok(session.ops.connection_info().await?)
	}

	/// The PNG for an avatar hash (hex), fetched from the engine.
	pub async fn avatar_png(&self, hash: &str) -> Result<Vec<u8>, ControlError> {
		let bytes = hex::decode(hash)
			.ok()
			.filter(|b| b.len() == 32)
			.ok_or(ControlError::Invalid("malformed avatar hash"))?;
		let session = self.slot.get().ok_or(ControlError::NotConnected)?;
		Ok(session.ops.avatar(bytes).await?)
	}
}
