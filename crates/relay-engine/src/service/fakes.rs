//! In-memory adapters that record what the control actor asked of them.

use relay_io::LockExt;
use std::sync::{Arc, Mutex, MutexGuard};

use relay_core::{AudioGeneration, CallId, PeerId, PeerMessage, ReceiveMix, SessionId};

use super::ports::{AdvertMode, InboundPolicy};
use super::ports::{
	ConnectionDetails, DeviceControl, DeviceInfo, ForgottenPeer, LocalProfile, MediaControl,
	MediaTelemetry, PeerControl, PeerInfo, PeerSelector, PortError, Ports, SettingsChange,
	SettingsStore, StoredSettings, TrustRequest,
};
use super::tones::ToneCommand;

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
	mutex.lock_unpoisoned()
}

/// One request seen by [`FakeMedia`].
#[derive(Debug, Clone, PartialEq)]
pub enum MediaCall {
	/// `open_capture`.
	OpenCapture(AudioGeneration),
	/// `close_capture`.
	CloseCapture(AudioGeneration),
	/// `prepare_media`.
	Prepare(CallId, SessionId),
	/// `stop_media`.
	Stop(CallId, SessionId),
	/// `set_receive_mix`.
	Mix(ReceiveMix),
	/// `set_audio_processing`.
	Processing(relay_audio::dsp::SonoraConfig),
	/// `start_mic_test`.
	MicTestStart(bool),
	/// `stop_mic_test`.
	MicTestStop,
	/// `tone`.
	Tone(ToneCommand),
	/// `set_ringtone`: the length in samples of the new ring, `None` for the built-in one.
	Ringtone(Option<usize>),
}

/// Records every media request and accepts them all.
#[derive(Debug, Default)]
pub struct FakeMedia {
	calls: Mutex<Vec<MediaCall>>,
	telemetry: Mutex<MediaTelemetry>,
	stats: Mutex<Option<super::call_stats::CallRaw>>,
}

impl FakeMedia {
	/// Sets what `call_stats()` reports for every call.
	pub fn set_call_stats(&self, raw: Option<super::call_stats::CallRaw>) {
		*lock(&self.stats) = raw;
	}

	/// Sets what `telemetry()` reports.
	pub fn set_telemetry(&self, telemetry: MediaTelemetry) {
		*lock(&self.telemetry) = telemetry;
	}

	/// Everything requested so far.
	pub fn calls(&self) -> Vec<MediaCall> {
		lock(&self.calls).clone()
	}

	fn push(&self, call: MediaCall) {
		lock(&self.calls).push(call);
	}
}

impl MediaControl for FakeMedia {
	fn open_capture(&self, generation: AudioGeneration) -> Result<(), PortError> {
		self.push(MediaCall::OpenCapture(generation));
		Ok(())
	}
	fn close_capture(&self, generation: AudioGeneration) {
		self.push(MediaCall::CloseCapture(generation));
	}
	fn prepare_media(&self, call: CallId, session: SessionId) -> Result<(), PortError> {
		self.push(MediaCall::Prepare(call, session));
		Ok(())
	}
	fn stop_media(&self, call: CallId, session: SessionId) {
		self.push(MediaCall::Stop(call, session));
	}
	fn set_receive_mix(&self, mix: ReceiveMix) {
		self.push(MediaCall::Mix(mix));
	}
	fn set_audio_processing(
		&self,
		config: relay_audio::dsp::SonoraConfig,
	) -> Result<(), PortError> {
		self.push(MediaCall::Processing(config));
		Ok(())
	}
	fn start_mic_test(&self, local_playback: bool) -> Result<(), PortError> {
		self.push(MediaCall::MicTestStart(local_playback));
		Ok(())
	}
	fn telemetry(&self) -> MediaTelemetry {
		lock(&self.telemetry).clone()
	}
	fn call_stats(&self, _call: relay_core::CallId) -> Option<super::call_stats::CallRaw> {
		lock(&self.stats).clone()
	}
	fn stop_mic_test(&self) -> Result<(), PortError> {
		self.push(MediaCall::MicTestStop);
		Ok(())
	}
	fn tone(&self, command: ToneCommand) {
		self.push(MediaCall::Tone(command));
	}
	fn set_ringtone(&self, ring: Option<std::sync::Arc<[f32]>>) {
		self.push(MediaCall::Ringtone(ring.map(|r| r.len())));
	}
}

