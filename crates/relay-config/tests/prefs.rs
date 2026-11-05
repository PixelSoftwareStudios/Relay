mod support;

use relay_config::prefs::{
	DeviceChoice, DeviceSelection, PreferenceStore, Preferences, export_toml, flat, import_toml,
};
use relay_config::{ConfigError, Paths};
use support::{chosen_mic, storage};

fn store(dir: &tempfile::TempDir) -> (PreferenceStore, relay_config::Storage) {
	let s = storage(dir.path());
	(PreferenceStore::open(&s.database).unwrap(), s)
}

#[test]
fn first_run_gives_safe_defaults_without_notices() {
	let dir = tempfile::tempdir().unwrap();
	let (store, s) = store(&dir);
	assert!(s.notices.is_empty());
	let loaded = store.load().unwrap();
	assert!(loaded.notices.is_empty());
	assert_eq!(
		loaded.preferences.audio.input_selection,
		DeviceSelection::Explicit
	);
	assert!(loaded.preferences.audio.preferred_input_id.is_empty());
	assert!(loaded.preferences.privacy.new_calls_receive_only);
	assert!(!loaded.preferences.diagnostics.record_raw_audio);
}

#[test]
fn save_then_load_round_trips_across_reopen() {
	let dir = tempfile::tempdir().unwrap();
	let mut prefs = chosen_mic();
	prefs.audio.microphone_gain = 0.1;
	prefs.profile.display_name = "Zoë \"quoted\"".into();
	{
		let (store, _) = store(&dir);
		store.save(&prefs).unwrap();
	}
	let (store, _) = store(&dir);
	assert_eq!(store.load().unwrap().preferences, prefs);
}

#[test]
fn invalid_values_are_rejected_on_save() {
	let dir = tempfile::tempdir().unwrap();
	let (store, _) = store(&dir);
	let mut p = Preferences::default();
	p.audio.microphone_gain = f32::NAN;
	assert!(store.save(&p).is_err());
	p.audio.microphone_gain = 1.0;
	p.profile.display_name = "  ".into();
	assert!(store.save(&p).is_err());
	assert_eq!(store.load().unwrap().preferences, Preferences::default());
}

#[test]
fn second_writer_is_refused_until_first_drops() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	let first = relay_config::Storage::open(&paths).unwrap();
	assert!(matches!(
		relay_config::Storage::open(&paths),
		Err(ConfigError::Locked(_))
	));
	drop(first);
	relay_config::Storage::open(&paths).unwrap();
}

#[test]
fn missing_explicit_mic_never_falls_back() {
	let prefs = chosen_mic();
	assert_eq!(
		prefs.audio.resolve_input(&["built-in", "usb-mic"]),
		DeviceChoice::Device("usb-mic".into())
	);
	assert_eq!(
		prefs.audio.resolve_input(&["built-in"]),
		DeviceChoice::Unavailable("usb-mic".into())
	);
	assert_eq!(
		Preferences::default().audio.resolve_input(&["built-in"]),
		DeviceChoice::NotSelected
	);
	assert_eq!(
		Preferences::default().audio.resolve_output(&[]),
		DeviceChoice::SystemDefault
	);
}

#[test]
fn get_and_set_are_typed_and_validated() {
	let dir = tempfile::tempdir().unwrap();
	let (store, _) = store(&dir);
	assert_eq!(store.get("audio.microphone_gain").unwrap(), "1.0");
	store.set("audio.microphone_gain", "2.5").unwrap();
	store.set("audio.noise_backend", "rnnoise").unwrap();
	store.set("audio.preferred_input_id", "123").unwrap();
	store
		.set("privacy.new_calls_receive_only", "false")
		.unwrap();
	let prefs = store.load().unwrap().preferences;
	assert_eq!(prefs.audio.microphone_gain, 2.5);
	assert_eq!(prefs.audio.preferred_input_id, "123");
	assert!(!prefs.privacy.new_calls_receive_only);
	assert_eq!(store.get("audio.preferred_input_id").unwrap(), "123");

	assert!(store.set("audio.microphone_gain", "99").is_err());
	assert!(store.set("audio.microphone_gain", "loud").is_err());
	assert!(
		store
			.set("privacy.new_calls_receive_only", "maybe")
			.is_err()
	);
	assert!(store.set("audio.nope", "1").is_err());
	assert!(store.set("config_version", "9").is_err());
	assert!(store.get("nope").is_err());
	assert_eq!(store.load().unwrap().preferences, prefs);
	assert!(flat::keys().contains(&"audio.noise_backend".to_owned()));
	assert!(!flat::keys().contains(&"config_version".to_owned()));
}

#[test]
fn toml_export_import_round_trips_without_secrets() {
	let dir = tempfile::tempdir().unwrap();
	let (store, _) = store(&dir);
	let mut prefs = chosen_mic();
	prefs.profile.display_name = "Hallway".into();
	store.save(&prefs).unwrap();
	let text = store.export_toml().unwrap();
	assert!(text.contains("display_name = \"Hallway\""));
	assert!(!text.to_lowercase().contains("secret"));
	assert_eq!(import_toml(&text).unwrap(), prefs);

	let other = tempfile::tempdir().unwrap();
	let (target, _) = self::store(&other);
	assert_eq!(target.import_toml(&text).unwrap(), prefs);
	assert_eq!(target.load().unwrap().preferences, prefs);
	assert_eq!(export_toml(&prefs).unwrap(), text);
}

#[test]
fn import_rejects_typos_bad_values_and_newer_versions() {
	assert!(import_toml("[audio]\nmicrophone_gane = 1.0\n").is_err());
	assert!(import_toml("[audio]\nmicrophone_gain = 40.0\n").is_err());
	assert!(import_toml("this is = = not toml").is_err());
	assert!(matches!(
		import_toml("config_version = 99\n"),
		Err(ConfigError::UnsupportedVersion { found: 99, .. })
	));
}

#[test]
fn read_only_peek_needs_no_lock_and_writes_nothing() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	assert_eq!(PreferenceStore::read_only(&paths).unwrap(), None);
	let (store, _s) = store(&dir);
	assert_eq!(PreferenceStore::read_only(&paths).unwrap(), None);
	store.save(&chosen_mic()).unwrap();
	// The writer (and its lock) is still alive.
	assert_eq!(
		PreferenceStore::read_only(&paths).unwrap(),
		Some(chosen_mic())
	);
}
