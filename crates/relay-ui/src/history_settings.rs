//! The call history switch and clearing, through the controller. The switch is the
//! preference `history.enabled`, sent with `SetSettings`; the engine's per-key answer
//! (applied now, saved, rejected) is shown as given, like the hotkey panel does.

use crate::controller::{ControlError, Controller};
use crate::engine::{Command, SettingResult};

/// The preference behind "Keep call history".
pub const KEY_ENABLED: &str = "history.enabled";

impl Controller {
	/// Turns recording of finished calls on or off. Entries already stored stay.
	pub async fn set_history_enabled(
		&self,
		enabled: bool,
	) -> Result<Vec<SettingResult>, ControlError> {
		let session = self.slot.get().ok_or(ControlError::NotConnected)?;
		Ok(session
			.ops
			.set_settings(vec![(KEY_ENABLED.to_owned(), enabled.to_string())])
			.await?)
	}

	/// Deletes every call history entry on this device.
	pub async fn clear_call_history(&self) -> Result<(), ControlError> {
		self.run(Command::ClearCallHistory).await.map(drop)
	}
}
