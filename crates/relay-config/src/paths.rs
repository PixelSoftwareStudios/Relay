//! Platform directory resolution and the per-file layout under it.
//!
//! `ProjectDirs` resolves inside the current OS user's profile, so every OS user of a
//! shared PC gets a separate database.

use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::error::{ConfigError, Result};

const QUALIFIER: &str = "";
const ORGANIZATION: &str = "Relay";
const APPLICATION: &str = "Relay";

/// Every on-disk location the crate uses, rooted at one config and one data directory.
#[derive(Debug, Clone)]
pub struct Paths {
	config_dir: PathBuf,
	data_dir: PathBuf,
}

impl Paths {
	/// Resolves the platform default directories.
	pub fn platform() -> Result<Self> {
		let dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
			.ok_or(ConfigError::NoPlatformDir)?;
		Ok(Self::new(dirs.config_dir(), dirs.data_local_dir()))
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

	/// The per-user SQLite database holding preferences, trust and the avatar cache.
	pub fn database_file(&self) -> PathBuf {
		self.data_dir.join("relay.db")
	}

	pub fn identity_file(&self) -> PathBuf {
		self.data_dir.join("identity.insecure.key")
	}

	/// Used only when the user chose the INSECURE file option for secrets.
	pub fn channel_secrets_dir(&self) -> PathBuf {
		self.data_dir.join("channel-secrets")
	}

	pub fn writer_lock_file(&self) -> PathBuf {
		self.config_dir.join("writer.lock")
	}
}
