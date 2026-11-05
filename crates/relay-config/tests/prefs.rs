use std::fs;
use std::sync::Arc;

use relay_config::fsx::WriterLock;
use relay_config::prefs::{
	CURRENT_VERSION, DeviceChoice, DeviceSelection, PreferenceStore, Preferences,
};
use relay_config::{ConfigError, Notice, Paths};

fn open(dir: &tempfile::TempDir) -> (PreferenceStore, Paths) {
	let paths = Paths::under(dir.path());
	let lock = Arc::new(WriterLock::acquire(&paths.writer_lock_file()).unwrap());
	(
		PreferenceStore::open(paths.preferences_file(), lock).unwrap(),
		paths,
	)
}

fn chosen_mic() -> Preferences {
	let mut p = Preferences::default();
	p.audio.preferred_input_id = "usb-mic".into();
	p.privacy.new_calls_receive_only = false;
	p
}

#[test]
fn first_run_gives_safe_defaults_without_notices() {
	let dir = tempfile::tempdir().unwrap();
	let (store, _) = open(&dir);
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
fn save_then_load_round_trips() {
	let dir = tempfile::tempdir().unwrap();
	let (store, _) = open(&dir);
	let prefs = chosen_mic();
	store.save(&prefs).unwrap();
	assert_eq!(store.load().unwrap().preferences, prefs);
}

#[test]
fn corrupt_main_recovers_backup_without_enabling_mic() {
	let dir = tempfile::tempdir().unwrap();
	let (store, paths) = open(&dir);
	store.save(&chosen_mic()).unwrap();
	store.save(&chosen_mic()).unwrap(); // creates .bak
	fs::write(paths.preferences_file(), b"\xff\x00 not toml [[[").unwrap();
	let loaded = store.load().unwrap();
	assert_eq!(loaded.notices, vec![Notice::PreferencesRecoveredFromBackup]);
	assert!(loaded.preferences.audio.preferred_input_id.is_empty());
	assert!(loaded.preferences.privacy.new_calls_receive_only);
}

#[test]
fn crash_between_backup_and_rename_recovers_via_backup() {
	let dir = tempfile::tempdir().unwrap();
	let (store, paths) = open(&dir);
	store.save(&chosen_mic()).unwrap();
	store.save(&chosen_mic()).unwrap();
	fs::remove_file(paths.preferences_file()).unwrap(); // main vanished mid-replace
	let loaded = store.load().unwrap();
	assert_eq!(loaded.notices, vec![Notice::PreferencesRecoveredFromBackup]);
	assert!(loaded.preferences.audio.preferred_input_id.is_empty());
}

#[test]
fn leftover_tmp_file_is_ignored() {
	let dir = tempfile::tempdir().unwrap();
	let (store, paths) = open(&dir);
	store.save(&chosen_mic()).unwrap();
	let mut tmp = paths.preferences_file().into_os_string();
	tmp.push(".tmp");
	fs::write(tmp, b"half written").unwrap();
	assert_eq!(store.load().unwrap().preferences, chosen_mic());
}

#[test]
fn corrupt_main_and_backup_resets_with_notice_and_preserves_file() {
	let dir = tempfile::tempdir().unwrap();
	let (store, paths) = open(&dir);
	fs::create_dir_all(paths.config_dir()).unwrap();
	fs::write(paths.preferences_file(), b"garbage = = =").unwrap();
	let loaded = store.load().unwrap();
	match loaded.notices.as_slice() {
		[Notice::PreferencesReset { preserved: Some(p) }] => assert!(p.exists()),
		other => panic!("unexpected notices {other:?}"),
	}
	assert_eq!(loaded.preferences, Preferences::default());
}

#[test]
fn legacy_v0_is_migrated_and_mic_stays_explicit() {
	let dir = tempfile::tempdir().unwrap();
	let (store, paths) = open(&dir);
	fs::create_dir_all(paths.config_dir()).unwrap();
	fs::write(
		paths.preferences_file(),
		"[audio]\ninput_device = \"legacy-mic\"\n",
	)
	.unwrap();
	let loaded = store.load().unwrap();
	assert_eq!(
		loaded.notices,
		vec![Notice::PreferencesMigrated {
			from: 0,
			to: CURRENT_VERSION
		}]
	);
	let audio = loaded.preferences.audio;
	assert_eq!(audio.preferred_input_id, "legacy-mic");
	assert_eq!(audio.input_selection, DeviceSelection::Explicit);
}

#[test]
fn newer_version_is_refused_not_downgraded() {
	let dir = tempfile::tempdir().unwrap();
	let (store, paths) = open(&dir);
	fs::create_dir_all(paths.config_dir()).unwrap();
	fs::write(paths.preferences_file(), "config_version = 99\n").unwrap();
	assert!(matches!(
		store.load(),
		Err(ConfigError::UnsupportedVersion { found: 99, .. })
	));
}

#[test]
fn invalid_values_are_rejected_on_save() {
	let dir = tempfile::tempdir().unwrap();
	let (store, _) = open(&dir);
	let mut p = Preferences::default();
	p.audio.microphone_gain = f32::NAN;
	assert!(store.save(&p).is_err());
	p.audio.microphone_gain = 1.0;
	p.profile.display_name = "  ".into();
	assert!(store.save(&p).is_err());
}

#[test]
fn second_writer_is_refused_until_first_drops() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	let first = WriterLock::acquire(&paths.writer_lock_file()).unwrap();
	assert!(matches!(
		WriterLock::acquire(&paths.writer_lock_file()),
		Err(ConfigError::Locked(_))
	));
	drop(first);
	WriterLock::acquire(&paths.writer_lock_file()).unwrap();
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
