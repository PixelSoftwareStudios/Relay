use std::sync::Arc;

use relay_config::channels::ChannelStorage;
use relay_config::prefs::KeyStorage;
use relay_config::vault::FileVault;
use relay_config::{Paths, Storage};
use relay_engine::daemon::SqliteChannelStore;
use relay_net::channel::{ChannelStore, Policy, Role, StoreError, new_channel};
use relay_net::identity::Fingerprint;

const NOW: u64 = 1_000_000;

fn local() -> Fingerprint {
	Fingerprint::from_bytes([7; 32])
}

fn open(root: &std::path::Path) -> (Storage, SqliteChannelStore) {
	let paths = Paths::under(root);
	let storage = Storage::open(&paths).unwrap();
	let vault = FileVault::new(root.join("secrets"), KeyStorage::InsecureFile).unwrap();
	let channels = ChannelStorage::open(&storage.database, Arc::new(vault)).unwrap();
	(storage, SqliteChannelStore::new(channels))
}

fn rich_record() -> relay_net::channel::ChannelRecord {
	let policy = Policy {
		listed: false,
		password: true,
		invite_only: true,
	};
	let mut record = new_channel("Dorm 3", policy, Some("correct horse"), &local(), NOW).unwrap();
	record.new_invite(&local(), NOW, 3600, 2).expect("invite");
	let member = Fingerprint::from_bytes([9; 32]);
	record.admit(&member, NOW).expect("admit");
	let revoked = record.authority.as_ref().unwrap().revoke(
		&record.id,
		&Fingerprint::from_bytes([5; 32]),
		NOW + 999,
	);
	record.add_revocation(revoked);
	record
}

#[test]
fn full_record_round_trips_through_sqlite_and_the_vault() {
	let dir = tempfile::tempdir().unwrap();
	let record = rich_record();
	{
		let (_s, store) = open(dir.path());
		store.put(record.clone()).unwrap();
	}
	let (_s, store) = open(dir.path());
	let got = store.get(&record.id).unwrap().expect("stored");
	assert_eq!(got.name.as_str(), "Dorm 3");
	assert_eq!(got.role, Role::Owner);
	assert_eq!(got.policy, record.policy);
	assert_eq!(got.owner_public, record.owner_public);
	assert_eq!(
		got.authority.as_ref().unwrap().seed(),
		record.authority.as_ref().unwrap().seed()
	);
	assert_eq!(
		*got.password.as_ref().unwrap().to_bytes(),
		*record.password.as_ref().unwrap().to_bytes()
	);
	assert_eq!(got.cert, record.cert);
	assert_eq!(got.issued, record.issued);
	assert_eq!(got.revocations, record.revocations);
	assert_eq!(got.invites.len(), 1);
	assert_eq!(got.invites[0].id, record.invites[0].id);
	assert_eq!(
		*got.invites[0].credential_bytes(),
		*record.invites[0].credential_bytes()
	);
	assert_eq!(store.list().unwrap().len(), 1);
}

#[test]
fn update_spends_a_single_use_invite_once_and_persists_it() {
	let dir = tempfile::tempdir().unwrap();
	let (_s, store) = open(dir.path());
	let mut record = rich_record();
	record.invites[0].uses_left = 1;
	store.put(record.clone()).unwrap();
	store
		.update(&record.id, &mut |r| {
			r.invites[0].uses_left -= 1;
			r.invites.retain(|i| i.uses_left > 0);
		})
		.unwrap();
	assert!(store.get(&record.id).unwrap().unwrap().invites.is_empty());
	let vault_files = std::fs::read_dir(dir.path().join("secrets"))
		.unwrap()
		.count();
	assert_eq!(
		vault_files, 2,
		"signing key and password remain; invite wiped"
	);
	assert_eq!(
		store.update(
			&relay_net::channel::ChannelId::from_bytes([0; 16]),
			&mut |_| {}
		),
		Err(StoreError::NotFound)
	);
}

#[test]
fn remove_wipes_secrets_and_no_secret_is_in_the_database() {
	let dir = tempfile::tempdir().unwrap();
	let (storage, store) = open(dir.path());
	let record = rich_record();
	store.put(record.clone()).unwrap();
	let seed = *record.authority.as_ref().unwrap().seed();
	let password = *record.password.as_ref().unwrap().to_bytes();
	let invite = *record.invites[0].credential_bytes();
	storage.database.refresh_backup(true);
	let db = storage.database.path().to_path_buf();
	for suffix in ["", "-wal", ".bak"] {
		let mut name = db.clone().into_os_string();
		name.push(suffix);
		let Ok(bytes) = std::fs::read(&name) else {
			continue;
		};
		for secret in [seed, password, invite] {
			assert!(
				!bytes.windows(32).any(|w| w == secret),
				"secret in {suffix:?}"
			);
		}
	}
	assert!(store.remove(&record.id).unwrap());
	assert!(!store.remove(&record.id).unwrap());
	assert_eq!(
		std::fs::read_dir(dir.path().join("secrets"))
			.unwrap()
			.count(),
		0
	);
	assert!(store.list().unwrap().is_empty());
}

#[test]
fn channel_count_is_bounded() {
	let dir = tempfile::tempdir().unwrap();
	let (_s, store) = open(dir.path());
	let open_policy = Policy::default();
	for n in 0..relay_core::channel::CHANNELS_MAX {
		let record = new_channel(&format!("c{n}"), open_policy, None, &local(), NOW).unwrap();
		store.put(record).unwrap();
	}
	let extra = new_channel("extra", open_policy, None, &local(), NOW).unwrap();
	assert_eq!(store.put(extra), Err(StoreError::Full));
}
