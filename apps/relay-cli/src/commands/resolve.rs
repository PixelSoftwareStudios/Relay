//! Turning what the user typed into ids from the current snapshot.

use std::net::SocketAddr;

use relay_api::pb::{Call, ChannelInfo, Device, DeviceDirection, Peer, Snapshot};

use super::names;
use crate::cli::TargetError;

/// A manual call target typed as `ip:port`.
pub fn parse_address(query: &str) -> Option<String> {
	query.parse::<SocketAddr>().ok().map(|a| a.to_string())
}

/// Choose the single best candidate: an exact match wins, then a unique prefix.
fn pick<'a, T>(
	kind: &'static str,
	query: &str,
	items: &'a [T],
	exact: impl Fn(&T) -> bool,
	prefix: impl Fn(&T) -> bool,
	label: impl Fn(&T) -> String,
) -> Result<&'a T, TargetError> {
	for matcher in [&exact as &dyn Fn(&T) -> bool, &prefix] {
		let hits: Vec<&T> = items.iter().filter(|i| matcher(i)).collect();
		match hits.as_slice() {
			[] => {}
			[one] => return Ok(one),
			many => {
				return Err(TargetError::Ambiguous {
					query: query.to_owned(),
					candidates: many.iter().map(|i| label(i)).collect::<Vec<_>>().join(", "),
				});
			}
		}
	}
	Err(TargetError::NotFound {
		kind,
		query: query.to_owned(),
	})
}

/// Find a peer by exact id, exact name (any case), id prefix or name prefix.
pub fn resolve_peer<'a>(snapshot: &'a Snapshot, query: &str) -> Result<&'a Peer, TargetError> {
	let q = query.to_lowercase();
	pick(
		"peer",
		query,
		&snapshot.peers,
		|p| names::hex_id(&p.peer_id) == q || p.display_name.to_lowercase() == q,
		|p| {
			names::hex_id(&p.peer_id).starts_with(&q)
				|| p.display_name.to_lowercase().starts_with(&q)
		},
		|p| format!("{} ({})", p.display_name, names::short_id(&p.peer_id)),
	)
}

/// Find a call by id prefix or by its peer; with no query, the only live call.
pub fn resolve_call<'a>(
	snapshot: &'a Snapshot,
	query: Option<&str>,
) -> Result<&'a Call, TargetError> {
	let live: Vec<Call> = snapshot
		.calls
		.iter()
		.filter(|c| names::call_is_live(c.state))
		.cloned()
		.collect();
	let found = match query {
		None => match live.as_slice() {
			[one] => Some(one.call_id.clone()),
			[] => return Err(TargetError::NoDefault("there is no active call".into())),
			_ => {
				return Err(TargetError::NoDefault(
					"several calls are active; name one with its id or peer".into(),
				));
			}
		},
		Some(q) => {
			let q = q.to_lowercase();
			let by_id = pick(
				"call",
				&q,
				&live,
				|c| names::hex_id(&c.call_id) == q,
				|c| names::hex_id(&c.call_id).starts_with(&q),
				|c| names::short_id(&c.call_id),
			);
			match by_id {
				Ok(call) => Some(call.call_id.clone()),
				Err(TargetError::NotFound { .. }) => {
					let peer = resolve_peer(snapshot, &q)?;
					let hit = live.iter().find(|c| c.peer_id == peer.peer_id).ok_or(
						TargetError::NotFound {
							kind: "call",
							query: q.clone(),
						},
					)?;
					Some(hit.call_id.clone())
				}
				Err(other) => return Err(other),
			}
		}
	};
	let id = found.unwrap_or_default();
	snapshot
		.calls
		.iter()
		.find(|c| c.call_id == id)
		.ok_or(TargetError::NotFound {
			kind: "call",
			query: query.unwrap_or_default().to_owned(),
		})
}

/// Find a device of one direction by exact id, exact name or prefix.
pub fn resolve_device<'a>(
	snapshot: &'a Snapshot,
	query: &str,
	direction: DeviceDirection,
) -> Result<&'a Device, TargetError> {
	let q = query.to_lowercase();
	let of_direction: Vec<Device> = snapshot
		.devices
		.iter()
		.filter(|d| d.direction == direction as i32)
		.cloned()
		.collect();
	let device = pick(
		"device",
		query,
		&of_direction,
		|d| d.device_id.to_lowercase() == q || d.name.to_lowercase() == q,
		|d| d.device_id.to_lowercase().starts_with(&q) || d.name.to_lowercase().starts_with(&q),
		|d| format!("{} ({})", d.name, d.device_id),
	)?;
	snapshot
		.devices
		.iter()
		.find(|d| d.device_id == device.device_id && d.direction == direction as i32)
		.ok_or(TargetError::NotFound {
			kind: "device",
			query: query.to_owned(),
		})
}

/// Find one of the user's channels by exact name, id prefix or name prefix.
pub fn resolve_channel<'a>(
	channels: &'a [ChannelInfo],
	query: &str,
) -> Result<&'a ChannelInfo, TargetError> {
	let q = query.to_lowercase();
	pick(
		"channel",
		query,
		channels,
		|c| c.name.to_lowercase() == q || names::hex_id(&c.channel_id) == q,
		|c| c.name.to_lowercase().starts_with(&q) || names::hex_id(&c.channel_id).starts_with(&q),
		|c| format!("{} ({})", c.name, names::short_id(&c.channel_id)),
	)
}
