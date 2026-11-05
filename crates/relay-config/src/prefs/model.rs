//! The preferences schema. Private key material and IPC tokens never live here.

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, Result};

/// Highest schema version this build writes and understands.
pub const CURRENT_VERSION: u32 = 1;

const MAX_DISPLAY_NAME_CHARS: usize = 64;
const MAX_DEVICE_ID_BYTES: usize = 512;
const MAX_GAIN: f32 = 4.0;

/// Writes an `f32` as its shortest decimal (`0.7`, not `0.699999988079071`).
fn short_f32<S: serde::Serializer>(
	value: &f32,
	serializer: S,
) -> std::result::Result<S::Ok, S::Error> {
	let text = value.to_string();
	serializer.serialize_f64(text.parse().unwrap_or_else(|_| f64::from(*value)))
}

/// How a device is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeviceSelection {
	/// Use exactly `preferred_*_id`; if it is missing, fail closed.
	Explicit,
	/// Follow the operating system default device.
	SystemDefault,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoiseBackend {
	Off,
	Rnnoise,
	Dtln,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Profile {
	pub display_name: String,
}

impl Default for Profile {
	fn default() -> Self {
		Self {
			display_name: "My device".to_owned(),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Audio {
	pub input_selection: DeviceSelection,
	pub preferred_input_id: String,
	pub output_selection: DeviceSelection,
	pub preferred_output_id: String,
	pub noise_backend: NoiseBackend,
	pub microphone_gain: f32,
	pub playback_gain: f32,
	/// Play the ring for an incoming call while a client is attached.
	pub ringtone_enabled: bool,
	/// Ring volume, `0.0..=1.0`.
	#[serde(serialize_with = "short_f32")]
	pub ringtone_volume: f32,
	/// Play the ringback while an outgoing call rings.
	pub ringback_enabled: bool,
	/// Play the short connect and hang-up blips.
	pub call_sounds_enabled: bool,
}

impl Default for Audio {
	fn default() -> Self {
		Self {
			// Fail closed: no microphone until the user picks one.
			input_selection: DeviceSelection::Explicit,
			preferred_input_id: String::new(),
			output_selection: DeviceSelection::SystemDefault,
			preferred_output_id: String::new(),
			noise_backend: NoiseBackend::Off,
			microphone_gain: 1.0,
			playback_gain: 1.0,
			ringtone_enabled: true,
			ringtone_volume: 0.7,
			ringback_enabled: true,
			call_sounds_enabled: true,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Privacy {
	pub new_calls_receive_only: bool,
	pub background_open_mic_requires_consent: bool,
	pub lock_closes_transmit: bool,
	/// Who may ring this device.
	pub inbound_policy: InboundPolicy,
	/// Whether LAN discovery shows this device's name and key hint.
	pub advert_mode: AdvertMode,
}

/// Who may start a call to this device.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InboundPolicy {
	/// Any authenticated, trusted peer.
	#[default]
	Anyone,
	/// Only peers that prove membership of a channel shared with this device.
	SharedChannelOnly,
}

/// How this device appears in LAN discovery.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AdvertMode {
	/// Name and key hint are advertised.
	#[default]
	Visible,
	/// Neither name nor key hint is advertised.
	Hidden,
}

impl Default for Privacy {
	fn default() -> Self {
		Self {
			new_calls_receive_only: true,
			background_open_mic_requires_consent: true,
			lock_closes_transmit: true,
			inbound_policy: InboundPolicy::default(),
			advert_mode: AdvertMode::default(),
		}
	}
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Discovery {
	Mdns,
	Manual,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Network {
	pub discovery: Discovery,
	pub interface_policy: String,
	pub allow_wan: bool,
	/// TCP port for inbound peer links. Fixed by default so a manually entered
	/// `ip:port` stays valid across restarts; `0` picks a free port each run.
	pub listen_port: u16,
	/// Accept 100.64.0.0/10 (Tailscale / CGNAT overlay) addresses the user types or
	/// pastes. Never applies to discovered, advertised or peer-supplied addresses.
	pub allow_overlay_manual: bool,
	/// TCP port of the file-transfer data-link listener. `0` (the default) picks a free
	/// port each run; the port is only ever told to a peer over an authenticated control link.
	/// Applies at daemon start.
	pub transfer_port: u16,
}

/// The default peer-link port.
pub const DEFAULT_LISTEN_PORT: u16 = 47_800;

impl Default for Network {
	fn default() -> Self {
		Self {
			discovery: Discovery::Mdns,
			interface_policy: "eligible_local_interfaces".to_owned(),
			allow_wan: false,
			listen_port: DEFAULT_LISTEN_PORT,
			allow_overlay_manual: false,
			transfer_port: 0,
		}
	}
}

/// Session messaging (`docs/session-messages.md`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Messages {
	/// Master switch. Off: no message, image, offer or transfer is accepted or sent.
	/// Existing transcripts stay readable.
	pub enabled: bool,
	/// Largest file this device will offer or accept, in MiB.
	pub max_offer_size_mib: u32,
	/// Pacing cap of one file transfer in megabytes per second (`0` removes the cap).
	pub transfer_rate_limit_mbps: u32,
}

/// Largest accepted `messages.max_offer_size_mib` (the protocol ceiling, 4 GiB).
pub const MAX_OFFER_SIZE_MIB: u32 = 4096;
/// Largest accepted `messages.transfer_rate_limit_mbps`.
pub const MAX_TRANSFER_RATE_MBPS: u32 = 1024;

impl Default for Messages {
	fn default() -> Self {
		Self {
			enabled: true,
			max_offer_size_mib: 2048,
			transfer_rate_limit_mbps: 16,
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Diagnostics {
	pub log_level: String,
	/// Explicit opt-in for raw-audio test capture. Always off by default.
	pub record_raw_audio: bool,
	pub export_prometheus: bool,
}

impl Default for Diagnostics {
	fn default() -> Self {
		Self {
			log_level: "info".to_owned(),
			record_raw_audio: false,
			export_prometheus: false,
		}
	}
}

/// Identity key storage choice. The file fallback is used only if the user opts in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyStorage {
	/// OS credential store (Keychain, Credential Manager, Secret Service).
	#[default]
	OsCredentialStore,
	/// Unencrypted 0600 file. Weaker than the OS store; explicit user choice only.
	InsecureFile,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Identity {
	pub key_storage: KeyStorage,
}

/// Global hotkey bindings. An empty accelerator disables the binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Hotkeys {
	/// Push-to-talk accelerator such as `Ctrl+Alt+Space`.
	pub push_to_talk: String,
}

/// Longest accepted accelerator text.
const MAX_ACCELERATOR_CHARS: usize = 64;

impl Default for Hotkeys {
	fn default() -> Self {
		Self {
			push_to_talk: "Ctrl+Alt+Space".to_owned(),
		}
	}
}

/// Push-to-talk behaviour.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Ptt {
	/// How long the transmit gate stays open after an ordinary key release, so the
	/// end of a sentence is not clipped. `0` closes at once.
	pub release_delay_ms: u32,
}

/// Longest accepted release delay.
pub const MAX_RELEASE_DELAY_MS: u32 = 2000;

impl Default for Ptt {
	fn default() -> Self {
		Self {
			release_delay_ms: 200,
		}
	}
}

/// Behaviour of the programs that talk to the daemon.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Clients {
	/// The desktop app starts the daemon when none runs and restarts it after
	/// a crash. (`RELAY_NO_AUTOSTART=1` overrides this for every client.)
	pub autostart_daemon: bool,
}

impl Default for Clients {
	fn default() -> Self {
		Self {
			autostart_daemon: true,
		}
	}
}

/// All user preferences.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
	pub config_version: u32,
	pub profile: Profile,
	pub audio: Audio,
	pub privacy: Privacy,
	pub network: Network,
	pub diagnostics: Diagnostics,
	pub identity: Identity,
	pub hotkeys: Hotkeys,
	pub ptt: Ptt,
	pub clients: Clients,
	pub messages: Messages,
}

impl Default for Preferences {
	fn default() -> Self {
		Self {
			config_version: CURRENT_VERSION,
			profile: Profile::default(),
			audio: Audio::default(),
			privacy: Privacy::default(),
			network: Network::default(),
			diagnostics: Diagnostics::default(),
			identity: Identity::default(),
			hotkeys: Hotkeys::default(),
			ptt: Ptt::default(),
			clients: Clients::default(),
			messages: Messages::default(),
		}
	}
}

impl Preferences {
	/// Checks value ranges; called before every save and after every load.
	pub fn validate(&self) -> Result<()> {
		let bad = |m: &str| Err(ConfigError::InvalidPreference(m.to_owned()));
		let name = &self.profile.display_name;
		if name.trim().is_empty() || name.chars().count() > MAX_DISPLAY_NAME_CHARS {
			return bad("display_name must be 1-64 characters");
		}
		if name.chars().any(char::is_control) {
			return bad("display_name must not contain control characters");
		}
		for gain in [self.audio.microphone_gain, self.audio.playback_gain] {
			if !gain.is_finite() || !(0.0..=MAX_GAIN).contains(&gain) {
				return bad("gain must be finite and within 0.0..=4.0");
			}
		}
		let volume = self.audio.ringtone_volume;
		if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
			return bad("audio.ringtone_volume must be finite and within 0.0..=1.0");
		}
		for id in [
			&self.audio.preferred_input_id,
			&self.audio.preferred_output_id,
		] {
			if id.len() > MAX_DEVICE_ID_BYTES {
				return bad("device id too long");
			}
		}
		let hotkey = &self.hotkeys.push_to_talk;
		if hotkey.chars().count() > MAX_ACCELERATOR_CHARS || hotkey.chars().any(char::is_control) {
			return bad("push_to_talk hotkey must be at most 64 printable characters");
		}
		if self.ptt.release_delay_ms > MAX_RELEASE_DELAY_MS {
			return bad("ptt.release_delay_ms must be within 0..=2000");
		}
		if !(1..=MAX_OFFER_SIZE_MIB).contains(&self.messages.max_offer_size_mib) {
			return bad("messages.max_offer_size_mib must be within 1..=4096");
		}
		if self.messages.transfer_rate_limit_mbps > MAX_TRANSFER_RATE_MBPS {
			return bad("messages.transfer_rate_limit_mbps must be within 0..=1024");
		}
		Ok(())
	}

	/// Drops any microphone choice and forces receive-only starts.
	pub(crate) fn clear_microphone(&mut self) {
		self.audio.input_selection = DeviceSelection::Explicit;
		self.audio.preferred_input_id.clear();
		self.privacy.new_calls_receive_only = true;
	}
}
