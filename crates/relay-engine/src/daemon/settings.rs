//! Preferences and the local avatar, persisted through `relay-config`.

use std::sync::{Mutex, MutexGuard};

use relay_config::avatar::{AvatarCache, process_avatar};
use relay_config::prefs::{
	self, DeviceSelection, NoiseBackend, PreferenceStore, Preferences, Profile,
};
use relay_net::discovery::sanitize_display_name;

use crate::service::RingPrefs;
use crate::service::ports::{
	AdvertMode, InboundPolicy, PortError, SettingsChange, SettingsStore, StoredSettings,
};

/// [`SettingsStore`] backed by the per-user SQLite preferences and avatar cache.
#[derive(Debug)]
pub struct FileSettings {
	store: PreferenceStore,
	avatars: AvatarCache,
	current: Mutex<Preferences>,
	avatar: Mutex<Option<[u8; 32]>>,
}

fn backend_name(backend: NoiseBackend) -> &'static str {
	match backend {
		NoiseBackend::Off => "off",
		NoiseBackend::Rnnoise => "rnnoise",
		NoiseBackend::Dtln => "dtln",
	}
}

fn parse_backend(name: &str) -> Result<NoiseBackend, PortError> {
	match name {
		"off" => Ok(NoiseBackend::Off),
		"rnnoise" => Ok(NoiseBackend::Rnnoise),
		"dtln" => Ok(NoiseBackend::Dtln),
		other => Err(PortError::Invalid(format!(
			"unknown noise backend `{other}`"
		))),
	}
}

/// The name shown when the user never chose one: the computer's hostname.
fn effective_name(stored: &str) -> String {
	let unset = stored.trim().is_empty() || stored == Profile::default().display_name;
	if !unset {
		return sanitize_display_name(stored);
	}
	let host = gethostname::gethostname().to_string_lossy().into_owned();
	sanitize_display_name(&host)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
	mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl FileSettings {
	/// Wraps an open store with the preferences it loaded. The local avatar hash
	/// persisted by an earlier run is restored if its bytes are still cached.
	pub fn new(store: PreferenceStore, avatars: AvatarCache, loaded: Preferences) -> Self {
		let avatar = avatars.local().ok().flatten();
		Self {
			store,
			avatars,
			current: Mutex::new(loaded),
			avatar: Mutex::new(avatar),
		}
	}
}

impl SettingsStore for FileSettings {
	fn load(&self) -> StoredSettings {
		let prefs = lock(&self.current);
		let audio = &prefs.audio;
		let explicit = |selection, id: &str| match selection {
			DeviceSelection::Explicit => id.to_owned(),
			DeviceSelection::SystemDefault => String::new(),
		};
		StoredSettings {
			display_name: effective_name(&prefs.profile.display_name),
			input_device: explicit(audio.input_selection, &audio.preferred_input_id),
			output_device: explicit(audio.output_selection, &audio.preferred_output_id),
			noise_backend: backend_name(audio.noise_backend).to_owned(),
			microphone_gain: audio.microphone_gain,
			playback_gain: audio.playback_gain,
			ptt_release_delay_ms: prefs.ptt.release_delay_ms,
			autostart_daemon: prefs.clients.autostart_daemon,
			avatar: *lock(&self.avatar),
			inbound_policy: match prefs.privacy.inbound_policy {
				prefs::InboundPolicy::Anyone => InboundPolicy::Anyone,
				prefs::InboundPolicy::SharedChannelOnly => InboundPolicy::SharedChannelOnly,
			},
			advert_mode: match prefs.privacy.advert_mode {
				prefs::AdvertMode::Visible => AdvertMode::Visible,
				prefs::AdvertMode::Hidden => AdvertMode::Hidden,
			},
			ring: RingPrefs {
				ringtone_enabled: audio.ringtone_enabled,
				ringtone_volume: audio.ringtone_volume,
				ringback_enabled: audio.ringback_enabled,
				call_sounds_enabled: audio.call_sounds_enabled,
			},
		}
	}

	fn save(&self, change: &SettingsChange) -> Result<(), PortError> {
		let mut next = lock(&self.current).clone();
		match change {
			SettingsChange::DisplayName(name) => next.profile.display_name.clone_from(name),
			SettingsChange::Devices { input, output } => {
				next.audio.input_selection = DeviceSelection::Explicit;
				next.audio.preferred_input_id.clone_from(input);
				if output.is_empty() {
					next.audio.output_selection = DeviceSelection::SystemDefault;
					next.audio.preferred_output_id.clear();
				} else {
					next.audio.output_selection = DeviceSelection::Explicit;
					next.audio.preferred_output_id.clone_from(output);
				}
			}
			SettingsChange::NoiseBackend(name) => next.audio.noise_backend = parse_backend(name)?,
			SettingsChange::PttReleaseDelay(ms) => next.ptt.release_delay_ms = *ms,
			SettingsChange::AutostartDaemon(on) => next.clients.autostart_daemon = *on,
			SettingsChange::Privacy { inbound, advert } => {
				next.privacy.inbound_policy = match inbound {
					InboundPolicy::Anyone => prefs::InboundPolicy::Anyone,
					InboundPolicy::SharedChannelOnly => prefs::InboundPolicy::SharedChannelOnly,
				};
				next.privacy.advert_mode = match advert {
					AdvertMode::Visible => prefs::AdvertMode::Visible,
					AdvertMode::Hidden => prefs::AdvertMode::Hidden,
				};
			}
			SettingsChange::Ring(ring) => {
				next.audio.ringtone_enabled = ring.ringtone_enabled;
				next.audio.ringtone_volume = ring.ringtone_volume;
				next.audio.ringback_enabled = ring.ringback_enabled;
				next.audio.call_sounds_enabled = ring.call_sounds_enabled;
			}
		}
		self.store
			.save(&next)
			.map_err(|e| PortError::Unavailable(e.to_string()))?;
		*lock(&self.current) = next;
		Ok(())
	}

	fn store_avatar(&self, encoded: &[u8]) -> Result<[u8; 32], PortError> {
		let avatar = process_avatar(encoded).map_err(|e| PortError::Invalid(e.to_string()))?;
		self.avatars
			.put(&avatar)
			.map_err(|e| PortError::Unavailable(e.to_string()))?;
		self.avatars
			.set_local(&avatar.hash)
			.map_err(|e| PortError::Unavailable(e.to_string()))?;
		*lock(&self.avatar) = Some(avatar.hash);
		Ok(avatar.hash)
	}

	fn avatar(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>, PortError> {
		self.avatars
			.get(hash)
			.map_err(|e| PortError::Unavailable(e.to_string()))
	}
}
