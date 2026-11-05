//! Preferences and the local avatar, persisted through `relay-config`.

use std::sync::{Mutex, MutexGuard};

use relay_config::avatar::{AvatarCache, process_avatar};
use relay_config::prefs::{DeviceSelection, NoiseBackend, PreferenceStore, Preferences};

use crate::service::ports::{PortError, SettingsChange, SettingsStore, StoredSettings};

/// [`SettingsStore`] backed by the preference file and the avatar cache.
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

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
	mutex.lock().unwrap_or_else(|e| e.into_inner())
}

impl FileSettings {
	/// Wraps an open store with the preferences it loaded.
	pub fn new(store: PreferenceStore, avatars: AvatarCache, loaded: Preferences) -> Self {
		Self {
			store,
			avatars,
			current: Mutex::new(loaded),
			avatar: Mutex::new(None),
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
			display_name: prefs.profile.display_name.clone(),
			input_device: explicit(audio.input_selection, &audio.preferred_input_id),
			output_device: explicit(audio.output_selection, &audio.preferred_output_id),
			noise_backend: backend_name(audio.noise_backend).to_owned(),
			microphone_gain: audio.microphone_gain,
			playback_gain: audio.playback_gain,
			avatar: *lock(&self.avatar),
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
		*lock(&self.avatar) = Some(avatar.hash);
		Ok(avatar.hash)
	}
}
