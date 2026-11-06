//! One preference at a time (`SetSettings`) for the switches and selectors that show the
//! engine's per-key answer under them: push-to-talk on or off, the mode a call starts in.

use crate::actions::Actions;
use crate::controller::{ControlError, Controller};
use crate::engine::SettingResult;
use crate::model::Mode;

/// Push-to-talk may be chosen.
pub const KEY_PTT_ENABLED: &str = "ptt.enabled";
/// The mode a call starts in once it connects.
pub const KEY_DEFAULT_MODE: &str = "calls.default_transmit_mode";

/// The preference text of a transmit mode.
pub fn mode_text(mode: Mode) -> &'static str {
	match mode {
		Mode::Muted => "muted",
		Mode::Unmuted => "unmuted",
		Mode::PushToTalk => "push_to_talk",
	}
}

impl Controller {
	/// Stores one preference and returns the engine's answer for its key.
	pub async fn set_setting(
		&self,
		key: &str,
		value: String,
	) -> Result<Vec<SettingResult>, ControlError> {
		let session = self.slot.get().ok_or(ControlError::NotConnected)?;
		Ok(session
			.ops
			.set_settings(vec![(key.to_owned(), value)])
			.await?)
	}
}

impl Actions {
	/// Saves `key` in the background; the answer reaches the UI through the inbox and is shown
	/// under the control (see `Local::setting_reports`).
	pub fn save_setting(&self, key: &'static str, value: String) {
		let (controller, shared, shell) = (
			self.controller.clone(),
			self.shared.clone(),
			self.shell.clone(),
		);
		self.runtime.spawn(async move {
			let result = controller
				.set_setting(key, value)
				.await
				.map_err(|e| e.to_string());
			shared.inbox.set_setting_saved(key, result);
			shell.request_repaint();
		});
	}
}