/// Records messages and resolves manual addresses from a table.
#[derive(Debug, Default)]
pub struct FakePeers {
	sent: Mutex<Vec<(PeerId, PeerMessage)>>,
	links: Mutex<Vec<PeerId>>,
	trust: Mutex<Vec<TrustRequest>>,
	profiles: Mutex<Vec<LocalProfile>>,
	known: Mutex<Vec<PeerInfo>>,
	addresses: Mutex<Vec<(String, PeerId)>>,
	connection: Mutex<Option<ConnectionDetails>>,
	forgotten: Mutex<Vec<PeerSelector>>,
}

impl FakePeers {
	/// Makes `address` resolve to `peer`.
	pub fn map_address(&self, address: &str, peer: PeerId) {
		lock(&self.addresses).push((address.to_owned(), peer));
	}

	/// Sets what `connection_info()` reports.
	pub fn set_connection(&self, details: ConnectionDetails) {
		*lock(&self.connection) = Some(details);
	}

	/// Sets the peers reported by `peers()`.
	pub fn set_known(&self, peers: Vec<PeerInfo>) {
		*lock(&self.known) = peers;
	}

	/// Messages sent so far.
	pub fn sent(&self) -> Vec<(PeerId, PeerMessage)> {
		lock(&self.sent).clone()
	}

	/// Peers a link was requested for.
	pub fn links(&self) -> Vec<PeerId> {
		lock(&self.links).clone()
	}

	/// Trust decisions received.
	pub fn trust_requests(&self) -> Vec<TrustRequest> {
		lock(&self.trust).clone()
	}

	/// Selectors `forget_peer` received.
	pub fn forgotten(&self) -> Vec<PeerSelector> {
		lock(&self.forgotten).clone()
	}

	/// Advertised profiles pushed so far.
	pub fn profiles(&self) -> Vec<LocalProfile> {
		lock(&self.profiles).clone()
	}
}

impl PeerControl for FakePeers {
	fn ensure_link(&self, peer: PeerId) -> Result<(), PortError> {
		lock(&self.links).push(peer);
		Ok(())
	}
	fn send(&self, peer: PeerId, message: PeerMessage) -> Result<(), PortError> {
		lock(&self.sent).push((peer, message));
		Ok(())
	}
	fn resolve_address(&self, address: &str) -> Result<PeerId, PortError> {
		lock(&self.addresses)
			.iter()
			.find(|(known, _)| known == address)
			.map(|(_, peer)| *peer)
			.ok_or_else(|| PortError::NotFound(address.to_owned()))
	}
	fn peers(&self) -> Vec<PeerInfo> {
		lock(&self.known).clone()
	}
	fn decide_trust(&self, request: TrustRequest) -> Result<(), PortError> {
		lock(&self.trust).push(request);
		Ok(())
	}
	fn forget_peer(&self, selector: &PeerSelector) -> Result<ForgottenPeer, PortError> {
		let mut known = lock(&self.known);
		let at = known
			.iter()
			.position(|p| match selector {
				PeerSelector::Id(id) => p.id == *id,
				PeerSelector::Query(name) => p.display_name.eq_ignore_ascii_case(name),
			})
			.ok_or_else(|| PortError::NotFound("no such peer".to_owned()))?;
		let gone = known.remove(at);
		lock(&self.forgotten).push(selector.clone());
		Ok(ForgottenPeer {
			id: gone.id,
			name: gone.display_name,
			key_changed: Vec::new(),
		})
	}
	fn profile_changed(&self, profile: &LocalProfile) {
		lock(&self.profiles).push(profile.clone());
	}
	fn connection_info(&self) -> Result<ConnectionDetails, PortError> {
		lock(&self.connection)
			.clone()
			.ok_or(PortError::Unsupported("connection info"))
	}
}

