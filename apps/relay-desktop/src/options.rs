//! The desktop's [`HostOptions`]: the notification switch (stored in `desktop.toml`) and the
//! login item (kept by the OS).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use relay_io::LockExt;
use relay_ui::host_options::{HostOptions, LoginStatus};

use crate::login::LoginItem;
use crate::notify::Dispatcher;
use crate::prefs::DesktopPrefs;

pub struct DesktopOptions {
	notify: AtomicBool,
	path: Option<PathBuf>,
	dispatcher: Arc<Dispatcher>,
	login: LoginItem,
	/// A failed save of the switch, shown as the status until the next successful one.
	save_failure: Mutex<Option<String>>,
}

impl DesktopOptions {
	pub fn new(path: Option<PathBuf>, dispatcher: Arc<Dispatcher>, login: LoginItem) -> Self {
		let prefs = path.as_deref().map(DesktopPrefs::load).unwrap_or_default();
		Self {
			notify: AtomicBool::new(prefs.notify_incoming),
			path,
			dispatcher,
			login,
			save_failure: Mutex::new(None),
		}
	}
}

impl HostOptions for DesktopOptions {
	fn notify_incoming(&self) -> bool {
		self.notify.load(Ordering::Acquire)
	}

	fn set_notify_incoming(&self, on: bool) {
		self.notify.store(on, Ordering::Release);
		let result = match &self.path {
			Some(path) => DesktopPrefs {
				notify_incoming: on,
			}
			.save(path),
			None => Ok(()),
		};
		*self.save_failure.lock_unpoisoned() = result
			.err()
			.map(|e| format!("the choice could not be saved: {e}"));
	}

	fn notify_failure(&self) -> Option<String> {
		let saved = self.save_failure.lock_unpoisoned().clone();
		saved.or_else(|| self.dispatcher.failure())
	}

	fn login_status(&self) -> LoginStatus {
		self.login.status()
	}

	fn set_launch_at_login(&self, on: bool) -> Result<(), String> {
		self.login.set(on)
	}
}
