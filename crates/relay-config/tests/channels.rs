mod support;

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

use relay_config::channels::{ChannelData, ChannelRow, ChannelStorage, InviteMeta, IssuedRow};
use relay_config::prefs::KeyStorage;
use relay_config::vault::{FileVault, MemoryVault, SecretVault};
use relay_config::{ConfigError, Storage};
use rusqlite::Connection;
use support::storage;
use zeroize::Zeroizing;

const SIGNING: [u8; 32] = [0xA5; 32];
const PASSWORD: [u8; 32] = [0x5C; 32];

fn data(id: u8, name: &str) -> ChannelData {
	let mut secrets = BTreeMap::new();
	secrets.insert("signing".to_owned(), Zeroizing::new(SIGNING.to_vec()));
	secrets.insert("password".to_owned(), Zeroizing::new(PASSWORD.to_vec()));
	secrets.insert("invite-0102".to_owned(), Zeroizing::new(vec![0x77; 32]));
	ChannelData {
		row: ChannelRow {
			id: [id; 16],
			name: name.to_owned(),
			flags: 3,
			role: 1,
			owner_public: [9; 32],
			delegation: None,
			cert: Some(vec![1, 2, 3]),
			issued: vec![IssuedRow {
				subject: [4; 32],
				expires_at: 99,
			}],
			revocations: vec![vec![7, 7], vec![8]],
			invites: vec![InviteMeta {
				id: [1, 2, 0, 0, 0, 0, 0, 0],
				expires_at: 50,
				uses_left: 2,
			}],
		},
		secrets,
	}
}

fn open(root: &std::path::Path) -> (Storage, Arc<MemoryVault>, ChannelStorage) {
	let s = storage(root);
	let vault = Arc::new(MemoryVault::default());
	let channels = ChannelStorage::open(&s.database, vault.clone()).unwrap();
	(s, vault, channels)
}

fn raw(s: &Storage) -> Connection {
	Connection::open(s.database.path()).unwrap()
}

#[test]
fn round_trip_keeps_metadata_and_secrets() {
	let dir = tempfile::tempdir().unwrap();
	let (_s, _v, ch) = open(dir.path());
	let original = data(1, "Dorm 3");
	ch.put(&original, 16).unwrap();
	ch.put(&data(2, "Alpha"), 16).unwrap();
	let got = ch.get(&[1; 16]).unwrap().unwrap();
	assert_eq!(got.row, original.row);
	assert_eq!(got.secrets, original.secrets);
	assert_eq!(ch.get(&[3; 16]).unwrap().map(|d| d.row), None);
	let names: Vec<_> = ch.list().unwrap().into_iter().map(|d| d.row.name).collect();
	assert_eq!(names, ["Alpha", "Dorm 3"]);
}

#[test]
fn state_survives_reopen_with_a_persistent_vault() {
	let dir = tempfile::tempdir().unwrap();
	let vault = Arc::new(MemoryVault::default());
	{
		let s = storage(dir.path());
		ChannelStorage::open(&s.database, vault.clone())
			.unwrap()
			.put(&data(1, "A"), 16)
			.unwrap();
	}
	let s = storage(dir.path());
	let ch = ChannelStorage::open(&s.database, vault).unwrap();
	assert_eq!(
		ch.get(&[1; 16]).unwrap().unwrap().secrets["signing"].as_slice(),
		SIGNING
	);
}

#[test]
fn limit_applies_to_new_channels_only() {
	let dir = tempfile::tempdir().unwrap();
	let (_s, vault, ch) = open(dir.path());
	ch.put(&data(1, "A"), 1).unwrap();
	assert!(matches!(
		ch.put(&data(2, "B"), 1),
		Err(ConfigError::ChannelLimit)
	));
	ch.put(&data(1, "A renamed"), 1).unwrap();
	assert_eq!(vault.len(), 3, "rejected put left no secrets behind");
}

#[test]
fn update_is_one_atomic_change_and_supersedes_old_secrets() {
	let dir = tempfile::tempdir().unwrap();
	let (_s, vault, ch) = open(dir.path());
	ch.put(&data(1, "A"), 16).unwrap();
	assert_eq!(vault.len(), 3);
	let spent = ch
		.update(&[1; 16], &mut |d| {
			d.row.invites.clear();
			d.secrets.remove("invite-0102");
			d.secrets
				.insert("password".to_owned(), Zeroizing::new(vec![0x11; 32]));
			d.row.cert = None;
		})
		.unwrap();
	assert!(spent);
	let got = ch.get(&[1; 16]).unwrap().unwrap();
	assert!(got.row.invites.is_empty() && got.row.cert.is_none());
	assert_eq!(got.secrets.len(), 2);
	assert_eq!(got.secrets["password"].as_slice(), [0x11; 32]);
	assert_eq!(got.secrets["signing"].as_slice(), SIGNING);
	assert_eq!(
		vault.len(),
		2,
		"old generations and the spent invite are wiped"
	);
	assert!(
		!ch.update(&[9; 16], &mut |_| panic!("must not run"))
			.unwrap()
	);
}