/// A fixed device list that accepts selections of known devices.
#[derive(Debug, Default)]
pub struct FakeDevices {
	devices: Mutex<Vec<DeviceInfo>>,
}

impl FakeDevices {
	/// Replaces the device list.
	pub fn set(&self, devices: Vec<DeviceInfo>) {
		*lock(&self.devices) = devices;
	}
}

impl DeviceControl for FakeDevices {
	fn devices(&self) -> Vec<DeviceInfo> {
		lock(&self.devices).clone()
	}
	fn select(&self, input: &str, output: &str) -> Result<(), PortError> {
		let devices = lock(&self.devices);
		for id in [input, output].into_iter().filter(|id| !id.is_empty()) {
			if !devices.iter().any(|d| d.id == id && d.available) {
				return Err(PortError::NotFound(id.to_owned()));
			}
		}
		Ok(())
	}
}

/// Settings kept in memory.
#[derive(Debug)]
pub struct MemorySettings {
	stored: Mutex<StoredSettings>,
	avatars: Mutex<Vec<([u8; 32], Vec<u8>)>>,
	prefs: Mutex<relay_config::prefs::Preferences>,
	history: Mutex<Vec<relay_config::history::CallRecord>>,
	favorites: Mutex<Vec<PeerId>>,
	auto_answer: Mutex<Vec<PeerId>>,
	ringtone: Mutex<Option<(String, std::sync::Arc<[f32]>)>>,
}

/// Copies one saved preference into the typed view the engine reads live.
fn mirror(stored: &mut StoredSettings, prefs: &relay_config::prefs::Preferences, key: &str) {
	use relay_config::prefs as p;
	let (audio, ring) = (&prefs.audio, &mut stored.ring);
	match key {
		"profile.display_name" => stored.display_name.clone_from(&prefs.profile.display_name),
		"audio.microphone_gain" => stored.microphone_gain = audio.microphone_gain,
		"audio.playback_gain" => stored.playback_gain = audio.playback_gain,
		"audio.noise_suppression" | "audio.noise_suppression_level" | "audio.auto_gain" => {
			stored.processing = crate::daemon::processing_of(audio);
		}
		"ptt.release_delay_ms" => stored.ptt_release_delay_ms = prefs.ptt.release_delay_ms,
		"clients.autostart_daemon" => stored.autostart_daemon = prefs.clients.autostart_daemon,
		"audio.ringtone_enabled" => ring.ringtone_enabled = audio.ringtone_enabled,
		"audio.ringtone_volume" => ring.ringtone_volume = audio.ringtone_volume,
		"audio.ringback_enabled" => ring.ringback_enabled = audio.ringback_enabled,
		"audio.call_sounds_enabled" => ring.call_sounds_enabled = audio.call_sounds_enabled,
		"history.enabled" => stored.history_enabled = prefs.history.enabled,
		"background.available" => stored.background_available = prefs.background.available,
		"calls.alert" => stored.call_alert = prefs.calls.alert,
		"calls.auto_answer_enabled" => stored.auto_answer_enabled = prefs.calls.auto_answer_enabled,
		"calls.default_transmit_mode" => {
			stored.default_transmit_mode = match prefs.calls.default_transmit_mode {
				p::TransmitModePref::Muted => relay_core::RouteMode::Muted,
				p::TransmitModePref::Unmuted => relay_core::RouteMode::Unmuted,
				p::TransmitModePref::PushToTalk => relay_core::RouteMode::PushToTalk,
			}
		}
		"privacy.inbound_policy" => {
			stored.inbound_policy = match prefs.privacy.inbound_policy {
				p::InboundPolicy::Anyone => InboundPolicy::Anyone,
				p::InboundPolicy::SharedChannelOnly => InboundPolicy::SharedChannelOnly,
			}
		}
		"privacy.advert_mode" => {
			stored.advert_mode = match prefs.privacy.advert_mode {
				p::AdvertMode::Visible => AdvertMode::Visible,
				p::AdvertMode::Hidden => AdvertMode::Hidden,
			}
		}
		_ => {}
	}
}

