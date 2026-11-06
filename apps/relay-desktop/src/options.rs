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

/// Reads the system's preferred languages, most wanted first; called at start
/// and whenever the user picks "System", so a changed system language is seen.
pub type SystemLocales = Box<dyn Fn() -> Vec<String> + Send + Sync>;

pub struct DesktopOptions {
	notify: AtomicBool,
	/// The language the user picked; `None` while the system language is followed.
	chosen: Mutex<Option<Language>>,
	/// The system language matched to the supported list, as last detected.
	system: AtomicUsize,
	locales: SystemLocales,
	path: Option<PathBuf>,
	dispatcher: Arc<Dispatcher>,
	login: LoginItem,
	/// A failed save of the switch, shown as the status until the next successful one.
	save_failure: Mutex<Option<String>>,
}

impl DesktopOptions {
	/// `locales` reads the system's preferred languages.
	pub fn new(
		path: Option<PathBuf>,
		dispatcher: Arc<Dispatcher>,
		login: LoginItem,
		locales: SystemLocales,
	) -> Self {
		let prefs = path.as_deref().map(DesktopPrefs::load).unwrap_or_default();
		let chosen = prefs.language.as_deref().and_then(Language::from_code);
		let system = detect(&locales);
		Self {
			notify: AtomicBool::new(prefs.notify_incoming),
			chosen: Mutex::new(chosen),
			system: AtomicUsize::new(system.index()),
			locales,
			path,
			dispatcher,
			login,
			save_failure: Mutex::new(None),
		}
	}
}

fn detect(locales: &SystemLocales) -> Language {
	locale::resolve(locales().iter().map(String::as_str))
}

fn language_at(index: usize) -> Language {
	Language::ALL[index.min(Language::ALL.len() - 1)]
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
		self.language_choice()
			.unwrap_or_else(|| self.system_language())
	}

	fn language_choice(&self) -> Option<Language> {
		*self.chosen.lock_unpoisoned()
	}

	fn system_language(&self) -> Language {
		language_at(self.system.load(Ordering::Acquire))
	}

	fn set_language_choice(&self, choice: Option<Language>) {
		if choice.is_none() {
			self.system
				.store(detect(&self.locales).index(), Ordering::Release);
		}
		*self.chosen.lock_unpoisoned() = choice;
		self.save();
	}

	fn login_status(&self) -> LoginStatus {
		self.login.status()
	}

	fn set_launch_at_login(&self, on: bool) -> Result<(), String> {
		self.login.set(on)
	}
}
