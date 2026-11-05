//! In-memory channel, avatar and connection-info behavior for the fake engine.

use std::collections::HashMap;
use std::sync::Mutex;

use relay_api::pb::*;

const MAX_CHANNELS: usize = 16;
const LOCKOUT_AFTER: u32 = 3;

struct Channel {
	info: ChannelInfo,
	password: String,
	invites: Vec<String>,
	joined: bool,
	failures: u32,
}

#[derive(Default)]
pub struct Channels {
	channels: Mutex<Vec<Channel>>,
	avatars: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
	pub last_connection_string: Mutex<Option<String>>,
}

fn op(context: &Option<MutationContext>, problem: Option<&str>) -> Operation {
	Operation {
		operation_id: context
			.as_ref()
			.map(|c| c.operation_id.clone())
			.unwrap_or_default(),
		state: if problem.is_some() {
			OperationState::Failed as i32
		} else {
			OperationState::Applied as i32
		},
		problem: problem.map(|code| Problem {
			code: code.to_owned(),
			..Problem::default()
		}),
		..Operation::default()
	}
}

impl Channels {
	/// Test hook: a channel on another device that this device may join.
	pub fn add_remote(&self, id: &[u8], name: &str, password: &str) {
		self.channels.lock().unwrap().push(Channel {
			info: ChannelInfo {
				channel_id: id.to_vec(),
				name: name.to_owned(),
				role: "member".into(),
				..ChannelInfo::default()
			},
			password: password.to_owned(),
			invites: Vec::new(),
			joined: false,
			failures: 0,
		});
	}

	pub fn put_avatar(&self, hash: &[u8], png: Vec<u8>) {
		self.avatars.lock().unwrap().insert(hash.to_vec(), png);
	}

	pub fn avatar(&self, hash: &[u8]) -> Option<AvatarImage> {
		let png = self.avatars.lock().unwrap().get(hash)?.clone();
		Some(AvatarImage {
			avatar_sha256: hash.to_vec(),
			png,
			width: 128,
			height: 128,
		})
	}

	pub fn create(&self, req: &CreateChannelRequest) -> Operation {
		let len = req.name.trim().chars().count();
		if !(1..=48).contains(&len) {
			return op(&req.context, Some("CHANNEL_NAME_INVALID"));
		}
		let mut all = self.channels.lock().unwrap();
		if all.iter().filter(|c| c.joined).count() >= MAX_CHANNELS {
			return op(&req.context, Some("CHANNEL_LIMIT"));
		}
		let id = req
			.context
			.as_ref()
			.map(|c| c.operation_id.clone())
			.unwrap_or_default();
		all.push(Channel {
			info: ChannelInfo {
				channel_id: id,
				name: req.name.clone(),
				policy: req.policy,
				role: "owner".into(),
				..ChannelInfo::default()
			},
			password: req.password.clone(),
			invites: Vec::new(),
			joined: true,
			failures: 0,
		});
		op(&req.context, None)
	}

	pub fn join(&self, req: &JoinChannelRequest) -> Operation {
		let mut all = self.channels.lock().unwrap();
		// Unknown channel, wrong secret and lockout look alike except lockout.
		let Some(channel) = all.iter_mut().find(|c| c.info.channel_id == req.channel_id) else {
			return op(&req.context, Some("CHANNEL_DENIED"));
		};
		if channel.failures >= LOCKOUT_AFTER {
			return op(&req.context, Some("CHANNEL_LOCKED_OUT"));
		}
		let invite_ok = channel.invites.iter().position(|i| *i == req.invite);
		let secret_ok = if !req.invite.is_empty() {
			invite_ok.is_some()
		} else {
			channel.password == req.password
		};
		if !secret_ok {
			channel.failures += 1;
			return op(&req.context, Some("CHANNEL_DENIED"));
		}
		if let Some(i) = invite_ok {
			channel.invites.remove(i);
		}
		channel.joined = true;
		op(&req.context, None)
	}

	pub fn leave(&self, req: &LeaveChannelRequest) -> Operation {
		let mut all = self.channels.lock().unwrap();
		match all
			.iter()
			.position(|c| c.joined && c.info.channel_id == req.channel_id)
		{
			Some(i) => {
				all.remove(i);
				op(&req.context, None)
			}
			None => op(&req.context, Some("CHANNEL_DENIED")),
		}
	}

	pub fn list(&self) -> Vec<ChannelInfo> {
		let all = self.channels.lock().unwrap();
		all.iter()
			.filter(|c| c.joined)
			.map(|c| c.info.clone())
			.collect()
	}

	pub fn invite(&self, req: &CreateInviteRequest) -> Option<CreateInviteResponse> {
		let mut all = self.channels.lock().unwrap();
		let channel = all
			.iter_mut()
			.find(|c| c.joined && c.info.role == "owner" && c.info.channel_id == req.channel_id)?;
		let invite = format!("RLYI1{}", hex::encode(uuid::Uuid::new_v4().as_bytes()));
		channel.invites.push(invite.clone());
		Some(CreateInviteResponse {
			invite,
			expires_unix: 1_000 + u64::from(req.ttl_seconds),
		})
	}

	pub fn revoke(&self, req: &RevokeChannelMemberRequest) -> Operation {
		let all = self.channels.lock().unwrap();
		let owned = all
			.iter()
			.any(|c| c.joined && c.info.role == "owner" && c.info.channel_id == req.channel_id);
		op(&req.context, (!owned).then_some("CHANNEL_DENIED"))
	}
}
