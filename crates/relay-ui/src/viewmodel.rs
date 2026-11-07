//! Window-less view model: everything a frame needs, computed from state.
//! Drawing code only reads these structs, so the logic is testable headless.

use crate::avatar::Avatar;
use crate::format::{TransmitStatus, describe_transmit, phase_label};
use crate::model::{Advert, AppState, Call, CallMeter, Channel, Link, Meters, Peer, Phase, Trust};
use crate::tr;

#[derive(Debug, Clone, PartialEq)]
pub struct CallCard {
	pub call_id: String,
	pub peer_id: String,
	pub name: String,
	pub avatar: Avatar,
	pub phase: Phase,
	pub subtitle: String,
	/// How the caller's trust reads, e.g. "Unknown" (shown on the incoming prompt).
	pub trust: String,
	/// The caller's short key digest, empty when unknown. A hint, never a verification.
	pub fingerprint: String,
	pub incoming: bool,
	/// The other device is not a contact: answering makes it one (unverified).
	pub unknown: bool,
	/// Controls such as mute and transmit mode are shown for these.
	pub controllable: bool,
	pub speaking: bool,
	pub receive_dbfs: Option<f32>,
	pub transmit: TransmitStatus,
	pub call: Call,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PeerRow {
	pub peer_id: String,
	pub name: String,
	pub avatar: Avatar,
	pub subtitle: String,
	/// Short key digest to tell devices apart (empty when the engine sent none).
	pub fingerprint: String,
	pub can_call: bool,
	/// Local IP address of the device (first one seen, without a port); empty when none.
	pub address: String,
	/// Every address seen for it, as the engine reported them.
	pub addresses: Vec<String>,
	/// Its listener port; zero when unknown.
	pub port: u32,
	/// When the engine last saw it (unix ms); zero when never.
	pub last_seen_ms: u64,
	/// The device answers on the network now.
	pub reachable: bool,
	/// A trust decision is stored for it (so it can be forgotten).
	pub known: bool,
	/// The user blocked it (listed only under Settings > Privacy).
	pub blocked: bool,
	/// Names of channels with proven shared membership.
	pub badges: Vec<String>,
	/// Channels (id, name) in which this device may revoke this peer.
	pub revocable: Vec<(String, String)>,
	/// On the favorites grid.
	pub favorite: bool,
	/// 0-based place among the favorites; zero when not a favorite.
	pub favorite_position: u32,
	/// The device presented a new key and is blocked until the user verifies it. `peer_id`
	/// and `fingerprint` are those of the new key.
	pub key_changed: bool,
	/// Short form of the pinned key; empty unless `key_changed`.
	pub previous_fingerprint: String,
	/// The safety code was compared: the only trust that allows answering automatically.
	pub verified: bool,
	/// Calls from it are answered without a tap.
	pub auto_answer: bool,
}

pub fn trust_label(trust: Trust) -> &'static str {
	match trust {
		Trust::Unknown => tr!("trust-unknown"),
		Trust::TofuUnverified => tr!("trust-tofu"),
		Trust::Verified => tr!("trust-verified"),
		Trust::KeyChanged => tr!("contact-key-changed-label"),
		Trust::Blocked => tr!("trust-blocked"),
	}
}

fn meter_for<'a>(meters: &'a Meters, call_id: &str) -> Option<&'a CallMeter> {
	meters.calls.iter().find(|m| m.call_id == call_id)
}

pub fn call_cards(state: &AppState, meters: &Meters) -> Vec<CallCard> {
	state
		.calls
		.iter()
		.filter(|c| !c.phase.is_terminal())
		.map(|call| {
			let peer = state.peers.iter().find(|p| p.peer_id == call.peer_id);
			let name = peer.map_or_else(
				|| tr!("peer-unknown").to_owned(),
				|p| p.display_name.clone(),
			);
			let avatar = Avatar::resolve(peer.map_or("", |p| p.avatar_hash.as_str()));
			let meter = meter_for(meters, &call.call_id);
			let connected = call.phase == Phase::Connected;
			let speaking = connected && meter.is_some_and(|m| m.speaking);
			let mut subtitle = phase_label(call.phase).to_owned();
			if speaking {
				subtitle.push_str(" · ");
				subtitle.push_str(tr!("call-speaking"));
			}
			CallCard {
				call_id: call.call_id.clone(),
				peer_id: call.peer_id.clone(),
				name,
				avatar,
				phase: call.phase,
				subtitle,
				trust: peer
					.map_or(tr!("trust-unknown"), |p| trust_label(p.trust))
					.to_owned(),
				fingerprint: peer.map_or_else(String::new, |p| p.fingerprint_short.clone()),
				incoming: call.phase == Phase::IncomingRinging,
				unknown: peer.is_none_or(|p| p.trust == Trust::Unknown),
				controllable: call.phase != Phase::IncomingRinging,
				speaking,
				receive_dbfs: if connected {
					meter.and_then(|m| m.receive_dbfs)
				} else {
					None
				},
				transmit: describe_transmit(call),
				call: call.clone(),
			}
		})
		.collect()
}