impl Default for MemorySettings {
	fn default() -> Self {
		Self {
			stored: Mutex::new(StoredSettings {
				display_name: "Test device".to_owned(),
				autostart_daemon: true,
				microphone_gain: 1.0,
				playback_gain: 1.0,
				history_enabled: true,
				background_available: true,
				auto_answer_enabled: true,
				default_transmit_mode: relay_core::RouteMode::Muted,
				..StoredSettings::default()
			}),
			avatars: Mutex::default(),
			history: Mutex::default(),
			favorites: Mutex::default(),
			auto_answer: Mutex::default(),
			ringtone: Mutex::default(),
			prefs: Mutex::new(relay_config::prefs::Preferences {
				profile: relay_config::prefs::Profile {
					display_name: "Test device".to_owned(),
				},
				..Default::default()
			}),
		}
	}
}

impl SettingsStore for MemorySettings {
	fn favorites(&self) -> Vec<PeerId> {
		lock(&self.favorites).clone()
	}
	/// Keeps the flag as given; the verified-only rule is the daemon's table to judge and the
	/// actor checks the live trust label again when a call arrives.
	fn auto_answer(&self) -> Vec<PeerId> {
		lock(&self.auto_answer).clone()
	}
	fn set_auto_answer(&self, peer: PeerId, on: bool) -> Result<(), PortError> {
		let mut flags = lock(&self.auto_answer);
		flags.retain(|p| *p != peer);
		if on {
			flags.push(peer);
		}
		Ok(())
	}
	/// Keeps the list as given; whether a peer is trusted is the daemon's table to judge.
	fn set_favorites(&self, ordered: &[PeerId]) -> Result<(), PortError> {
		use relay_config::favorites::{DUPLICATE, FULL, MAX_FAVORITES};
		if ordered.len() > MAX_FAVORITES {
			return Err(PortError::Invalid(FULL.to_owned()));
		}
		if ordered
			.iter()
			.enumerate()
			.any(|(i, p)| ordered[..i].contains(p))
		{
			return Err(PortError::Invalid(DUPLICATE.to_owned()));
		}
		*lock(&self.favorites) = ordered.to_vec();
		Ok(())
	}
	fn load(&self) -> StoredSettings {
		lock(&self.stored).clone()
	}
	fn save(&self, change: &SettingsChange) -> Result<(), PortError> {
		let mut stored = lock(&self.stored);
		match change {
			SettingsChange::DisplayName(name) => stored.display_name.clone_from(name),
			SettingsChange::Devices { input, output } => {
				stored.input_device.clone_from(input);
				stored.output_device.clone_from(output);
			}
			SettingsChange::PttReleaseDelay(ms) => stored.ptt_release_delay_ms = *ms,
			SettingsChange::AutostartDaemon(on) => stored.autostart_daemon = *on,
			SettingsChange::Privacy { inbound, advert } => {
				stored.inbound_policy = *inbound;
				stored.advert_mode = *advert;
			}
			SettingsChange::Ring(ring) => stored.ring = *ring,
		}
		Ok(())
	}
	fn store_avatar(&self, encoded: &[u8]) -> Result<[u8; 32], PortError> {
		if encoded.is_empty() {
			return Err(PortError::Invalid("empty image".to_owned()));
		}
		let hash = *blake3::hash(encoded).as_bytes();
		lock(&self.stored).avatar = Some(hash);
		lock(&self.avatars).push((hash, encoded.to_vec()));
		Ok(hash)
	}
	fn store_ringtone(&self, name: &str, data: &[u8]) -> Result<std::sync::Arc<[f32]>, PortError> {
		use relay_audio::ringtone::{RingtoneLimits, process};
		let limits = lock(&self.prefs).limits;
		let samples: std::sync::Arc<[f32]> = process(
			data,
			RingtoneLimits {
				max_bytes: limits.ringtone_max_bytes(),
				max_seconds: limits.ringtone_max_seconds,
			},
		)
		.map_err(|e| PortError::Rejected {
			code: e.code(),
			message: e.to_string(),
		})?
		.into();
		lock(&self.stored).ringtone_name = name.to_owned();
		*lock(&self.ringtone) = Some((name.to_owned(), samples.clone()));
		Ok(samples)
	}
	fn clear_ringtone(&self) -> Result<(), PortError> {
		lock(&self.stored).ringtone_name.clear();
		*lock(&self.ringtone) = None;
		Ok(())
	}
	fn ringtone(&self) -> Option<(String, std::sync::Arc<[f32]>)> {
		lock(&self.ringtone).clone()
	}
	fn apply(&self, entries: &[(String, String)]) -> Result<Vec<Option<String>>, PortError> {
		let mut prefs = lock(&self.prefs);
		let (next, answers) = relay_config::prefs::flat::set_many(&prefs, entries);
		*prefs = next;
		for ((key, _), answer) in entries.iter().zip(&answers) {
			if answer.is_none() {
				mirror(&mut lock(&self.stored), &prefs, key);
			}
		}
		Ok(answers)
	}
	fn all(&self) -> Result<std::collections::BTreeMap<String, String>, PortError> {
		relay_config::prefs::flat::all(&lock(&self.prefs))
			.map_err(|e| PortError::Unavailable(e.to_string()))
	}
	fn record_call(
		&self,
		entry: &relay_config::history::NewCallRecord,
		_now_ms: u64,
	) -> Result<(), PortError> {
		use relay_config::history::CallRecord;
		let mut rows = lock(&self.history);
		let id = rows.first().map_or(1, |r| r.id + 1);
		rows.insert(
			0,
			CallRecord {
				id,
				peer_name: entry.peer_name.clone(),
				peer_fingerprint: entry.peer_fingerprint.clone(),
				peer_fingerprint_short: entry.peer_fingerprint_short.clone(),
				direction: entry.direction,
				outcome: entry.outcome,
				started_at_ms: entry.started_at_ms,
				duration_ms: entry.duration_ms,
				call_id: entry.call_id,
			},
		);
		Ok(())
	}
	fn call_history(
		&self,
		limit: usize,
		before: Option<u64>,
	) -> Result<Vec<relay_config::history::CallRecord>, PortError> {
		Ok(lock(&self.history)
			.iter()
			.filter(|r| before.is_none_or(|b| r.id < b))
			.take(limit)
			.cloned()
			.collect())
	}
	fn clear_call_history(&self) -> Result<(), PortError> {
		lock(&self.history).clear();
		Ok(())
	}
	fn clear_avatar(&self) -> Result<(), PortError> {
		lock(&self.stored).avatar = None;
		Ok(())
	}
	fn avatar(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>, PortError> {
		Ok(lock(&self.avatars)
			.iter()
			.find(|(known, _)| known == hash)
			.map(|(_, bytes)| bytes.clone()))
	}
}

/// A complete fake adapter set that keeps typed access to each fake.
#[derive(Debug, Clone, Default)]
pub struct FakePorts {
	/// Media fake.
	pub media: Arc<FakeMedia>,
	/// Peer fake.
	pub peers: Arc<FakePeers>,
	/// Device fake.
	pub devices: Arc<FakeDevices>,
	/// Settings fake.
	pub settings: Arc<MemorySettings>,
}

impl FakePorts {
	/// The trait-object view the control actor takes.
	pub fn ports(&self) -> Ports {
		Ports {
			media: self.media.clone(),
			peers: self.peers.clone(),
			devices: self.devices.clone(),
			settings: self.settings.clone(),
		}
	}
}
