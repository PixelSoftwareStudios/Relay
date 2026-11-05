//! What the engine knows besides calls: peers, devices and local settings.

use std::collections::{BTreeMap, BTreeSet};

use relay_api::pb::{Removed, StateEvent, state_event};
use relay_core::PeerId;

use super::convert::{device_to_pb, peer_to_pb};
use super::ports::{
	AdvertMode, DeviceInfo, InboundPolicy, LocalProfile, PeerInfo, PeerTrust, StoredSettings,
};
use crate::channels::ChannelView;

const MAX_FLAGGED: usize = 16;
const MAX_WARNINGS: usize = 32;

/// Local settings that live outside the domain core.
#[derive(Debug, Clone, PartialEq)]
pub struct Aux {
	/// Selected input device id.
	pub input_device: String,
	/// Selected output device id.
	pub output_device: String,
	/// Qualified noise suppression backend id.
	pub noise_backend: String,
	/// Microphone gain.
	pub microphone_gain: f32,
	/// Advertised display name.
	pub display_name: String,
	/// Advertised avatar hash.
	pub avatar: Option<[u8; 32]>,
	/// Who may ring this device.
	pub inbound_policy: InboundPolicy,
	/// How the device appears in discovery.
	pub advert_mode: AdvertMode,
	/// The desktop app starts the daemon by itself.
	pub autostart_daemon: bool,
}

impl Aux {
	/// What the peer adapter advertises.
	pub fn profile(&self) -> LocalProfile {
		LocalProfile {
			display_name: self.display_name.clone(),
			avatar: self.avatar,
			hidden: self.advert_mode == AdvertMode::Hidden,
		}
	}
}

impl From<StoredSettings> for Aux {
	fn from(stored: StoredSettings) -> Self {
		Self {
			input_device: stored.input_device,
			output_device: stored.output_device,
			noise_backend: stored.noise_backend,
			microphone_gain: stored.microphone_gain,
			display_name: stored.display_name,
			avatar: stored.avatar,
			autostart_daemon: stored.autostart_daemon,
			inbound_policy: stored.inbound_policy,
			advert_mode: stored.advert_mode,
		}
	}
}

/// Peers, devices and auxiliary settings, owned by the control actor.
#[derive(Debug)]
pub struct Directory {
	peers: BTreeMap<PeerId, PeerInfo>,
	linked: BTreeSet<PeerId>,
	devices: BTreeMap<String, DeviceInfo>,
	warnings: Vec<relay_api::pb::Problem>,
	flagged: BTreeSet<PeerId>,
	channels: BTreeMap<Vec<u8>, relay_api::pb::ChannelInfo>,
	shared: BTreeMap<PeerId, Vec<Vec<u8>>>,
	/// Auxiliary settings.
	pub aux: Aux,
}

impl Directory {
	/// Starts from persisted settings and the adapters' initial lists.
	pub fn new(
		aux: Aux,
		peers: Vec<PeerInfo>,
		devices: Vec<DeviceInfo>,
		warnings: Vec<relay_api::pb::Problem>,
	) -> Self {
		Self {
			peers: peers.into_iter().map(|p| (p.id, p)).collect(),
			linked: BTreeSet::new(),
			devices: devices.into_iter().map(|d| (d.id.clone(), d)).collect(),
			warnings,
			flagged: BTreeSet::new(),
			channels: BTreeMap::new(),
			shared: BTreeMap::new(),
			aux,
		}
	}

	/// Start-up notices.
	pub fn warnings(&self) -> Vec<relay_api::pb::Problem> {
		self.warnings.clone()
	}

	/// Every known peer, including linked peers discovery has not reported.
	pub fn peers(&self) -> Vec<relay_api::pb::Peer> {
		let mut ids: BTreeSet<PeerId> = self.peers.keys().copied().collect();
		ids.extend(self.linked.iter().copied());
		ids.into_iter().filter_map(|id| self.peer(id)).collect()
	}

	/// The peer as clients see it: discovery data, link state and the channels it
	/// proved (never merely hinted).
	fn peer_view(&self, info: &PeerInfo, linked: bool) -> relay_api::pb::Peer {
		let mut view = peer_to_pb(info, linked);
		if let Some(shared) = self.shared.get(&info.id) {
			view.shared_channel_ids.clone_from(shared);
		}
		view
	}

	/// Joined channels.
	pub fn channels(&self) -> Vec<relay_api::pb::ChannelInfo> {
		self.channels.values().cloned().collect()
	}

	/// Replaces the channel state; reports changed channels, left channels and
	/// every peer whose proven set moved.
	pub fn set_channels(&mut self, view: ChannelView) -> Vec<StateEvent> {
		let next: BTreeMap<Vec<u8>, relay_api::pb::ChannelInfo> = view
			.channels
			.into_iter()
			.map(|c| (c.channel_id.clone(), c))
			.collect();
		let mut events = Vec::new();
		for (id, info) in &next {
			if self.channels.get(id) != Some(info) {
				events.push(StateEvent {
					event: Some(state_event::Event::Channel(info.clone())),
				});
			}
		}
		for id in self.channels.keys().filter(|id| !next.contains_key(*id)) {
			events.push(removed(id.clone(), "channel"));
		}
		let moved: BTreeSet<PeerId> = self
			.shared
			.keys()
			.chain(view.shared.keys())
			.filter(|p| self.shared.get(p) != view.shared.get(p))
			.copied()
			.collect();
		self.channels = next;
		self.shared = view.shared;
		for peer in moved {
			if let Some(view) = self.peer(peer) {
				events.push(peer_event(view));
			}
		}
		events
	}

