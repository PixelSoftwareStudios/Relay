//! Everything loaded from disk before the service starts.

use std::sync::{Arc, Mutex};

use relay_config::avatar::AvatarCache;
use relay_config::fsx::WriterLock;
use relay_config::identity::{IdentityStore, Secret32, StoredIdentity, TrustStore, open_store};
use relay_config::prefs::{PreferenceStore, Preferences};
use relay_config::{Notice, Paths};
use relay_core::DeviceKeyId;
use relay_net::identity::{Identity, PublicKey};

use super::error::DaemonError;
use super::settings::FileSettings;
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
	/// Persistent trust store sharing the writer lock.
	pub trust: Arc<Mutex<TrustStore>>,
	/// Preferences and avatar persistence.
	pub settings: Arc<FileSettings>,
	/// Avatar cache (shared with peers for received avatars).
	pub avatars: Arc<AvatarCache>,
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
	pub trust: Arc<Mutex<TrustStore>>,
	pub settings: Arc<FileSettings>,
	pub avatars: Arc<AvatarCache>,
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
) -> Result<Loaded, DaemonError> {
	let lock = Arc::new(WriterLock::acquire(&paths.writer_lock_file())?);
	let prefs_store = PreferenceStore::open(paths.preferences_file(), Arc::clone(&lock))?;
	let loaded = prefs_store.load()?;
	let mut notices = loaded.notices;
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

	let (trust, trust_notices) = TrustStore::open(paths.trust_file(), Arc::clone(&lock))?;
	notices.extend(trust_notices);

	let avatars = Arc::new(AvatarCache::open(paths.avatar_cache_dir())?);
	let settings = Arc::new(FileSettings::new(
		prefs_store,
		AvatarCache::open(paths.avatar_cache_dir())?,
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
		trust: Arc::new(Mutex::new(trust)),
		settings,
		avatars,
	})
}
