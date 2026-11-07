//! Choosing a ringtone: the shell's audio picker, then the engine. The file is read by the
//! shell (a file dialog on desktop, the system document picker on Android), sent to the engine
//! as bytes and decoded there, off the real-time thread. The UI keeps nothing but a busy flag
//! and the last answer, shown under the controls like any saved setting.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::actions::Actions;
use crate::controller::{ControlError, Controller};
use crate::engine::{SettingOutcome, SettingResult};

/// Largest file the UI hands to the engine; the engine applies the configured
/// `limits.ringtone_max_kib` (at most this).
pub const PICK_MAX_BYTES: usize = 8 * 1024 * 1024;

/// The key the ringtone answer is reported under (not a stored preference).
pub const KEY_RINGTONE_FILE: &str = "audio.ringtone_file";

impl Actions {
	/// Ask the shell for an audio file and make it this device's ringtone. `busy` is true from
	/// the click until the engine has answered.
	pub fn pick_ringtone(&self, busy: &Arc<AtomicBool>) {
		self.ringtone_job(busy, |controller, shell| async move {
			match shell.pick_audio().await {
				Ok(Some(file)) => controller
					.set_ringtone(&file.name, file.bytes)
					.await
					.map(|()| true),
				Ok(None) => Ok(false),
				Err(message) => Err(ControlError::Engine(message)),
			}
		});
	}

	/// Go back to the built-in chime.
	pub fn reset_ringtone(&self, busy: &Arc<AtomicBool>) {
		self.ringtone_job(busy, |controller, _| async move {
			controller.reset_ringtone().await.map(|()| true)
		});
	}

	fn ringtone_job<F>(
		&self,
		busy: &Arc<AtomicBool>,
		job: impl FnOnce(Arc<Controller>, Arc<dyn crate::shell::Shell>) -> F,
	) where
		F: Future<Output = Result<bool, ControlError>> + Send + 'static,
	{
		if busy.swap(true, Ordering::AcqRel) {
			return;
		}
		let (shared, shell, busy) = (self.shared.clone(), self.shell.clone(), Arc::clone(busy));
		let work = job(self.controller.clone(), self.shell.clone());
		self.runtime.spawn(async move {
			let result = work.await.map(|changed| {
				changed.then(|| {
					vec![SettingResult {
						key: KEY_RINGTONE_FILE.to_owned(),
						outcome: SettingOutcome::AppliedLive,
						reason: String::new(),
					}]
				})
			});
			match result {
				Ok(Some(results)) => shared
					.inbox
					.set_setting_saved(KEY_RINGTONE_FILE, Ok(results)),
				Ok(None) => {}
				Err(e) => shared
					.inbox
					.set_setting_saved(KEY_RINGTONE_FILE, Err(e.to_string())),
			}
			busy.store(false, Ordering::Release);
			shell.request_repaint();
		});
	}
}
