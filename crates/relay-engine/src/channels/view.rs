//! What the driver tells the control service: channel info and who shares what.

use std::collections::BTreeMap;

use relay_api::pb::{ChannelInfo, ChannelPolicy, NearbyChannel};
use relay_core::PeerId;
use relay_core::channel::{PeerScope, Role};
use relay_net::channel::ChannelRecord;

/// Joined channels and, per linked peer, the channels it proved. No secrets.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ChannelView {
	/// One entry per joined channel.
	pub channels: Vec<ChannelInfo>,
	/// Channel ids each peer proved on its live trusted link.
	pub shared: BTreeMap<PeerId, Vec<Vec<u8>>>,
	/// Listed channels advertised nearby that this device is not in.
	pub nearby: Vec<NearbyChannel>,
}

fn role_name(role: Role) -> &'static str {
	match role {
		Role::Owner => "owner",
		Role::Admin => "admin",
		Role::Member => "member",
	}
}

/// The wire form of one record.
pub(super) fn info(record: &ChannelRecord, shared_peers: usize) -> ChannelInfo {
	ChannelInfo {
		channel_id: record.id.as_bytes().to_vec(),
		name: record.name.as_str().to_owned(),
		policy: Some(ChannelPolicy {
			listed: record.policy.listed,
			password: record.policy.password,
			invite_only: record.policy.invite_only,
		}),
		role: role_name(record.role).to_owned(),
		shared_peers: u32::try_from(shared_peers).unwrap_or(u32::MAX),
		cert_expires_unix: record.cert.as_ref().map_or(0, |c| c.expires_at),
	}
}

pub(super) fn build<'a>(
	records: &[ChannelRecord],
	scopes: impl Iterator<Item = (&'a PeerId, &'a PeerScope)>,
	nearby: Vec<NearbyChannel>,
) -> ChannelView {
	let mut shared: BTreeMap<PeerId, Vec<Vec<u8>>> = BTreeMap::new();
	let mut counts: BTreeMap<[u8; 16], usize> = BTreeMap::new();
	for (peer, scope) in scopes {
		for id in scope.proven() {
			*counts.entry(*id.as_bytes()).or_default() += 1;
			shared
				.entry(*peer)
				.or_default()
				.push(id.as_bytes().to_vec());
		}
	}
	let channels = records
		.iter()
		.map(|r| info(r, counts.get(r.id.as_bytes()).copied().unwrap_or(0)))
		.collect();
	ChannelView {
		channels,
		shared,
		nearby,
	}
}
