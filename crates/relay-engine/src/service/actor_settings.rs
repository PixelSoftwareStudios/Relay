//! Actor side of `SetSettings`: save by key, then apply what a running daemon can.

use relay_core::{Input, SystemEvent};

use super::actor::Actor;
use super::error::ServiceError;
use super::ports::PortError;
use super::settings_keys::{KeyOutcome, KeyResult, is_live, is_messaging};

/// Keys another part of the engine applies after the actor answered (the domain core owns
/// playback gain and the push-to-talk delay, the messaging actor owns `messages.*`).
fn applied_elsewhere(key: &str) -> bool {
	is_messaging(key) || matches!(key, "audio.playback_gain" | "ptt.release_delay_ms")
}

impl Actor {
	/// Tells the domain the call defaults. They apply to calls that connect from now on.
	pub(super) fn send_call_defaults(&mut self) {
		let aux = &self.directory.aux;
		let input = Input::System(SystemEvent::CallDefaults {
			mode: aux.default_transmit_mode,
		});
		let _ = self.run_input(input);
	}

	/// Saves `entries` together and applies the live ones that live in the actor's own state.
	pub(super) fn settings(
		&mut self,
		entries: &[(String, String)],
	) -> Result<Vec<KeyResult>, ServiceError> {
		let answers = self.ports.settings.apply(entries).map_err(|e| match e {
			PortError::Unsupported(what) => ServiceError::Unimplemented(what),
			other => ServiceError::from(other),
		})?;
		let stored = self.ports.settings.load();
		let mut results = Vec::with_capacity(entries.len());
		let (mut profile, mut changed, mut defaults) = (false, false, false);
		for ((key, _), answer) in entries.iter().zip(answers) {
			let outcome = match answer {
				Some(reason) => KeyOutcome::Rejected(reason),
				None if !is_live(key) => KeyOutcome::SavedNeedsRestart(
					"read when the daemon starts; restart it to apply".to_owned(),
				),
				None => {
					changed = true;
					match key.as_str() {
						"profile.display_name" => {
							self.directory
								.aux
								.display_name
								.clone_from(&stored.display_name);
							profile = true;
						}
						"privacy.inbound_policy" => {
							self.directory.aux.inbound_policy = stored.inbound_policy;
							profile = true;
						}
						"privacy.advert_mode" => {
							self.directory.aux.advert_mode = stored.advert_mode;
							profile = true;
						}
						"history.enabled" => {
							self.directory.aux.history_enabled = stored.history_enabled;
						}
						"calls.default_transmit_mode" => {
							self.directory.aux.default_transmit_mode = stored.default_transmit_mode;
							defaults = true;
						}
						"clients.autostart_daemon" => {
							self.directory.aux.autostart_daemon = stored.autostart_daemon;
						}
						"audio.microphone_gain" => {
							match self.ports.media.set_microphone_gain(stored.microphone_gain) {
								Ok(()) => {
									self.directory.aux.microphone_gain = stored.microphone_gain
								}
								Err(error) => {
									results.push(KeyResult {
										key: key.clone(),
										outcome: KeyOutcome::SavedNeedsRestart(error.to_string()),
									});
									continue;
								}
							}
						}
						k if k.starts_with("audio.") && !applied_elsewhere(k) => {
							self.directory.aux.ring = stored.ring;
						}
						_ => {}
					}
					KeyOutcome::AppliedLive
				}
			};
			results.push(KeyResult {
				key: key.clone(),
				outcome,
			});
		}
		if defaults {
			self.send_call_defaults();
		}
		if profile {
			self.ports
				.peers
				.profile_changed(&self.directory.aux.profile());
		}
		if changed {
			self.publish_settings();
		}
		Ok(results)
	}
}
