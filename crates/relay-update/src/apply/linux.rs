//! Linux AppImage: the verified download already sits in the same directory as `$APPIMAGE`
//! (same filesystem), so installing is one `rename(2)`, atomic for anyone starting the file.

use std::fs;
use std::path::Path;

use semver::Version;

use super::Relaunch;
use crate::error::{Error, Result};

/// Gives `payload` the old file's permissions (plus owner execute) and renames it over `target`.
pub fn swap(payload: &Path, target: &Path) -> Result<()> {
	use std::os::unix::fs::PermissionsExt;
	let old = fs::metadata(target).map_err(Error::io("read the current AppImage"))?;
	let mode = (old.permissions().mode() & 0o777) | 0o100;
	fs::set_permissions(payload, fs::Permissions::from_mode(mode))
		.map_err(Error::io("set permissions on the new AppImage"))?;
	fs::rename(payload, target).map_err(Error::io("replace the AppImage"))
}

pub fn apply(target: &Path, payload: &Path, version: &Version) -> Result<Relaunch> {
	swap(payload, target)?;
	// The new image is in place; if no helper can start, the user starts it by hand.
	crate::helper::relaunch_exec(std::process::id(), target)?;
	Ok(Relaunch {
		version: version.clone(),
		launch: target.to_path_buf(),
	})
}
