//! The preferences schema. Private key material and IPC tokens never live here.

use serde::{Deserialize, Serialize};

use crate::error::{ConfigError, Result};

/// Highest schema version this build writes and understands.
pub const CURRENT_VERSION: u32 = 1;

const MAX_DISPLAY_NAME_CHARS: usize = 64;
const MAX_DEVICE_ID_BYTES: usize = 512;
const MAX_GAIN: f32 = 4.0;

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
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Privacy {
	pub new_calls_receive_only: bool,
	pub background_open_mic_requires_consent: bool,
	pub lock_closes_transmit: bool,
}

impl Default for Privacy {
	fn default() -> Self {
		Self {
			new_calls_receive_only: true,
			background_open_mic_requires_consent: true,
			lock_closes_transmit: true,
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
		for id in [
			&self.audio.preferred_input_id,
			&self.audio.preferred_output_id,
		] {
			if id.len() > MAX_DEVICE_ID_BYTES {
				return bad("device id too long");
			}
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