	/// Every known device.
	pub fn devices(&self) -> Vec<relay_api::pb::Device> {
		self.devices.values().map(device_to_pb).collect()
	}

	fn peer(&self, id: PeerId) -> Option<relay_api::pb::Peer> {
		let linked = self.linked.contains(&id);
		match self.peers.get(&id) {
			Some(info) => Some(self.peer_view(info, linked)),
			None => linked.then(|| self.peer_view(&PeerInfo::bare(id), true)),
		}
	}

	/// Records a peer that needs the user's attention (waiting for a trust
	/// decision, or presenting a changed key). It stays listed until the
	/// adapter reports it with a settled trust label.
	pub fn flag(&mut self, info: PeerInfo) -> Vec<StateEvent> {
		if self.flagged.len() >= MAX_FLAGGED && !self.flagged.contains(&info.id) {
			return Vec::new();
		}
		self.flagged.insert(info.id);
		let event = peer_event(self.peer_view(&info, self.linked.contains(&info.id)));
		self.peers.insert(info.id, info);
		vec![event]
	}

	/// Adds a runtime warning; repeats are ignored and the list is bounded.
	pub fn warn(&mut self, warning: relay_api::pb::Problem) -> Option<StateEvent> {
		if self
			.warnings
			.iter()
			.any(|w| w.code == warning.code && w.message == warning.message)
		{
			return None;
		}
		if self.warnings.len() >= MAX_WARNINGS {
			self.warnings.remove(0);
		}
		self.warnings.push(warning.clone());
		Some(StateEvent {
			event: Some(state_event::Event::Warning(warning)),
		})
	}

	/// Replaces the discovered peers and reports what changed.
	pub fn set_peers(&mut self, peers: Vec<PeerInfo>) -> Vec<StateEvent> {
		let mut next: BTreeMap<PeerId, PeerInfo> = peers.into_iter().map(|p| (p.id, p)).collect();
		let flagged: Vec<PeerId> = self.flagged.iter().copied().collect();
		for id in flagged {
			match next.get(&id) {
				Some(info) if info.trust != PeerTrust::Unknown => {
					self.flagged.remove(&id);
				}
				Some(_) => {}
				None => {
					if let Some(kept) = self.peers.get(&id) {
						next.insert(id, kept.clone());
					}
				}
			}
		}
		let mut events = Vec::new();
		for (id, info) in &next {
			if self.peers.get(id) != Some(info) {
				events.push(peer_event(self.peer_view(info, self.linked.contains(id))));
			}
		}
		for id in self.peers.keys().filter(|id| !next.contains_key(id)) {
			events.push(removed(id.as_bytes().to_vec(), "peer"));
		}
		self.peers = next;
		events
	}

	/// Replaces the device list and reports what changed.
	pub fn set_devices(&mut self, devices: Vec<DeviceInfo>) -> Vec<StateEvent> {
		let next: BTreeMap<String, DeviceInfo> =
			devices.into_iter().map(|d| (d.id.clone(), d)).collect();
		let mut events = Vec::new();
		for (id, info) in &next {
			if self.devices.get(id) != Some(info) {
				events.push(StateEvent {
					event: Some(state_event::Event::Device(device_to_pb(info))),
				});
			}
		}
		for id in self.devices.keys().filter(|id| !next.contains_key(*id)) {
			events.push(removed(id.as_bytes().to_vec(), "device"));
		}
		self.devices = next;
		events
	}

	/// Records a link change and returns the peer's new view.
	pub fn link_changed(&mut self, peer: PeerId, up: bool) -> Vec<StateEvent> {
		if up {
			self.linked.insert(peer);
		} else {
			self.linked.remove(&peer);
		}
		match self.peer(peer) {
			Some(view) => vec![peer_event(view)],
			None => vec![removed(peer.as_bytes().to_vec(), "peer")],
		}
	}
}

impl PeerInfo {
	/// A peer known only by its identity.
	pub fn bare(id: PeerId) -> Self {
		Self {
			id,
			display_name: String::new(),
			addresses: Vec::new(),
			trust: super::ports::PeerTrust::Unknown,
			reachable: false,
			avatar: None,
		}
	}
}

fn peer_event(peer: relay_api::pb::Peer) -> StateEvent {
	StateEvent {
		event: Some(state_event::Event::Peer(peer)),
	}
}

fn removed(id: Vec<u8>, kind: &str) -> StateEvent {
	StateEvent {
		event: Some(state_event::Event::Removed(Removed {
			id,
			kind: kind.to_owned(),
		})),
	}
}
