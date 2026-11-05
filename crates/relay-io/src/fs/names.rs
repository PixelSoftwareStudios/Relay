//! Sibling file names (`.bak`, `.corrupt`, sidecars).

use std::path::{Path, PathBuf};

/// `path` with `suffix` appended to its file name.
pub fn with_suffix(path: &Path, suffix: &str) -> PathBuf {
	let mut name = path.as_os_str().to_owned();
	name.push(suffix);
	PathBuf::from(name)
}

/// Location of the previous good copy of `path`.
pub fn backup_path(path: &Path) -> PathBuf {
	with_suffix(path, ".bak")
}

/// Where an unreadable `path` is preserved for the user to inspect.
pub fn corrupt_path(path: &Path) -> PathBuf {
	with_suffix(path, ".corrupt")
}
