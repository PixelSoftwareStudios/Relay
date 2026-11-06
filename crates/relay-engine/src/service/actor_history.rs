//! Actor side of the call history: record a finished call, clear on request.

use relay_core::CallView;

use super::actor::Actor;
use super::error::ServiceError;

impl Actor {
	/// Records a call that just ended. Nothing is written while `history.enabled` is off.
	pub(super) fn record_call(&mut self, view: &CallView) {
		let now = self.now();
		let Some(entry) = self.recorder.observe(view, &self.directory, now) else {
			return;
		};
		if !self.directory.aux.history_enabled {
			return;
		}
		match self
			.ports
			.settings
			.record_call(&entry, self.recorder.now_ms())
		{
			Ok(()) => self.history_changed(),
			Err(error) => tracing::warn!(%error, "call history entry not stored"),
		}
	}

	/// Deletes every entry and tells watchers.
	pub(super) fn clear_history(&mut self) -> Result<(), ServiceError> {
		self.ports.settings.clear_call_history()?;
		self.history_changed();
		Ok(())
	}

	fn history_changed(&mut self) {
		self.directory.aux.history_revision += 1;
		self.publish_settings();
	}
}
