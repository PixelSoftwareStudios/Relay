//! The global push-to-talk key of the desktop app.
//!
//! [`HotkeyHost`] owns the platform backend (`relay-platform`, the same one the
//! CLI uses) and lives on the main thread: macOS and Windows register hotkeys
//! against the event loop that eframe runs there, so [`HotkeyHost::sync`] is
//! called from the frame logic. The wanted key comes from the
//! `hotkeys.push_to_talk` setting ([`HotkeyShared`], filled by
//! [`follow_settings`] and by the settings panel when the user saves); when it
//! changes the key is unregistered and registered again. Key edges go to [`PttControl`] from a task, so they keep working with
//! the window hidden. A refused registration is kept as plain words for
//! Settings and never stops the on-screen button.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_io::LockExt;
use relay_platform::hotkey::{
	BackendStatus, HotkeyBackend, HotkeyBinding, HotkeyError, HotkeyEvent, HotkeyStream,
};
use relay_ui::controller::Controller;
use relay_ui::hotkey_settings::HotkeyState;
use relay_ui::hub::Hub;
use relay_ui::model::Link;
use relay_ui::shell::Shell;
use relay_ui::tr;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use crate::ptt::PttControl;

/// How often the engine is asked for the stored key while connected, so a key
/// changed from another client is followed too.
const REREAD: Duration = Duration::from_secs(5);

/// The wanted key and what became of it, shared with the shell.
#[derive(Default)]
pub struct HotkeyShared {
	/// The stored accelerator; `None` until the engine has been asked.
	accelerator: Mutex<Option<String>>,
	state: Mutex<HotkeyState>,
}

impl HotkeyShared {
	/// A new stored value; true when it differs from the last one.
	pub fn set_accelerator(&self, accelerator: &str) -> bool {
		let mut slot = self.accelerator.lock_unpoisoned();
		let changed = slot.as_deref() != Some(accelerator);
		*slot = Some(accelerator.to_owned());
		changed
	}

	fn wanted(&self) -> Option<String> {
		let accelerator = self.accelerator.lock_unpoisoned().clone()?;
		let accelerator = accelerator.trim();
		(!accelerator.is_empty()).then(|| accelerator.to_owned())
	}

	pub fn state(&self) -> HotkeyState {
		self.state.lock_unpoisoned().clone()
	}

	fn set_state(&self, state: HotkeyState) {
		*self.state.lock_unpoisoned() = state;
	}
}

/// Registers the wanted key and forwards its edges. Main thread only.
pub struct HotkeyHost {
	backend: Box<dyn HotkeyBackend>,
	shared: Arc<HotkeyShared>,
	control: Arc<PttControl>,
	handle: Handle,
	/// What was last acted on (the key may have been refused: no retry per frame).
	applied: Option<Option<String>>,
	pump: Option<JoinHandle<()>>,
}

impl HotkeyHost {
	pub fn new(
		backend: Box<dyn HotkeyBackend>,
		shared: Arc<HotkeyShared>,
		control: Arc<PttControl>,
		handle: Handle,
	) -> Self {
		Self {
			backend,
			shared,
			control,
			handle,
			applied: None,
			pump: None,
		}
	}

	/// Bring the registration in line with the setting. Cheap when nothing
	/// changed; call it every frame from the main thread.
	pub fn sync(&mut self) {
		let wanted = self.shared.wanted();
		if self.applied.as_ref() == Some(&wanted) {
			return;
		}
		self.unregister();
		self.applied = Some(wanted.clone());
		let Some(accelerator) = wanted else {
			self.shared.set_state(HotkeyState::Off);
			return;
		};
		match self.register(&accelerator) {
			Ok(keys) => {
				self.pump = Some(self.handle.spawn(pump(keys, self.control.clone())));
				self.shared.set_state(HotkeyState::Registered(accelerator));
			}
			Err(error) => {
				tracing::warn!(%error, %accelerator, "push-to-talk hotkey not registered");
				self.shared
					.set_state(HotkeyState::Failed(reason(&error, &accelerator)));
			}
		}
	}

	fn register(&mut self, accelerator: &str) -> Result<HotkeyStream, HotkeyError> {
		let status = self.backend.probe();
		if !status.is_available() {
			return Err(HotkeyError::Unavailable(status));
		}
		self.backend
			.start(&HotkeyBinding::push_to_talk(accelerator))
	}

	/// Drop the registration; a held key is released at once.
	fn unregister(&mut self) {
		self.backend.stop();
		if let Some(pump) = self.pump.take() {
			pump.abort();
			self.control.cancel();
		}
	}
}

impl Drop for HotkeyHost {
	fn drop(&mut self) {
		self.unregister();
		self.control.cancel();
	}
}

/// Key edges to presses. The stream ending means the key can never come up.
async fn pump(mut keys: HotkeyStream, control: Arc<PttControl>) {
	while let Some(edge) = keys.recv().await {
		match edge {
			HotkeyEvent::Pressed => control.press_current(),
			HotkeyEvent::Released => control.release(),
		}
	}
	control.cancel();
}

/// Why registration failed, in the user's language.
fn reason(error: &HotkeyError, accelerator: &str) -> String {
	match error {
		HotkeyError::Unavailable(BackendStatus::NeedsPermission(_)) => {
			tr!("hotkey-reason-permission").to_owned()
		}
		HotkeyError::Unavailable(_) => tr!("hotkey-reason-unsupported").to_owned(),
		HotkeyError::InvalidBinding(..) => tr!("hotkey-reason-invalid", key = accelerator),
		HotkeyError::Registration(_) => tr!("hotkey-reason-taken").to_owned(),
		other => tr!("hotkey-reason-other", detail = other.to_string()),
	}
}

/// Read the stored key from the engine whenever the app is connected, then
/// now and then, so the key is registered without the settings page open and
/// follows changes made elsewhere.
pub async fn follow_settings(hub: Arc<Hub>, controller: Arc<Controller>, shell: Arc<dyn Shell>) {
	let mut tick = tokio::time::interval(Duration::from_millis(500));
	let mut last_read: Option<tokio::time::Instant> = None;
	loop {
		tick.tick().await;
		if hub.read(|s| s.link != Link::Connected) {
			last_read = None;
			continue;
		}
		if last_read.is_some_and(|at| at.elapsed() < REREAD) {
			continue;
		}
		last_read = Some(tokio::time::Instant::now());
		match controller.hotkey_settings().await {
			Ok(values) => shell.hotkey_accelerator(&values.accelerator),
			Err(error) => tracing::debug!(%error, "push-to-talk key not read"),
		}
	}
}