#[test]
fn failed_commit_rolls_back_and_leaves_no_secrets() {
	let dir = tempfile::tempdir().unwrap();
	let (s, vault, ch) = open(dir.path());
	ch.put(&data(1, "A"), 16).unwrap();
	let before = ch.get(&[1; 16]).unwrap().unwrap();
	raw(&s)
		.execute_batch(
			"CREATE TRIGGER boom BEFORE INSERT ON channels BEGIN SELECT RAISE(ABORT, 'boom'); END;",
		)
		.unwrap();
	let err = ch.update(&[1; 16], &mut |d| {
		d.secrets
			.insert("password".to_owned(), Zeroizing::new(vec![0x22; 32]));
		d.row.name = "changed".into();
	});
	assert!(err.is_err());
	let after = ch.get(&[1; 16]).unwrap().unwrap();
	assert_eq!(after.row, before.row);
	assert_eq!(after.secrets, before.secrets);
	assert_eq!(vault.len(), 3, "staged secret was removed again");
	assert!(ch.put(&data(5, "New"), 16).is_err());
	assert_eq!(vault.len(), 3);
}

#[test]
fn crash_between_vault_write_and_commit_leaves_no_orphan() {
	let dir = tempfile::tempdir().unwrap();
	let vault = Arc::new(MemoryVault::default());
	{
		let s = storage(dir.path());
		let ch = ChannelStorage::open(&s.database, vault.clone()).unwrap();
		ch.put(&data(1, "A"), 16).unwrap();
		// A write that died after recording its intent and writing the vault entry,
		// but before the metadata transaction committed.
		raw(&s)
			.execute(
				"INSERT INTO channel_secrets (channel, slot, gen, state)
				 VALUES (?1, 'password', 2, 'pending')",
				[[1u8; 16].as_slice()],
			)
			.unwrap();
		vault.put(&[1; 16], "password.2", &[0xEE; 32]).unwrap();
		// And one that died before even the intent was recorded is invisible by design:
		// nothing in the vault is written without a prior pending row.
		assert_eq!(vault.len(), 4);
	}
	let s = storage(dir.path());
	let ch = ChannelStorage::open(&s.database, vault.clone()).unwrap();
	assert_eq!(vault.len(), 3);
	let got = ch.get(&[1; 16]).unwrap().unwrap();
	assert_eq!(
		got.secrets["password"].as_slice(),
		PASSWORD,
		"committed secret intact"
	);
	assert!(vault.get(&[1; 16], "password.2").unwrap().is_none());
}

#[test]
fn crash_after_commit_before_cleanup_is_finished_at_open() {
	let dir = tempfile::tempdir().unwrap();
	let vault = Arc::new(MemoryVault::default());
	{
		let s = storage(dir.path());
		let ch = ChannelStorage::open(&s.database, vault.clone()).unwrap();
		ch.put(&data(1, "A"), 16).unwrap();
		// Removal committed; the process died before the vault was wiped.
		raw(&s)
			.execute("UPDATE channel_secrets SET state = 'dead'", [])
			.unwrap();
		raw(&s).execute("DELETE FROM channels", []).unwrap();
	}
	let s = storage(dir.path());
	let ch = ChannelStorage::open(&s.database, vault.clone()).unwrap();
	assert!(vault.is_empty());
	assert!(ch.list().unwrap().is_empty());
}

struct FlakyVault {
	inner: MemoryVault,
	puts_left: AtomicUsize,
}

impl SecretVault for FlakyVault {
	fn describe(&self) -> &'static str {
		"flaky"
	}
	fn get(&self, c: &[u8; 16], s: &str) -> relay_config::Result<Option<Zeroizing<Vec<u8>>>> {
		self.inner.get(c, s)
	}
	fn put(&self, c: &[u8; 16], s: &str, v: &[u8]) -> relay_config::Result<()> {
		if self
			.puts_left
			.try_update(Ordering::SeqCst, Ordering::SeqCst, |n| n.checked_sub(1))
			.is_err()
		{
			return Err(ConfigError::CredentialStore("locked".into()));
		}
		self.inner.put(c, s, v)
	}
	fn delete(&self, c: &[u8; 16], s: &str) -> relay_config::Result<()> {
		self.inner.delete(c, s)
	}
}

