//! The language preference: default from the system locales, stored beside
//! the notification switch, and remembered.

use std::sync::Arc;

use relay_desktop::login::{LoginBackend, LoginItem};
use relay_desktop::notify::{Dispatcher, Notifier};
use relay_desktop::options::{DesktopOptions, SystemLocales};
use relay_desktop::prefs::DesktopPrefs;
use relay_ui::host_options::HostOptions;
use relay_ui::i18n::Language;

struct Silent;

impl Notifier for Silent {
	fn incoming_call(&self, _: &str) -> Result<(), String> {
		Ok(())
	}
}

struct NoLogin;

impl LoginBackend for NoLogin {
	fn is_enabled(&self) -> Result<bool, String> {
		Ok(false)
	}
	fn enable(&self) -> Result<(), String> {
		Ok(())
	}
	fn disable(&self) -> Result<(), String> {
		Ok(())
	}
}

fn options(path: Option<std::path::PathBuf>, locales: &[&str]) -> DesktopOptions {
	let locales: Vec<String> = locales.iter().map(|l| (*l).to_owned()).collect();
	options_with(path, Box::new(move || locales.clone()))
}

fn options_with(path: Option<std::path::PathBuf>, locales: SystemLocales) -> DesktopOptions {
	let backend: Arc<dyn LoginBackend> = Arc::new(NoLogin);
	DesktopOptions::new(
		path,
		Arc::new(Dispatcher::new(Arc::new(Silent))),
		LoginItem::new(Ok(backend)),
		locales,
	)
}

#[test]
fn the_system_language_is_the_default_and_english_the_last_resort() {
	assert_eq!(options(None, &["pt_BR.UTF-8"]).language(), Language::PtBr);
	assert_eq!(
		options(None, &["fr_FR", "zh_Hans_CN"]).language(),
		Language::ZhCn
	);
	assert_eq!(options(None, &["fr_FR"]).language(), Language::En);
	assert_eq!(options(None, &[]).language(), Language::En);
}

#[test]
fn a_choice_is_stored_with_the_other_preferences_and_beats_the_system() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("cfg").join("desktop.toml");
	let first = options(Some(path.clone()), &["de"]);
	assert_eq!(first.language(), Language::De);
	first.set_language_choice(Some(Language::Ja));
	assert_eq!(first.language(), Language::Ja);
	assert_eq!(DesktopPrefs::load(&path).language.as_deref(), Some("ja"));
	assert!(DesktopPrefs::load(&path).notify_incoming);

	let again = options(Some(path.clone()), &["de"]);
	assert_eq!(again.language(), Language::Ja, "remembered over the system");

	// The notification switch keeps the language when it is saved.
	again.set_notify_incoming(false);
	let prefs = DesktopPrefs::load(&path);
	assert!(!prefs.notify_incoming);
	assert_eq!(prefs.language.as_deref(), Some("ja"));
}

#[test]
fn an_unknown_stored_code_follows_the_system_language() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("desktop.toml");
	std::fs::write(&path, "language = \"tlh\"\n").unwrap();
	assert_eq!(options(Some(path), &["es"]).language(), Language::Es);
}

#[test]
fn picking_system_stores_no_language_and_detects_again() {
	let dir = tempfile::tempdir().unwrap();
	let path = dir.path().join("desktop.toml");
	let locale = Arc::new(std::sync::Mutex::new("de".to_owned()));
	let source = locale.clone();
	let o = options_with(
		Some(path.clone()),
		Box::new(move || vec![source.lock().unwrap().clone()]),
	);
	o.set_language_choice(Some(Language::Ja));
	assert_eq!(o.language_choice(), Some(Language::Ja));
	assert_eq!(o.system_language(), Language::De);

	*locale.lock().unwrap() = "ko_KR".to_owned();
	o.set_language_choice(None);
	assert_eq!(o.language_choice(), None);
	assert_eq!(o.system_language(), Language::Ko, "detected again");
	assert_eq!(o.language(), Language::Ko);
	assert_eq!(DesktopPrefs::load(&path).language, None);
}
