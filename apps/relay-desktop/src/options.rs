//! The desktop's [`HostOptions`]: the notification switch (stored in `desktop.toml`) and the
//! login item (kept by the OS), and the language (stored beside the switch).

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use relay_io::LockExt;
use relay_ui::host_options::{HostOptions, LoginStatus};
use relay_ui::i18n::{Language, locale};
use relay_ui::tr;

use crate::login::LoginItem;
use crate::notify::Dispatcher;
use crate::prefs::DesktopPrefs;

pub struct DesktopOptions {
	notify: AtomicBool,
	/// The language the user picked; `None` while the system language is followed.
	chosen: Mutex<Option<Language>>,
	/// The language in use: the choice, else the system language matched to the supported list.
	language: AtomicUsize,
	path: Option<PathBuf>,
	dispatcher: Arc<Dispatcher>,
	login: LoginItem,
	/// A failed save of the switch, shown as the status until the next successful one.
	save_failure: Mutex<Option<String>>,
}

impl DesktopOptions {
	/// `system_locales` are the system's preferred languages, most wanted first.
	pub fn new(
		path: Option<PathBuf>,
		dispatcher: Arc<Dispatcher>,
		login: LoginItem,
		system_locales: &[String],
	) -> Self {
		let prefs = path.as_deref().map(DesktopPrefs::load).unwrap_or_default();
		let chosen = prefs.language.as_deref().and_then(Language::from_code);
		let language =
			chosen.unwrap_or_else(|| locale::resolve(system_locales.iter().map(String::as_str)));
		Self {
			notify: AtomicBool::new(prefs.notify_incoming),
			chosen: Mutex::new(chosen),
			language: AtomicUsize::new(language.index()),
			path,
			dispatcher,
			login,
			save_failure: Mutex::new(None),
		}
	}
}

impl DesktopOptions {
	/// Write the preferences; a failure is kept as the status until the next successful save.
	fn save(&self) {
		let result = match &self.path {
			Some(path) => DesktopPrefs {
				notify_incoming: self.notify.load(Ordering::Acquire),
				language: self.chosen.lock_unpoisoned().map(|l| l.code().to_owned()),
			}
			.save(path),
			None => Ok(()),
		};
		*self.save_failure.lock_unpoisoned() = result
			.err()
			.map(|e| tr!("options-save-failed", error = e.to_string()));
	}
}

impl HostOptions for DesktopOptions {
	fn notify_incoming(&self) -> bool {
		self.notify.load(Ordering::Acquire)
	}

	fn set_notify_incoming(&self, on: bool) {
		self.notify.store(on, Ordering::Release);
		self.save();
	}

	fn notify_failure(&self) -> Option<String> {
		let saved = self.save_failure.lock_unpoisoned().clone();
		saved.or_else(|| self.dispatcher.failure())
	}

	fn language(&self) -> Language {
		Language::ALL[self
			.language
			.load(Ordering::Acquire)
			.min(Language::ALL.len() - 1)]
	}

	fn set_language(&self, language: Language) {
		*self.chosen.lock_unpoisoned() = Some(language);
		self.language.store(language.index(), Ordering::Release);
		self.save();
	}

	fn login_status(&self) -> LoginStatus {
		self.login.status()
	}

	fn set_launch_at_login(&self, on: bool) -> Result<(), String> {
		self.login.set(on)
	}
}
