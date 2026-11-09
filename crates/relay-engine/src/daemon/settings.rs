//! Preferences and the local avatar, persisted through `relay-config`.

use relay_io::LockExt;
use std::sync::{Arc, Mutex, MutexGuard};

use relay_audio::dsp::{self as dsp, SonoraConfig};
use relay_audio::ringtone::RingtoneLimits;

use relay_config::avatar::{AvatarCache, process_avatar};
use relay_config::history::{CallHistory, CallRecord, NewCallRecord};
use relay_config::prefs::{
	self, DeviceSelection, NoiseLevel, PreferenceStore, Preferences, Profile,
};
use relay_net::discovery::sanitize_display_name;

use super::favorites::FavoriteBook;
use super::ringtone::RingtoneStore;
use crate::service::RingPrefs;
use crate::service::ports::{
	AdvertMode, InboundPolicy, PortError, SettingsChange, SettingsStore, StoredSettings,
};

/// [`SettingsStore`] backed by the per-user SQLite preferences and avatar cache.
#[derive(Debug)]
pub struct FileSettings {
	store: PreferenceStore,
	avatars: AvatarCache,
	history: CallHistory,
	favorites: FavoriteBook,
	ringtone: RingtoneStore,
	/// File name of the stored ringtone; empty for the built-in chime.
	ringtone_name: Mutex<String>,
	current: Mutex<Preferences>,
	avatar: Mutex<Option<[u8; 32]>>,
	/// Shown while the user has not chosen a name; see [`effective_name`].
	default_name: Option<String>,
}

/// The microphone processing the saved preferences ask for.
pub fn processing_of(audio: &prefs::Audio) -> SonoraConfig {
	SonoraConfig {
		noise_suppression: audio.noise_suppression,
		noise_level: match audio.noise_suppression_level {
			NoiseLevel::Low => dsp::NoiseLevel::Low,
			NoiseLevel::Moderate => dsp::NoiseLevel::Moderate,
			NoiseLevel::High => dsp::NoiseLevel::High,
		},
		auto_gain: audio.auto_gain,
		// Speaker mode (WP-12) turns this on once a far-end reference is fed.
		echo_cancel: false,
	}
}

/// The name shown: the one the user chose, else the platform's default name
/// (the device name on a phone), else the computer's hostname. A default that
/// is blank after sanitising is ignored.
fn effective_name(stored: &str, platform_default: Option<&str>) -> String {
	let unset = stored.trim().is_empty() || stored == Profile::default().display_name;
	if !unset {
		return sanitize_display_name(stored);
	}
	// A name with nothing left after sanitising comes back as the placeholder.
	let placeholder = sanitize_display_name("");
	if let Some(name) = platform_default.map(sanitize_display_name)
		&& name != placeholder
	{
		return name;
	}
	let host = gethostname::gethostname().to_string_lossy().into_owned();
	sanitize_display_name(&host)
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
	mutex.lock_unpoisoned()
}

impl FileSettings {
	/// Wraps an open store with the preferences it loaded. `default_name` is shown
	/// until the user chooses a name. The local avatar hash
	/// persisted by an earlier run is restored if its bytes are still cached.
	pub fn new(
		store: PreferenceStore,
		avatars: AvatarCache,
		history: CallHistory,
		favorites: FavoriteBook,
		ringtone: RingtoneStore,
		loaded: Preferences,
		default_name: Option<String>,
	) -> Self {
		let avatar = avatars.local().ok().flatten();
		Self {
			store,
			avatars,
			history,
			favorites,
			ringtone_name: Mutex::new(ringtone.name()),
			ringtone,
			current: Mutex::new(loaded),
			avatar: Mutex::new(avatar),
			default_name,
		}
	}
}

impl FileSettings {
	/// Puts changed `limits.*` into force: the avatar cache and the call history shrink at
	/// once. (The messaging budgets are applied by the messaging actor.) The new bounds are
	/// stored before anything is deleted, so a failed cleanup still limits every later write.
	fn enforce_limits(&self, before: prefs::Limits, now: prefs::Limits) {
		if before.avatar_cache_mib != now.avatar_cache_mib
			&& let Err(error) = self.avatars.set_limit(now.avatar_cache_bytes())
		{
			tracing::warn!(%error, "the avatar cache could not be trimmed to its new limit");
		}
		if (before.history_max_entries, before.history_max_age_days)
			!= (now.history_max_entries, now.history_max_age_days)
		{
			let limits = relay_config::history::Limits {
				max_entries: now.history_max_entries as usize,
				max_age_ms: now.history_max_age_ms(),
			};
			if let Err(error) = self
				.history
				.set_limits(limits, relay_io::clock::unix_millis())
			{
				tracing::warn!(%error, "the call history could not be trimmed to its new limit");
			}
		}
	}
}