#[test]
fn vault_failure_midway_leaves_nothing_behind() {
	let dir = tempfile::tempdir().unwrap();
	let s = storage(dir.path());
	let vault = Arc::new(FlakyVault {
		inner: MemoryVault::default(),
		puts_left: AtomicUsize::new(2),
	});
	let ch = ChannelStorage::open(&s.database, vault.clone()).unwrap();
	assert!(ch.put(&data(1, "A"), 16).is_err());
	assert!(vault.inner.is_empty());
	assert!(ch.list().unwrap().is_empty());
	let rows: i64 = raw(&s)
		.query_row("SELECT COUNT(*) FROM channel_secrets", [], |r| r.get(0))
		.unwrap();
	assert_eq!(rows, 0);
}

#[test]
fn removal_wipes_metadata_and_every_secret() {
	let dir = tempfile::tempdir().unwrap();
	let (s, vault, ch) = open(dir.path());
	ch.put(&data(1, "A"), 16).unwrap();
	ch.put(&data(2, "B"), 16).unwrap();
	assert!(ch.remove(&[1; 16]).unwrap());
	assert!(!ch.remove(&[1; 16]).unwrap());
	assert_eq!(vault.len(), 3);
	assert!(ch.get(&[2; 16]).unwrap().is_some());
	ch.remove(&[2; 16]).unwrap();
	assert!(vault.is_empty());
	let conn = raw(&s);
	for table in [
		"channels",
		"channel_issued",
		"channel_revocations",
		"channel_invites",
		"channel_secrets",
	] {
		let n: i64 = conn
			.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
			.unwrap();
		assert_eq!(n, 0, "{table}");
	}
}

#[test]
fn two_users_have_separate_channels_and_secrets() {
	let a_home = tempfile::tempdir().unwrap();
	let b_home = tempfile::tempdir().unwrap();
	let open_user = |home: &std::path::Path| {
		let s = storage(home);
		let vault = FileVault::new(home.join("secrets"), KeyStorage::InsecureFile).unwrap();
		let ch = ChannelStorage::open(&s.database, Arc::new(vault)).unwrap();
		(s, ch)
	};
	let (_sa, a) = open_user(a_home.path());
	let (_sb, b) = open_user(b_home.path());
	a.put(&data(1, "Alice only"), 16).unwrap();
	assert!(b.list().unwrap().is_empty());
	assert!(b.get(&[1; 16]).unwrap().is_none());
	assert!(
		std::fs::read_dir(b_home.path().join("secrets")).map_or(true, |mut d| d.next().is_none())
	);
	assert!(
		std::fs::read_dir(a_home.path().join("secrets"))
			.unwrap()
			.count() >= 3
	);
	#[cfg(unix)]
	{
		use std::os::unix::fs::PermissionsExt;
		let secrets = a_home.path().join("secrets");
		assert_eq!(
			std::fs::metadata(&secrets).unwrap().permissions().mode() & 0o777,
			0o700
		);
		for entry in std::fs::read_dir(secrets).unwrap() {
			let mode = entry.unwrap().metadata().unwrap().permissions().mode() & 0o777;
			assert_eq!(mode, 0o600);
		}
	}
}

#[test]
fn file_vault_needs_the_explicit_insecure_choice() {
	let dir = tempfile::tempdir().unwrap();
	assert!(matches!(
		FileVault::new(dir.path(), KeyStorage::OsCredentialStore),
		Err(ConfigError::InsecureStorageNotEnabled)
	));
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
	haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn no_secret_material_reaches_the_database_files_or_the_bundle() {
	let dir = tempfile::tempdir().unwrap();
	let (s, _v, ch) = open(dir.path());
	ch.put(&data(1, "A"), 16).unwrap();
	ch.update(&[1; 16], &mut |d| d.row.name = "B".into())
		.unwrap();
	s.database.refresh_backup(true);
	let db = s.database.path().to_path_buf();
	let mut files = vec![db.clone()];
	for suffix in ["-wal", "-shm", ".bak"] {
		let mut p = db.clone().into_os_string();
		p.push(suffix);
		files.push(p.into());
	}
	let mut checked = 0;
	for file in files.iter().filter(|f| f.exists()) {
		let bytes = std::fs::read(file).unwrap();
		for secret in [
			SIGNING.as_slice(),
			PASSWORD.as_slice(),
			[0x77u8; 32].as_slice(),
		] {
			assert!(
				!contains(&bytes, secret),
				"secret found in {}",
				file.display()
			);
		}
		checked += 1;
	}
	assert!(checked >= 2);
	let bundle = relay_config::diagnostics::build_bundle(&relay_config::diagnostics::BundleInput {
		app_version: "0",
		os: "test",
		preferences: &relay_config::prefs::Preferences::default(),
		notices: &[],
		counters: &BTreeMap::new(),
		log_lines: &[],
	});
	assert!(!bundle.contains("A5A5") && !bundle.to_lowercase().contains("a5a5a5"));
	assert!(!bundle.contains(&db.display().to_string()));
}
