//! `reset_local_data`: the database and its sidecars go, everything else stays, and a live
//! writer lock refuses the reset. Private temp directories only.

use std::fs;
use std::path::Path;

use relay_client::reset::{ResetError, reset_local_data};
use relay_io::lock::ProcessLock;

struct Dirs {
	_root: tempfile::TempDir,
	config: std::path::PathBuf,
	data: std::path::PathBuf,
}

fn dirs() -> Dirs {
	let root = tempfile::tempdir().expect("tempdir");
	let config = root.path().join("config");
	let data = root.path().join("data");
	fs::create_dir_all(&config).unwrap();
	fs::create_dir_all(&data).unwrap();
	Dirs {
		_root: root,
		config,
		data,
	}
}

fn touch(dir: &Path, name: &str) {
	fs::write(dir.join(name), b"x").unwrap();
}

const GONE: [&str; 10] = [
	"relay.db",
	"relay.db-wal",
	"relay.db-shm",
	"relay.db-journal",
	"relay.db.bak",
	"relay.db.bak.tmp",
	"relay.db.corrupt",
	"avatars.db",
	"avatars.db-wal",
	"avatars.db-shm",
];

const KEPT: [&str; 5] = [
	"identity.insecure.key",
	"identity.key",
	"notes.txt",
	"relay.dbx",
	"my-relay.db",
];

#[test]
fn removes_the_database_the_avatar_cache_and_their_sidecars() {
	let d = dirs();
	for name in GONE.iter().chain(KEPT.iter()) {
		touch(&d.data, name);
	}
	let removed = reset_local_data(&d.config, &d.data).expect("reset");
	assert_eq!(removed.len(), GONE.len());
	for name in GONE {
		assert!(!d.data.join(name).exists(), "{name} should be gone");
	}
	for name in KEPT {
		assert!(d.data.join(name).exists(), "{name} must stay");
	}
}

#[test]
fn keeps_directories_such_as_the_ringtone_and_channel_secrets() {
	let d = dirs();
	touch(&d.data, "relay.db");
	fs::create_dir(d.data.join("ringtone")).unwrap();
	touch(&d.data.join("ringtone"), "ring.wav");
	fs::create_dir(d.data.join("relay.db.d")).unwrap();
	reset_local_data(&d.config, &d.data).expect("reset");
	assert!(d.data.join("ringtone/ring.wav").exists());
	assert!(d.data.join("relay.db.d").is_dir());
	assert!(!d.data.join("relay.db").exists());
}

#[test]
fn is_idempotent_and_accepts_empty_or_missing_directories() {
	let d = dirs();
	assert!(reset_local_data(&d.config, &d.data).unwrap().is_empty());
	touch(&d.data, "relay.db");
	assert_eq!(reset_local_data(&d.config, &d.data).unwrap().len(), 1);
	assert!(reset_local_data(&d.config, &d.data).unwrap().is_empty());
	let nowhere = d.data.join("missing");
	assert!(
		reset_local_data(&d.config.join("none"), &nowhere)
			.unwrap()
			.is_empty()
	);
}

#[test]
fn refuses_while_a_daemon_holds_the_writer_lock() {
	let d = dirs();
	touch(&d.data, "relay.db");
	touch(&d.data, "avatars.db");
	let held = ProcessLock::acquire(&d.config.join("writer.lock")).expect("lock");
	let error = reset_local_data(&d.config, &d.data).expect_err("must refuse");
	assert!(matches!(error, ResetError::DaemonRunning), "{error:?}");
	assert!(d.data.join("relay.db").exists() && d.data.join("avatars.db").exists());

	drop(held);
	assert_eq!(reset_local_data(&d.config, &d.data).unwrap().len(), 2);
}

#[test]
fn a_stale_lock_file_without_a_holder_does_not_block() {
	let d = dirs();
	touch(&d.config, "writer.lock");
	touch(&d.data, "relay.db");
	assert_eq!(reset_local_data(&d.config, &d.data).unwrap().len(), 1);
	// The lock file itself is not Relay's data and stays.
	assert!(d.config.join("writer.lock").exists());
}

#[cfg(unix)]
#[test]
fn reports_what_could_not_be_removed() {
	use std::os::unix::fs::PermissionsExt;
	let d = dirs();
	touch(&d.data, "relay.db");
	touch(&d.data, "avatars.db");
	fs::set_permissions(&d.data, fs::Permissions::from_mode(0o500)).unwrap();
	let result = reset_local_data(&d.config, &d.data);
	fs::set_permissions(&d.data, fs::Permissions::from_mode(0o700)).unwrap();
	if result.is_ok() {
		// Privileges that ignore the directory mode (root): nothing to assert.
		return;
	}
	match result {
		Err(ResetError::Incomplete { removed, failed }) => {
			assert!(removed.is_empty());
			assert_eq!(failed.len(), 2);
		}
		other => panic!("expected Incomplete, got {other:?}"),
	}
}
