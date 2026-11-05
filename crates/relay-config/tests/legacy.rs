mod support;

use std::fs;

use relay_config::identity::{TrustDecision, TrustLevel, TrustStore};
use relay_config::prefs::{CURRENT_VERSION, DeviceSelection, PreferenceStore};
use relay_config::{ConfigError, Notice, Paths, Storage};

fn key_hex(b: u8) -> String {
	hex::encode([b; 32])
}

fn write_legacy(paths: &Paths, prefs: &str, trust: Option<&str>) {
	fs::create_dir_all(paths.config_dir()).unwrap();
	fs::create_dir_all(paths.data_dir()).unwrap();
	fs::write(paths.legacy_preferences_file(), prefs).unwrap();
	if let Some(trust) = trust {
		fs::write(paths.legacy_trust_file(), trust).unwrap();
	}
}

fn trust_toml() -> String {
	format!(
		"version = 1\n[[entries]]\npublic_key = \"{}\"\ndisplay_name = \"Old friend\"\nlevel = \"verified\"\nfirst_seen = 42\n",
		key_hex(7)
	)
}

#[test]
fn toml_files_are_imported_once_then_retired() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	write_legacy(
		&paths,
		"[audio]\ninput_device = \"legacy-mic\"\n",
		Some(&trust_toml()),
	);
	{
		let s = Storage::open(&paths).unwrap();
		assert_eq!(
			s.notices,
			vec![Notice::PreferencesMigrated {
				from: 0,
				to: CURRENT_VERSION
			}]
		);
		let prefs = PreferenceStore::open(&s.database)
			.unwrap()
			.load()
			.unwrap()
			.preferences;
		assert_eq!(prefs.audio.preferred_input_id, "legacy-mic");
		assert_eq!(prefs.audio.input_selection, DeviceSelection::Explicit);
		let trust = TrustStore::open(&s.database).unwrap();
		assert_eq!(
			trust.lookup(&[7; 32]),
			TrustDecision::Known(TrustLevel::Verified)
		);
		assert_eq!(trust.entries()[0].first_seen, 42);
	}
	assert!(!paths.legacy_preferences_file().exists());
	assert!(!paths.legacy_trust_file().exists());
	assert!(paths.config_dir().join("config.toml.imported").exists());

	// A stray file appearing later is not re-imported and nothing writes TOML.
	fs::write(
		paths.legacy_preferences_file(),
		"[profile]\ndisplay_name = \"X\"\n",
	)
	.unwrap();
	let s = Storage::open(&paths).unwrap();
	assert!(s.notices.is_empty());
	let prefs = PreferenceStore::open(&s.database).unwrap();
	prefs.set("audio.microphone_gain", "2.0").unwrap();
	assert_eq!(
		fs::read_to_string(paths.legacy_preferences_file()).unwrap(),
		"[profile]\ndisplay_name = \"X\"\n"
	);
	assert_ne!(prefs.load().unwrap().preferences.profile.display_name, "X");
}

#[test]
fn damaged_legacy_prefs_use_backup_without_mic_and_say_so() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	write_legacy(&paths, "garbage = = =", None);
	fs::write(
		format!("{}.bak", paths.legacy_preferences_file().display()),
		"[audio]\npreferred_input_id = \"usb\"\n[privacy]\nnew_calls_receive_only = false\n",
	)
	.unwrap();
	let s = Storage::open(&paths).unwrap();
	assert_eq!(s.notices, vec![Notice::PreferencesRecoveredFromBackup]);
	let prefs = PreferenceStore::open(&s.database)
		.unwrap()
		.load()
		.unwrap()
		.preferences;
	assert!(prefs.audio.preferred_input_id.is_empty());
	assert!(prefs.privacy.new_calls_receive_only);
}

#[test]
fn damaged_legacy_prefs_without_backup_reset_with_preserved_copy() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	write_legacy(&paths, "garbage = = =", None);
	let s = Storage::open(&paths).unwrap();
	match s.notices.as_slice() {
		[Notice::PreferencesReset { preserved: Some(p) }] => assert!(p.exists()),
		other => panic!("{other:?}"),
	}
}

#[test]
fn damaged_legacy_trust_is_reported() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	write_legacy(&paths, "", Some("\u{0}not toml"));
	let s = Storage::open(&paths).unwrap();
	assert!(
		s.notices
			.iter()
			.any(|n| matches!(n, Notice::TrustStoreLost { preserved: Some(_) }))
	);
}

#[test]
fn newer_legacy_version_is_refused_without_importing() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	write_legacy(&paths, "config_version = 99\n", None);
	assert!(matches!(
		Storage::open(&paths),
		Err(ConfigError::UnsupportedVersion { found: 99, .. })
	));
	assert!(paths.legacy_preferences_file().exists());
}
