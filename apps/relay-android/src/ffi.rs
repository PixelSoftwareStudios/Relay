//! The engine control surface Kotlin calls (UniFFI).

use std::sync::Arc;

use crate::bridge::{EngineConfig, EngineError, FoundService, HostBridge, InsetsPx, PickOutcome};
use crate::host::{Host, host};

/// Handle to the in-process engine. The state lives in a process-wide host so
/// the activity's native thread reaches the same engine.
#[derive(uniffi::Object)]
pub struct RelayEngine {
	host: Arc<Host>,
}

#[uniffi::export]
impl RelayEngine {
	/// Starts the engine (blocking for a moment: SQLite, sockets, audio
	/// devices), or returns the running one. Call from a worker thread.
	#[uniffi::constructor]
	pub fn new(
		config: EngineConfig,
		bridge: Arc<dyn HostBridge>,
	) -> Result<Arc<Self>, EngineError> {
		crate::logging::init();
		Ok(Arc::new(Self {
			host: Host::start(&config, bridge, None)?,
		}))
	}

	/// Ends any call, leaves the LAN and stops the engine.
	pub fn stop(&self) {
		self.host.stop();
	}

	/// The activity is started (true) or stopped (false).
	pub fn set_foreground(&self, foreground: bool) {
		self.host.set_foreground(foreground);
	}

	pub fn set_microphone_granted(&self, granted: bool) {
		self.host.set_microphone_granted(granted);
	}

	pub fn set_insets(&self, insets: InsetsPx) {
		self.host.shell.set_insets(insets);
	}

	pub fn complete_pick(&self, request_id: u64, outcome: PickOutcome) {
		self.host.shell.complete_pick(request_id, outcome);
	}

	/// Mute or unmute the microphone (notification action).
	pub fn set_muted(&self, muted: bool) {
		self.host.mute(muted);
	}

	pub fn answer(&self) {
		self.host.answer();
	}

	pub fn hang_up(&self) {
		self.host.hang_up();
	}

	/// A volume key went down or up. Returns true when the key was used for
	/// hold-to-talk (a push-to-talk call is up), false to let Android change the volume.
	pub fn talk_key(&self, pressed: bool) -> bool {
		self.host.talk_key(pressed)
	}

	/// The talk button or keys can no longer report their release (pause,
	/// focus or surface loss): release whatever is held. Always safe to call.
	pub fn cancel_talk(&self) {
		self.host.cancel_talk();
	}

	/// A service the platform browser resolved.
	pub fn service_found(&self, service: FoundService) {
		self.host.found(service);
	}

	pub fn service_lost(&self, fullname: String) {
		self.host.lost(fullname);
	}
}

/// True when the engine is running in this process.
#[uniffi::export]
pub fn engine_running() -> bool {
	host().is_some()
}
