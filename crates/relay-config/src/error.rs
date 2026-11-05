//! Error type shared by every module of the crate.

use std::path::PathBuf;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, ConfigError>;

/// Failures raised by configuration, identity, avatar and diagnostics code.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
	#[error("i/o error at {path}: {source}")]
	Io {
		path: PathBuf,
		#[source]
		source: std::io::Error,
	},
	#[error("could not determine a platform directory for application data")]
	NoPlatformDir,
	#[error("another process holds the single-writer lock at {0}")]
	Locked(PathBuf),
	#[error("invalid TOML in {path}: {message}")]
	Parse { path: PathBuf, message: String },
	#[error("could not serialize {what}: {message}")]
	Serialize { what: &'static str, message: String },
	#[error("config version {found} is newer than supported version {supported}")]
	UnsupportedVersion { found: i64, supported: u32 },
	#[error("invalid config version value")]
	BadVersion,
	#[error("invalid preference value: {0}")]
	InvalidPreference(String),
	#[error("credential store error: {0}")]
	CredentialStore(String),
	#[error("no identity is stored")]
	IdentityMissing,
	#[error("stored identity is malformed: {0}")]
	IdentityMalformed(String),
	#[error("insecure file key storage was not explicitly enabled")]
	InsecureStorageNotEnabled,
	#[error("avatar rejected: {0}")]
	Avatar(#[from] AvatarError),
}

impl ConfigError {
	pub(crate) fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
		Self::Io {
			path: path.into(),
			source,
		}
	}
}

/// Reasons an avatar upload is refused.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum AvatarError {
	#[error("encoded avatar is {size} bytes, limit is {limit}")]
	TooLarge { size: usize, limit: usize },
	#[error("avatar is empty")]
	Empty,
	#[error("unsupported or unrecognised image format")]
	UnsupportedFormat,
	#[error("avatar dimensions {width}x{height} exceed the {limit} pixel limit")]
	DimensionsTooLarge { width: u32, height: u32, limit: u32 },
	#[error("avatar has zero width or height")]
	ZeroDimension,
	#[error("avatar would need {needed} bytes to decode, limit is {limit}")]
	DecodeBudget { needed: u64, limit: u64 },
	#[error("avatar could not be decoded: {0}")]
	Decode(String),
	#[error("avatar path is not acceptable: {0}")]
	BadPath(&'static str),
	#[error("avatar hash is not valid hex of the expected length")]
	BadHash,
}
