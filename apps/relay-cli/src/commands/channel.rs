//! `relay channel create|join|leave|list|nearby|invite|revoke`.
//!
//! Passwords and invite tokens are read from a hidden prompt or one line of
//! stdin, never from arguments, and are not printed or logged. The only secret
//! written out is a freshly minted invite, which is the point of `invite`.

use std::fmt::{self, Display, Formatter};

use relay_api::pb::{ChannelInfo, ChannelPolicy, NearbyChannel};
use serde::Serialize;

use super::resolve::resolve_channel;
use super::table::{self, Layout};
use super::trust::parse_fingerprint;
use super::{Out, names, outcome, report};
use crate::cli::{ChannelCommand, ChannelCreate, ChannelJoin, CliError};
use crate::client::{ChannelRef, Credential, Manage};
use crate::secret::SecretInput;

/// One channel row.
#[derive(Debug, Serialize)]
pub struct ChannelView {
	pub id: String,
	pub name: String,
	pub role: String,
	pub listed: bool,
	pub password: bool,
	pub invite_only: bool,
	pub shared_peers: u32,
}

impl From<&ChannelInfo> for ChannelView {
	fn from(c: &ChannelInfo) -> Self {
		let policy = c.policy.unwrap_or_default();
		Self {
			id: names::hex_id(&c.channel_id),
			name: c.name.clone(),
			role: c.role.clone(),
			listed: policy.listed,
			password: policy.password,
			invite_only: policy.invite_only,
			shared_peers: c.shared_peers,
		}
	}
}

/// All channels.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct ChannelList(pub Vec<ChannelView>);

impl ChannelList {
	/// Text table, or the empty-result message.
	pub fn render(&self, layout: Layout) -> String {
		if self.0.is_empty() {
			return "no channels".into();
		}
		let mut t = table::with_header(layout, &["ID", "NAME", "ROLE", "ACCESS", "PEERS"]);
		for c in &self.0 {
			let mut access = Vec::new();
			if !c.listed {
				access.push("hidden");
			}
			if c.password {
				access.push("password");
			}
			if c.invite_only {
				access.push("invite");
			}
			if access.is_empty() {
				access.push("open");
			}
			t.add_row([
				c.id.chars().take(8).collect::<String>(),
				c.name.clone(),
				c.role.clone(),
				access.join(", "),
				c.shared_peers.to_string(),
			]);
		}
		table::render(t)
	}
}

impl Display for ChannelList {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(&self.render(Layout::detect()))
	}
}

/// One channel listed on the network.
#[derive(Debug, Serialize)]
pub struct NearbyView {
	pub name: String,
	pub devices: u32,
	/// Two channels use this name: join it with its channel id (join code), not by name.
	pub ambiguous: bool,
}

impl From<&NearbyChannel> for NearbyView {
	fn from(c: &NearbyChannel) -> Self {
		Self {
			name: c.name.clone(),
			devices: c.devices,
			ambiguous: c.ambiguous,
		}
	}
}

/// Channels listed on the network.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct NearbyList(pub Vec<NearbyView>);

impl NearbyList {
	/// Text table, or the empty-result message.
	pub fn render(&self, layout: Layout) -> String {
		if self.0.is_empty() {
			return "no channels found on your network".into();
		}
		let mut t = table::with_header(layout, &["NAME", "DEVICES", "NOTE"]);
		for c in &self.0 {
			t.add_row([
				c.name.clone(),
				c.devices.to_string(),
				if c.ambiguous {
					"ambiguous: two channels use this name, join with its id".to_owned()
				} else {
					String::new()
				},
			]);
		}
		table::render(t)
	}
}

impl Display for NearbyList {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(&self.render(Layout::detect()))
	}
}

/// A minted invite. The text form is the bare token so it can be piped.
#[derive(Debug, Serialize)]
pub struct InviteView {
	pub invite: String,
	pub expires_unix: u64,
}

impl Display for InviteView {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(&self.invite)
	}
}

pub async fn run<C: Manage>(
	client: &mut C,
	cmd: ChannelCommand,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	match cmd {
		ChannelCommand::Create(args) => create(client, &args, out).await,
		ChannelCommand::Join(args) => join(client, &args, out).await,
		ChannelCommand::List => {
			let channels = client.list_channels().await?;
			out.emit(&ChannelList(
				channels.iter().map(ChannelView::from).collect(),
			))
		}
		ChannelCommand::Nearby => {
			let nearby = client.nearby_channels().await?;
			out.emit(&NearbyList(nearby.iter().map(NearbyView::from).collect()))
		}
		ChannelCommand::Leave { channel } => {
			let id = channel_id(client, &channel).await?;
			report(&client.leave_channel(&id).await?, out)
		}
		ChannelCommand::Invite {
			channel,
			ttl_seconds,
			uses,
		} => {
			let id = channel_id(client, &channel).await?;
			let invite = client.create_invite(&id, ttl_seconds, uses).await?;
			// A refused invite is a FAILED operation with an empty token.
			if let Some(op) = &invite.operation {
				outcome(op)?;
			}
			if invite.invite.is_empty() {
				return Err(CliError::Rejected {
					code: "no_invite".into(),
					message: "the engine did not mint an invite".into(),
					retryable: false,
				});
			}
			out.emit(&InviteView {
				invite: invite.invite,
				expires_unix: invite.expires_unix,
			})
		}
		ChannelCommand::Revoke {
			channel,
			fingerprint,
		} => {
			let fingerprint = parse_fingerprint(&fingerprint)?;
			let id = channel_id(client, &channel).await?;
			report(&client.revoke_member(&id, &fingerprint).await?, out)
		}
	}
}

async fn channel_id<C: Manage>(client: &mut C, query: &str) -> Result<Vec<u8>, CliError> {
	let channels = client.list_channels().await?;
	Ok(resolve_channel(&channels, query)?.channel_id.clone())
}

async fn create<C: Manage>(
	client: &mut C,
	args: &ChannelCreate,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let password = if args.password {
		let first = out.secret(SecretInput::Prompt("Channel password"))?;
		let again = out.secret(SecretInput::Prompt("Repeat password"))?;
		if first != again {
			return Err(CliError::Usage("the passwords do not match".into()));
		}
		Some(first)
	} else if args.password_stdin {
		Some(out.secret(SecretInput::Stdin)?)
	} else {
		None
	};
	let policy = ChannelPolicy {
		listed: !args.hidden,
		password: password.is_some(),
		invite_only: args.invite_only,
	};
	let op = client
		.create_channel(&args.name, policy, password.as_ref())
		.await?;
	report(&op, out)
}

async fn join<C: Manage>(
	client: &mut C,
	args: &ChannelJoin,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let credential = if args.password {
		Credential::Password(out.secret(SecretInput::Prompt("Channel password"))?)
	} else if args.password_stdin {
		Credential::Password(out.secret(SecretInput::Stdin)?)
	} else if args.invite {
		Credential::Invite(out.secret(SecretInput::Prompt("Invite token"))?)
	} else if args.invite_stdin {
		Credential::Invite(out.secret(SecretInput::Stdin)?)
	} else {
		Credential::None
	};
	let target = match hex::decode(&args.channel) {
		Ok(id) if id.len() == 16 => ChannelRef::Id(id),
		_ => ChannelRef::Name(args.channel.clone()),
	};
	let op = client.join_channel(target, credential).await?;
	report(&op, out)
}
