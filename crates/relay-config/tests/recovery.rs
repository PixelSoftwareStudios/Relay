mod support;

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use relay_config::prefs::{PreferenceStore, TransmitModePref};
use relay_config::{ConfigError, Notice, Paths, Storage};
use rusqlite::Connection;
use support::{chosen_mic, storage};

fn sidecar(db: &Path, suffix: &str) -> PathBuf {
	let mut name = db.as_os_str().to_owned();
	name.push(suffix);
	PathBuf::from(name)
}

fn seed(root: &Path) -> Paths {
	let s = storage(root);
	PreferenceStore::open(&s.database)
		.unwrap()
		.save(&chosen_mic())
		.unwrap();
	s.database.refresh_backup(true);
	Paths::under(root)
}

fn assert_mic_cleared(s: &Storage) {
	let prefs = PreferenceStore::open(&s.database)
		.unwrap()
		.load()
		.unwrap()
		.preferences;
	assert!(prefs.audio.preferred_input_id.is_empty());
	assert_eq!(prefs.calls.default_transmit_mode, TransmitModePref::Muted);
}

#[test]
fn garbage_main_recovers_backup_without_enabling_mic() {
	let dir = tempfile::tempdir().unwrap();
	let paths = seed(dir.path());
	fs::write(paths.database_file(), vec![0xAB; 8192]).unwrap();
	let s = Storage::open(&paths).unwrap();
	assert_eq!(
		s.notices,
		vec![
			Notice::PreferencesRecoveredFromBackup,
			Notice::TrustRecoveredFromBackup
		]
	);
	assert_mic_cleared(&s);
	assert!(sidecar(&paths.database_file(), ".corrupt").exists());
}

#[test]
fn truncated_main_is_detected_and_recovered() {
	let dir = tempfile::tempdir().unwrap();
	let paths = seed(dir.path());
	let size = fs::metadata(paths.database_file()).unwrap().len();
	OpenOptions::new()
		.write(true)
		.open(paths.database_file())
		.unwrap()
		.set_len(size / 2 + 100)
		.unwrap();
	let _ = fs::remove_file(sidecar(&paths.database_file(), "-wal"));
	let s = Storage::open(&paths).unwrap();
	assert!(
		s.notices.contains(&Notice::PreferencesRecoveredFromBackup),
		"{:?}",
		s.notices
	);
	assert_mic_cleared(&s);
}

#[test]
fn emptied_main_with_backup_is_not_a_first_run() {
	let dir = tempfile::tempdir().unwrap();
	let paths = seed(dir.path());
	fs::write(paths.database_file(), b"").unwrap();
	let s = Storage::open(&paths).unwrap();
	assert!(s.notices.contains(&Notice::TrustRecoveredFromBackup));
}

#[test]
fn missing_main_with_backup_recovers() {
	let dir = tempfile::tempdir().unwrap();
	let paths = seed(dir.path());
	fs::remove_file(paths.database_file()).unwrap();
	let s = Storage::open(&paths).unwrap();
	assert_eq!(s.notices.len(), 2);
	assert_mic_cleared(&s);
}

#[test]
fn corrupt_main_and_backup_resets_loudly_and_preserves_file() {
	let dir = tempfile::tempdir().unwrap();
	let paths = Paths::under(dir.path());
	fs::create_dir_all(paths.data_dir()).unwrap();
	fs::write(paths.database_file(), b"garbage garbage garbage").unwrap();
	fs::write(sidecar(&paths.database_file(), ".bak"), b"also garbage").unwrap();
	let s = Storage::open(&paths).unwrap();
	match s.notices.as_slice() {
		[
			Notice::PreferencesReset { preserved: Some(p) },
			Notice::TrustStoreLost { preserved: Some(_) },
		] => assert_eq!(fs::read(p).unwrap(), b"garbage garbage garbage"),
		other => panic!("unexpected notices {other:?}"),
	}
	assert!(
		PreferenceStore::open(&s.database)
			.unwrap()
			.load()
			.unwrap()
			.notices
			.is_empty()
	);
}

#[test]
fn newer_schema_is_refused_and_left_untouched() {
	let dir = tempfile::tempdir().unwrap();
	let paths = seed(dir.path());
	{
		let conn = Connection::open(paths.database_file()).unwrap();
		conn.pragma_update(None, "user_version", 99).unwrap();
	}
	let before = fs::read(paths.database_file()).unwrap();
	assert!(matches!(
		Storage::open(&paths),
		Err(ConfigError::UnsupportedVersion { found: 99, .. })
	));
	assert_eq!(fs::read(paths.database_file()).unwrap(), before);
	assert!(!sidecar(&paths.database_file(), ".corrupt").exists());
}

#[test]
fn uncommitted_transaction_is_invisible_after_restart() {
	let dir = tempfile::tempdir().unwrap();
	let paths = seed(dir.path());
	{
		// A writer that dies before commit: dropping rolls the transaction back.
		let mut conn = Connection::open(paths.database_file()).unwrap();
		let tx = conn.transaction().unwrap();
		tx.execute("DELETE FROM prefs", []).unwrap();
		tx.execute(
			"INSERT INTO prefs VALUES ('audio.microphone_gain', '4.0')",
			[],
		)
		.unwrap();
	}
	let s = Storage::open(&paths).unwrap();
	assert!(s.notices.is_empty(), "{:?}", s.notices);
	let prefs = PreferenceStore::open(&s.database).unwrap().load().unwrap();
	assert_eq!(prefs.preferences, chosen_mic());
}

#[test]
fn garbage_wal_sidecar_never_silently_keeps_a_recovered_mic() {
	let dir = tempfile::tempdir().unwrap();
	let paths = seed(dir.path());
	let wal = sidecar(&paths.database_file(), "-wal");
	let mut f = OpenOptions::new()
		.create(true)
		.append(true)
		.open(wal)
		.unwrap();
	f.write_all(&[0x5A; 4096]).unwrap();
	drop(f);
	// SQLite either ignores the invalid log (committed state, no notice) or
	// recovery runs and must then clear the microphone with a notice.
	let s = Storage::open(&paths).unwrap();
	if s.notices.is_empty() {
		let prefs = PreferenceStore::open(&s.database).unwrap().load().unwrap();
		assert_eq!(prefs.preferences, chosen_mic());
	} else {
		assert!(s.notices.contains(&Notice::PreferencesRecoveredFromBackup));
		assert_mic_cleared(&s);
	}
}
