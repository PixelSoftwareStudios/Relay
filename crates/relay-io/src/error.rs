//! Error type of the crate.

use std::path::PathBuf;

/// Result alias for this crate.
pub type Result<T> = std::result::Result<T, IoError>;

/// Failures of the OS I/O helpers; every variant names the path involved.
#[derive(Debug, thiserror::Error)]
pub enum IoError {
	#[error("i/o error at {path}: {source}")]
	Io {
		path: PathBuf,
		#[source]
		source: std::io::Error,
	},
	/// The path is a symlink, belongs to another user or is group/world accessible.
	#[error("{path} is not private to this user: {reason}")]
	Insecure { path: PathBuf, reason: &'static str },
	/// Another process holds the advisory lock.
	#[error("another process holds the lock at {0}")]
	Locked(PathBuf),
	#[error("{path} is larger than the {limit} byte limit")]
	TooLarge { path: PathBuf, limit: u64 },
	#[error("could not determine a platform directory for application data")]
	NoPlatformDir,
}

impl IoError {
	/// An I/O failure at `path`.
	pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
		Self::Io {
			path: path.into(),
			source,
		}
	}
}
