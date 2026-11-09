//! What the engine knows besides calls: peers, devices and local settings.

use std::collections::{BTreeMap, BTreeSet};

use relay_api::pb::{Removed, StateEvent, state_event};
use relay_core::PeerId;

use super::convert::{device_to_pb, peer_to_pb};
use super::ports::{
	AdvertMode, DeviceInfo, InboundPolicy, LocalProfile, PeerInfo, PeerTrust, StoredSettings,
};
use super::tones::RingPrefs;
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
	/// Microphone processing: noise suppression and automatic gain.
	pub processing: relay_audio::dsp::SonoraConfig,
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
	/// Ring, ringback and call-sound preferences.
	pub ring: RingPrefs,
	/// File name of the custom ringtone; empty for the built-in chime.
	pub ringtone_name: String,
	/// Finished calls are recorded.
	pub history_enabled: bool,
	/// The mode a call starts in when it connects (`calls.default_transmit_mode`).
	pub default_transmit_mode: relay_core::RouteMode,
	/// `background.available`.
	pub background_available: bool,
	/// `calls.alert`.
	pub call_alert: relay_config::prefs::CallAlert,
	/// `calls.auto_answer_enabled`.
	pub auto_answer_enabled: bool,
	/// Counts changes of the call history, so clients know when to refetch.
	pub history_revision: u64,
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
			processing: stored.processing,
			microphone_gain: stored.microphone_gain,
			display_name: stored.display_name,
			avatar: stored.avatar,
			autostart_daemon: stored.autostart_daemon,
			inbound_policy: stored.inbound_policy,
			advert_mode: stored.advert_mode,
			ring: stored.ring,
			ringtone_name: stored.ringtone_name,
			history_enabled: stored.history_enabled,
			default_transmit_mode: stored.default_transmit_mode,
			background_available: stored.background_available,
			call_alert: stored.call_alert,
			auto_answer_enabled: stored.auto_answer_enabled,
			history_revision: 0,
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
	nearby: Vec<relay_api::pb::NearbyChannel>,
	shared: BTreeMap<PeerId, Vec<Vec<u8>>>,
	/// Wall clock (unix ms) at which each call became connected.
	connected: BTreeMap<relay_core::CallId, u64>,
	/// Wall clock (unix ms) the engine last saw each peer reachable or linked.
	seen: BTreeMap<PeerId, u64>,
	/// Favorites in order (a subset of the trusted peers).
	favorites: Vec<PeerId>,
	/// Contacts flagged for auto-answer.
	auto_answer: BTreeSet<PeerId>,
	/// Calls the engine answered by itself.
	auto_answered: BTreeSet<relay_core::CallId>,
	now_ms: u64,
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
			nearby: Vec::new(),
			shared: BTreeMap::new(),
			connected: BTreeMap::new(),
			seen: BTreeMap::new(),
			favorites: Vec::new(),
			auto_answer: BTreeSet::new(),
			auto_answered: BTreeSet::new(),
			now_ms: 0,
			aux,
		}
	}

	/// Sets the wall clock used to stamp peers seen from now on.
	pub fn set_clock(&mut self, now_ms: u64) {
		self.now_ms = now_ms;
	}

	/// Remembers the wall-clock instant a call first became connected. Later calls to this
	/// for the same call keep the first instant.
	pub fn note_connected(&mut self, call: relay_core::CallId, now_ms: u64) {
		self.connected.entry(call).or_insert(now_ms);
	}

	/// When the call became connected (unix ms), if it did.
	pub fn connected_at(&self, call: relay_core::CallId) -> Option<u64> {
		self.connected.get(&call).copied()
	}

	/// Forgets a call's bookkeeping.
	pub fn forget_call(&mut self, call: relay_core::CallId) {
		self.connected.remove(&call);
		self.auto_answered.remove(&call);
	}

	/// The display name last reported for a peer, if any.
	pub fn peer_name(&self, id: PeerId) -> Option<String> {
		self.peers
			.get(&id)
			.map(|p| p.display_name.clone())
			.filter(|n| !n.trim().is_empty())
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
		view.last_seen_unix_ms = if info.reachable || linked {
			self.now_ms
		} else {
			self.seen.get(&info.id).copied().unwrap_or(0)
		};
		if let Some(shared) = self.shared.get(&info.id) {
			view.shared_channel_ids.clone_from(shared);
		}
		view.auto_answer = self.auto_answer.contains(&info.id);
		if let Some(position) = self.favorites.iter().position(|id| *id == info.id) {
			view.favorite = true;
			view.favorite_position = position as u32;
		}
		view
	}

	/// Joined channels.
	pub fn channels(&self) -> Vec<relay_api::pb::ChannelInfo> {
		self.channels.values().cloned().collect()
	}

	/// Listed channels advertised nearby.
	pub fn nearby(&self) -> Vec<relay_api::pb::NearbyChannel> {
		self.nearby.clone()
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
		if self.nearby != view.nearby {
			events.push(StateEvent {
				event: Some(state_event::Event::NearbyChannels(
					relay_api::pb::NearbyChannels {
						channels: view.nearby.clone(),
					},
				)),
			});
			self.nearby = view.nearby;
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

	/// The devices in use for the saved choice and the current list.
	pub fn device_use(&self) -> super::usage::DeviceUse {
		super::usage::device_use(
			&self.devices,
			&self.aux.input_device,
			&self.aux.output_device,
		)
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

	/// Drops the warning with this code, if any; clients learn it no longer holds.
	pub fn clear_warning(&mut self, code: &str) -> Option<StateEvent> {
		let before = self.warnings.len();
		self.warnings.retain(|w| w.code != code);
		(self.warnings.len() != before).then(|| removed(code.as_bytes().to_vec(), "warning"))
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
		for (id, info) in &next {
			if info.reachable {
				self.seen.insert(*id, self.now_ms);
			}
		}
		self.peers = next;
		events
	}

	/// A peer's stored trust was removed: it leaves the list now (it comes back as an unknown
	/// device if it reaches us again).
	pub fn forget_peer(&mut self, id: PeerId) -> Vec<StateEvent> {
		self.peers.remove(&id);
		self.flagged.remove(&id);
		vec![removed(id.as_bytes().to_vec(), "peer")]
	}

	/// Replaces the favorites and reports the peers whose place changed.
	pub fn set_favorites(&mut self, favorites: Vec<PeerId>) -> Vec<StateEvent> {
		let moved: Vec<PeerId> = self
			.favorites
			.iter()
			.chain(&favorites)
			.filter_map(|id| {
				let before = self.favorites.iter().position(|f| f == id);
				let after = favorites.iter().position(|f| f == id);
				(before != after).then_some(*id)
			})
			.collect::<BTreeSet<_>>()
			.into_iter()
			.collect();
		self.favorites = favorites;
		moved
			.into_iter()
			.filter_map(|id| self.peer(id).map(peer_event))
			.collect()
	}

	/// Replaces the auto-answer flags and reports the peers whose flag changed.
	pub fn set_auto_answer(&mut self, flagged: Vec<PeerId>) -> Vec<StateEvent> {
		let next: BTreeSet<PeerId> = flagged.into_iter().collect();
		let moved: Vec<PeerId> = self
			.auto_answer
			.symmetric_difference(&next)
			.copied()
			.collect();
		self.auto_answer = next;
		moved
			.into_iter()
			.filter_map(|id| self.peer(id).map(peer_event))
			.collect()
	}

	/// True when `peer` is flagged for auto-answer, is stored as verified and has a live
	/// authenticated link: the three facts auto-answer needs about a caller.
	pub fn may_auto_answer(&self, peer: PeerId) -> bool {
		self.auto_answer.contains(&peer)
			&& self.linked.contains(&peer)
			&& self
				.peers
				.get(&peer)
				.is_some_and(|p| p.trust == PeerTrust::Verified)
	}

	/// Notes that the engine answered `call` by itself.
	pub fn note_auto_answered(&mut self, call: relay_core::CallId) {
		self.auto_answered.insert(call);
	}

	/// Whether the engine answered `call` by itself.
	pub fn was_auto_answered(&self, call: relay_core::CallId) -> bool {
		self.auto_answered.contains(&call)
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
			self.seen.insert(peer, self.now_ms);
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
			key_change: None,
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
