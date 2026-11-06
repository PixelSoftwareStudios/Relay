//! Platform directory resolution and the per-file layout under it.
//!
//! `UserDirs` (relay-io) resolves inside the current OS user's profile, so every OS user of a
//! shared PC gets a separate database.

use std::path::{Path, PathBuf};

use relay_io::paths::UserDirs;

use crate::error::Result;

/// Every on-disk location the crate uses, rooted at one config and one data directory.
#[derive(Debug, Clone)]
pub struct Paths {
	config_dir: PathBuf,
	data_dir: PathBuf,
}

impl Paths {
	/// Resolves the platform default directories.
	pub fn platform() -> Result<Self> {
		let dirs = UserDirs::platform()?;
		Ok(Self::new(dirs.config_dir(), dirs.data_dir()))
	}

	/// Uses explicit directories (portable installs and tests).
	pub fn new(config_dir: impl AsRef<Path>, data_dir: impl AsRef<Path>) -> Self {
		Self {
			config_dir: config_dir.as_ref().to_path_buf(),
			data_dir: data_dir.as_ref().to_path_buf(),
		}
	}

	/// Roots both directories under one folder.
	pub fn under(root: impl AsRef<Path>) -> Self {
		let root = root.as_ref();
		Self::new(root.join("config"), root.join("data"))
	}

	pub fn config_dir(&self) -> &Path {
		&self.config_dir
	}

	pub fn data_dir(&self) -> &Path {
		&self.data_dir
	}

	/// The per-user SQLite database holding preferences, trust and channel metadata.
	pub fn database_file(&self) -> PathBuf {
		self.data_dir.join("relay.db")
	}

	/// The disposable avatar cache; never backed up, safe to delete.
	pub fn avatar_cache_file(&self) -> PathBuf {
		self.data_dir.join("avatars.db")
	}

	pub fn identity_file(&self) -> PathBuf {
		self.data_dir.join("identity.insecure.key")
	}

	/// Used only when the user chose the INSECURE file option for secrets.
	pub fn channel_secrets_dir(&self) -> PathBuf {
		self.data_dir.join("channel-secrets")
	}

	/// Serialises read-modify-write of the single credential-store item across processes.
	pub fn keychain_lock_file(&self) -> PathBuf {
		self.config_dir.join("keychain.lock")
	}

	pub fn writer_lock_file(&self) -> PathBuf {
		self.config_dir.join("writer.lock")
	}
}