impl SettingsStore for FileSettings {
	fn favorites(&self) -> Vec<relay_core::PeerId> {
		self.favorites.list()
	}

	fn set_favorites(&self, ordered: &[relay_core::PeerId]) -> Result<(), PortError> {
		self.favorites.set(ordered)
	}

	fn auto_answer(&self) -> Vec<relay_core::PeerId> {
		self.favorites.auto_answer()
	}

	fn set_auto_answer(&self, peer: relay_core::PeerId, on: bool) -> Result<(), PortError> {
		self.favorites.set_auto_answer(peer, on)
	}

	fn load(&self) -> StoredSettings {
		let prefs = lock(&self.current);
		let audio = &prefs.audio;
		let explicit = |selection, id: &str| match selection {
			DeviceSelection::Explicit => id.to_owned(),
			DeviceSelection::SystemDefault => String::new(),
		};
		StoredSettings {
			display_name: effective_name(&prefs.profile.display_name, self.default_name.as_deref()),
			input_device: explicit(audio.input_selection, &audio.preferred_input_id),
			output_device: explicit(audio.output_selection, &audio.preferred_output_id),
			processing: processing_of(audio),
			microphone_gain: audio.microphone_gain,
			playback_gain: audio.playback_gain,
			ptt_release_delay_ms: prefs.ptt.release_delay_ms,
			autostart_daemon: prefs.clients.autostart_daemon,
			avatar: *lock(&self.avatar),
			inbound_policy: match prefs.privacy.inbound_policy {
				prefs::InboundPolicy::Anyone => InboundPolicy::Anyone,
				prefs::InboundPolicy::SharedChannelOnly => InboundPolicy::SharedChannelOnly,
				prefs::InboundPolicy::Nobody => InboundPolicy::Nobody,
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
			ringtone_name: lock(&self.ringtone_name).clone(),
			history_enabled: prefs.history.enabled,
			background_available: prefs.background.available,
			call_alert: prefs.calls.alert,
			auto_answer_enabled: prefs.calls.auto_answer_enabled,
			default_transmit_mode: match prefs.calls.default_transmit_mode {
				prefs::TransmitModePref::Muted => relay_core::RouteMode::Muted,
				prefs::TransmitModePref::Unmuted => relay_core::RouteMode::Unmuted,
				prefs::TransmitModePref::PushToTalk => relay_core::RouteMode::PushToTalk,
			},
		}
	}

	fn save(&self, change: &SettingsChange) -> Result<(), PortError> {
		let mut next = lock(&self.current).clone();
		match change {
			SettingsChange::DisplayName(name) => next.profile.display_name.clone_from(name),
			SettingsChange::Devices { input, output } => {
				if input.is_empty() {
					next.audio.input_selection = DeviceSelection::SystemDefault;
					next.audio.preferred_input_id.clear();
				} else {
					next.audio.input_selection = DeviceSelection::Explicit;
					next.audio.preferred_input_id.clone_from(input);
				}
				if output.is_empty() {
					next.audio.output_selection = DeviceSelection::SystemDefault;
					next.audio.preferred_output_id.clear();
				} else {
					next.audio.output_selection = DeviceSelection::Explicit;
					next.audio.preferred_output_id.clone_from(output);
				}
			}
			SettingsChange::PttReleaseDelay(ms) => next.ptt.release_delay_ms = *ms,
			SettingsChange::AutostartDaemon(on) => next.clients.autostart_daemon = *on,
			SettingsChange::Privacy { inbound, advert } => {
				next.privacy.inbound_policy = match inbound {
					InboundPolicy::Anyone => prefs::InboundPolicy::Anyone,
					InboundPolicy::SharedChannelOnly => prefs::InboundPolicy::SharedChannelOnly,
					InboundPolicy::Nobody => prefs::InboundPolicy::Nobody,
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

	fn store_ringtone(&self, name: &str, data: &[u8]) -> Result<Arc<[f32]>, PortError> {
		let limits = RingtoneLimits {
			max_bytes: lock(&self.current).limits.ringtone_max_bytes(),
			max_seconds: lock(&self.current).limits.ringtone_max_seconds,
		};
		let samples = self.ringtone.save(name, data, limits)?;
		*lock(&self.ringtone_name) = name.to_owned();
		Ok(samples)
	}

	fn clear_ringtone(&self) -> Result<(), PortError> {
		self.ringtone.clear()?;
		lock(&self.ringtone_name).clear();
		Ok(())
	}

	fn ringtone(&self) -> Option<(String, Arc<[f32]>)> {
		self.ringtone.load()
	}

	fn apply(&self, entries: &[(String, String)]) -> Result<Vec<Option<String>>, PortError> {
		let mut current = lock(&self.current);
		let (next, answers) = prefs::flat::set_many(&current, entries);
		if answers.iter().any(Option::is_none) {
			self.store
				.save(&next)
				.map_err(|e| PortError::Unavailable(e.to_string()))?;
			let before = current.limits;
			*current = next;
			self.enforce_limits(before, current.limits);
		}
		Ok(answers)
	}

	fn all(&self) -> Result<std::collections::BTreeMap<String, String>, PortError> {
		prefs::flat::all(&lock(&self.current)).map_err(|e| PortError::Unavailable(e.to_string()))
	}

	fn record_call(&self, entry: &NewCallRecord, now_ms: u64) -> Result<(), PortError> {
		self.history
			.record(entry, now_ms)
			.map(drop)
			.map_err(|e| PortError::Unavailable(e.to_string()))
	}

	fn call_history(
		&self,
		limit: usize,
		before: Option<u64>,
	) -> Result<Vec<CallRecord>, PortError> {
		self.history
			.list(limit, before)
			.map_err(|e| PortError::Unavailable(e.to_string()))
	}

	fn clear_call_history(&self) -> Result<(), PortError> {
		self.history
			.clear()
			.map(drop)
			.map_err(|e| PortError::Unavailable(e.to_string()))
	}

	fn avatar_cache_bytes(&self) -> Option<u64> {
		self.avatars.size().ok()
	}

	fn clear_avatar(&self) -> Result<(), PortError> {
		self.avatars
			.clear_local()
			.map_err(|e| PortError::Unavailable(e.to_string()))?;
		*lock(&self.avatar) = None;
		Ok(())
	}

	fn avatar(&self, hash: &[u8; 32]) -> Result<Option<Vec<u8>>, PortError> {
		self.avatars
			.get(hash)
			.map_err(|e| PortError::Unavailable(e.to_string()))
	}
}

impl crate::messaging::ConfigStore for FileSettings {
	fn save(&self, config: crate::messaging::MessagingConfig) -> Result<(), String> {
		let mut next = lock(&self.current).clone();
		next.messages.enabled = config.enabled;
		next.messages.max_offer_size_mib = config.max_offer_size_mib;
		next.messages.transfer_rate_limit_mbps = config.transfer_rate_limit_mbps;
		self.store.save(&next).map_err(|e| e.to_string())?;
		*lock(&self.current) = next;
		Ok(())
	}
}

#[cfg(test)]
mod tests {
	use super::*;

	fn host() -> String {
		sanitize_display_name(&gethostname::gethostname().to_string_lossy())
	}

	#[test]
	fn a_name_the_user_chose_beats_the_platform_default() {
		assert_eq!(effective_name("Ana", Some("Galaxy S25")), "Ana");
	}

	#[test]
	fn the_platform_default_beats_the_hostname_when_no_name_was_chosen() {
		let factory = Profile::default().display_name;
		for stored in ["", "   ", factory.as_str()] {
			assert_eq!(
				effective_name(stored, Some("Galaxy S25 Ultra")),
				"Galaxy S25 Ultra"
			);
		}
	}

	#[test]
	fn a_blank_default_falls_back_to_the_hostname() {
		for blank in ["", "   ", "\u{200b}\t", "<>"] {
			assert_eq!(effective_name("", Some(blank)), host(), "{blank:?}");
		}
		assert_eq!(effective_name("", None), host());
	}

	#[test]
	fn the_default_is_sanitised_like_any_name() {
		assert_eq!(
			effective_name("", Some("  Pixel \u{0007}  <b>9</b> \n Pro ")),
			"Pixel b9/b Pro"
		);
	}
}