fn callable(state: &AppState, peer: &Peer) -> bool {
	peer.reachable
		&& !matches!(peer.trust, Trust::Blocked | Trust::KeyChanged)
		&& !state
			.calls
			.iter()
			.any(|c| c.peer_id == peer.peer_id && !c.phase.is_terminal())
}

/// Known channels the peer has proven membership of.
fn shared_channels<'a>(
	state: &'a AppState,
	peer: &'a Peer,
) -> impl Iterator<Item = &'a Channel> + 'a {
	state
		.channels
		.iter()
		.filter(|c| peer.shared_channels.contains(&c.channel_id))
}

/// The host of `address` without its port (`192.168.1.4:7000` is `192.168.1.4`).
pub fn host_of(address: &str) -> String {
	address
		.parse::<std::net::SocketAddr>()
		.map_or_else(|_| address.to_owned(), |a| a.ip().to_string())
}

pub fn peer_rows(state: &AppState) -> Vec<PeerRow> {
	state
		.peers
		.iter()
		.map(|p| PeerRow {
			peer_id: p.peer_id.clone(),
			name: p.display_name.clone(),
			avatar: Avatar::resolve(&p.avatar_hash),
			subtitle: format!(
				"{} · {}",
				if p.reachable {
					tr!("peer-online")
				} else {
					tr!("peer-unreachable")
				},
				trust_label(p.trust)
			),
			fingerprint: p.fingerprint_short.clone(),
			can_call: callable(state, p),
			address: p.addresses.first().map_or_else(String::new, |a| host_of(a)),
			addresses: p.addresses.clone(),
			port: p.port,
			last_seen_ms: p.last_seen_unix_ms,
			reachable: p.reachable,
			known: !matches!(p.trust, Trust::Unknown | Trust::Blocked),
			blocked: p.trust == Trust::Blocked,
			badges: shared_channels(state, p).map(|c| c.name.clone()).collect(),
			revocable: shared_channels(state, p)
				.filter(|c| c.can_admin())
				.map(|c| (c.channel_id.clone(), c.name.clone()))
				.collect(),
			favorite: p.favorite,
			favorite_position: p.favorite_position,
			key_changed: p.key_changed,
			previous_fingerprint: p.previous_fingerprint_short.clone(),
			verified: p.trust == Trust::Verified,
			auto_answer: p.auto_answer,
		})
		.collect()
}

/// The favorites in grid order (at most 6).
pub fn favorites(state: &AppState) -> Vec<PeerRow> {
	let mut rows: Vec<PeerRow> = peer_rows(state)
		.into_iter()
		.filter(|r| r.favorite)
		.collect();
	rows.sort_by_key(|r| r.favorite_position);
	rows
}

/// Avatar of the local profile.
pub fn local_avatar(state: &AppState) -> Avatar {
	Avatar::resolve(
		state
			.settings
			.as_ref()
			.map_or("", |s| s.avatar_hash.as_str()),
	)
}

/// Whether other devices on the network can find this one. The engine reports
/// the stored preference (`Settings.advert`) but not the live advertising
/// state, so this combines it with what the app knows: the daemon advertises
/// only while a client is attached, i.e. while the link is connected.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Discoverability {
	/// Advertised on the network: others can find and call this device.
	Discoverable,
	/// Not advertised: reachable only by someone with the address.
	Private,
	/// Meant to be advertised, but the daemon is not (the link state says why).
	NotAdvertising(Link),
}

/// `None` while the preference is unknown (no settings yet) and the link is up.
pub fn discoverability(state: &AppState) -> Option<Discoverability> {
	match (state.link, state.settings.as_ref().map(|s| s.advert)) {
		(Link::Connected, Some(Advert::Visible)) => Some(Discoverability::Discoverable),
		(Link::Connected, Some(Advert::Hidden)) => Some(Discoverability::Private),
		(Link::Connected, None) => None,
		(_, Some(Advert::Hidden)) => Some(Discoverability::Private),
		(link, _) => Some(Discoverability::NotAdvertising(link)),
	}
}
