//! In-memory adapters that record what the control actor asked of them.

use std::sync::{Arc, Mutex, MutexGuard};

use relay_core::{AudioGeneration, CallId, PeerId, PeerMessage, ReceiveMix, SessionId};

use super::ports::{
	ConnectionDetails, DeviceControl, DeviceInfo, LocalProfile, MediaControl, MediaTelemetry,
	PeerControl, PeerInfo, PortError, Ports, SettingsChange, SettingsStore, StoredSettings,
	TrustRequest,
};

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
	mutex.lock().unwrap_or_else(|e| e.into_inner())
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
	/// `set_noise_suppression`.
	Noise(String),
	/// `start_mic_test`.
	MicTestStart(bool),
	/// `stop_mic_test`.
	MicTestStop,
}

/// Records every media request and accepts them all.
#[derive(Debug, Default)]
pub struct FakeMedia {
	calls: Mutex<Vec<MediaCall>>,
	telemetry: Mutex<MediaTelemetry>,
}

impl FakeMedia {
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
	fn set_noise_suppression(&self, backend: &str) -> Result<(), PortError> {
		self.push(MediaCall::Noise(backend.to_owned()));
		Ok(())
	}
	fn start_mic_test(&self, local_playback: bool) -> Result<(), PortError> {
		self.push(MediaCall::MicTestStart(local_playback));
		Ok(())
	}
	fn telemetry(&self) -> MediaTelemetry {
		lock(&self.telemetry).clone()
	}
	fn stop_mic_test(&self) -> Result<(), PortError> {
		self.push(MediaCall::MicTestStop);
		Ok(())
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
}

impl Default for MemorySettings {
	fn default() -> Self {
		Self {
			stored: Mutex::new(StoredSettings {
				display_name: "Test device".to_owned(),
				microphone_gain: 1.0,
				playback_gain: 1.0,
				..StoredSettings::default()
			}),
			avatars: Mutex::default(),
		}
	}
}

impl SettingsStore for MemorySettings {
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
			SettingsChange::NoiseBackend(id) => stored.noise_backend.clone_from(id),
			SettingsChange::PttReleaseDelay(ms) => stored.ptt_release_delay_ms = *ms,
			SettingsChange::Privacy { inbound, advert } => {
				stored.inbound_policy = *inbound;
				stored.advert_mode = *advert;
			}
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
