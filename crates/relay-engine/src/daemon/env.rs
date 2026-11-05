//! Everything loaded from disk before the service starts.

use std::sync::Arc;

use relay_config::avatar::AvatarCache;
use relay_config::channels::ChannelStorage;
use relay_config::WriterLock;
use relay_config::identity::{IdentityStore, Secret32, StoredIdentity, TrustStore, open_store};
use relay_config::prefs::{PreferenceStore, Preferences};
use relay_config::vault::{SecretVault, open_vault};
use relay_config::{Notice, Paths, Storage};
use relay_core::DeviceKeyId;
use relay_net::channel::ChannelStore;
use relay_net::identity::{Identity, PublicKey};

use super::SqliteChannelStore;
use super::error::DaemonError;
use super::settings::FileSettings;
use crate::peers::{ConfigTrust, SharedTrust};
use crate::service::{ControlHandle, GateReader};

/// Process-wide facts and stores handed to the adapter factory.
pub struct Environment {
	/// Resolved directories.
	pub paths: Paths,
	/// The one writer lock of this process (already held by the stores below).
	pub lock: Arc<WriterLock>,
	/// Preferences as loaded.
	pub preferences: Preferences,
	/// Recovery notices raised while loading; clients are shown these.
	pub notices: Vec<Notice>,
	/// Digest of the local static public key.
	pub local: DeviceKeyId,
	/// The local identity (private half included) for the secure transport.
	pub identity: StoredIdentity,
	/// Where the identity lives, for display.
	pub identity_storage: &'static str,
	/// The local identity as the secure transport uses it.
	pub transport_identity: Arc<Identity>,
	/// Persistent trust store sharing the writer lock.
	pub trust: SharedTrust,
	/// The engine incarnation clients see; peers and media echo it.
	pub incarnation: [u8; 16],
	/// Preferences and avatar persistence.
	pub settings: Arc<FileSettings>,
	/// Avatar cache (shared with peers for received avatars).
	pub avatars: Arc<AvatarCache>,
	/// Channel records (metadata in SQLite, secrets in the vault).
	pub channels: Arc<dyn ChannelStore>,
	/// Handle adapters use to report events to the control actor.
	pub handle: ControlHandle,
	/// Lock-free transmit gates for the media thread.
	pub gates: GateReader,
}

/// Disk state, loaded before the service exists.
pub(super) struct Loaded {
	pub paths: Paths,
	pub lock: Arc<WriterLock>,
	pub preferences: Preferences,
	pub notices: Vec<Notice>,
	pub local: DeviceKeyId,
	pub identity: StoredIdentity,
	pub identity_storage: &'static str,
	pub transport_identity: Arc<Identity>,
	pub trust: SharedTrust,
	pub settings: Arc<FileSettings>,
	pub avatars: Arc<AvatarCache>,
	pub channels: Arc<dyn ChannelStore>,
}

fn generate_identity() -> Result<StoredIdentity, DaemonError> {
	let identity = Identity::generate().map_err(|e| DaemonError::Identity(e.to_string()))?;
	Ok(StoredIdentity {
		secret: Secret32::new(*identity.secret_bytes()),
		public: *identity.public().as_bytes(),
	})
}

pub(super) fn load(
	paths: Paths,
	identity_store: Option<Box<dyn IdentityStore>>,
	channel_vault: Option<Arc<dyn SecretVault>>,
) -> Result<Loaded, DaemonError> {
	let storage = Storage::open(&paths)?;
	let lock = Arc::clone(&storage.lock);
	let prefs_store = PreferenceStore::open(&storage.database)?;
	let loaded = prefs_store.load()?;
	let mut notices = storage.notices.clone();
	notices.extend(loaded.notices);
	let preferences = loaded.preferences;

	let store = match identity_store {
		Some(store) => store,
		None => open_store(&preferences, &paths)?,
	};
	let identity = match store.load()? {
		Some(existing) => existing,
		None => {
			let fresh = generate_identity()?;
			store.save(&fresh)?;
			fresh
		}
	};
	let local = DeviceKeyId::from_bytes(
		*PublicKey::from_bytes(identity.public)
			.fingerprint()
			.as_bytes(),
	);

	let trust = SharedTrust::new(ConfigTrust::new(TrustStore::open(&storage.database)?));
	let transport_identity = Arc::new(
		Identity::from_secret_bytes(*identity.secret.expose())
			.map_err(|e| DaemonError::Identity(e.to_string()))?,
	);

	let vault = match channel_vault {
		Some(vault) => vault,
		None => open_vault(&preferences, &paths)?,
	};
	let channels: Arc<dyn ChannelStore> = Arc::new(SqliteChannelStore::new(ChannelStorage::open(
		&storage.database,
		vault,
	)?));
	let avatars = Arc::new(AvatarCache::new(&storage.database));
	let settings = Arc::new(FileSettings::new(
		prefs_store,
		AvatarCache::new(&storage.database),
		preferences.clone(),
	));
	Ok(Loaded {
		identity_storage: store.describe(),
		paths,
		lock,
		preferences,
		notices,
		local,
		identity,
		transport_identity,
		trust,
		settings,
		avatars,
		channels,
	})
}
