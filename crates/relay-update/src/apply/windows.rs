//! Windows. UNVERIFIED on a real Windows machine: the path, archive and rename logic is tested on
//! the build host; the PowerShell relaunch, `msiexec` and the elevation prompt are not.
//!
//! * MSI (`WindowsMsi`, per-machine): after this process exits a hidden PowerShell runs
//!   `msiexec /i <msi> /passive /norestart` elevated (one UAC prompt) and starts Relay again.
//!   The package is `MajorUpgrade` with the fixed UpgradeCode of `packaging/windows.wxs`.
//! * Portable (`WindowsPortable`): the executables are extracted next to the running ones; each
//!   running `.exe` is renamed aside (Windows allows renaming a running image, not overwriting
//!   it) and the new file takes its name.

use std::fs;
use std::path::{Path, PathBuf};

use super::random_suffix;
use crate::error::{Error, Result};
use crate::zipx::{self, Limits};

/// Files of the portable archive's `bin/` that are replaced. Anything else in there is refused.
pub const PROGRAMS: [&str; 3] = ["relay-desktop.exe", "relay.exe", "relayd.exe"];

/// The top-level directory of `relay-<v>-windows-<arch>-portable.zip`.
pub fn portable_top(asset_name: &str) -> Option<&str> {
	asset_name
		.strip_suffix("-portable.zip")
		.filter(|top| !top.is_empty())
}

/// Extracts the archive below `staging` and returns its `bin` directory, checked to hold exactly
/// the expected programs (and `relay-desktop.exe` for certain).
pub fn prepare_portable(zip: &Path, staging: &Path, asset_name: &str) -> Result<PathBuf> {
	let top = portable_top(asset_name)
		.ok_or_else(|| Error::BundleInvalid("unexpected asset name".into()))?;
	let target = staging.join("extracted");
	fs::create_dir(&target).map_err(Error::io("create the extraction directory"))?;
	let root = zipx::extract(zip, &target, top, &Limits::APP)?;
	let bin = root.join("bin");
	let mut found = Vec::new();
	let entries = fs::read_dir(&bin)
		.map_err(|_| Error::BundleInvalid("archive has no bin directory".into()))?;
	for entry in entries.flatten() {
		let name = entry.file_name().to_string_lossy().into_owned();
		let plain = entry.file_type().is_ok_and(|t| t.is_file());
		if !plain || !PROGRAMS.contains(&name.as_str()) {
			return Err(Error::BundleInvalid(format!(
				"unexpected file {name:?} in bin"
			)));
		}
		found.push(name);
	}
	if !found.iter().any(|n| n == "relay-desktop.exe") {
		return Err(Error::BundleInvalid("relay-desktop.exe is missing".into()));
	}
	Ok(bin)
}

/// Replaces the programs in `install_dir` with those of `new_bin`. Existing files move to
/// `<name>.old-<random>`; on any failure every move is undone. Returns the moved-aside files.
pub fn swap_portable(install_dir: &Path, new_bin: &Path) -> Result<Vec<PathBuf>> {
	let mut asides: Vec<(PathBuf, PathBuf)> = Vec::new();
	let mut placed: Vec<PathBuf> = Vec::new();
	let result = (|| {
		for name in PROGRAMS {
			let source = new_bin.join(name);
			if !source.is_file() {
				continue;
			}
			let target = install_dir.join(name);
			if target.exists() {
				let old = install_dir.join(format!("{name}.old-{}", random_suffix()));
				fs::rename(&target, &old).map_err(Error::io("move a running program aside"))?;
				asides.push((target.clone(), old));
			}
			fs::rename(&source, &target).map_err(Error::io("move a new program into place"))?;
			placed.push(target);
		}
		Ok(())
	})();
	if let Err(error) = result {
		for path in placed.iter().rev() {
			let _ = fs::remove_file(path);
		}
		for (target, old) in asides.iter().rev() {
			let _ = fs::rename(old, target);
		}
		return Err(error);
	}
	Ok(asides.into_iter().map(|(_, old)| old).collect())
}

/// Removes `<name>.old-<32 hex>` files of the programs in `install_dir`.
pub fn cleanup_stale(install_dir: &Path) {
	let Ok(entries) = fs::read_dir(install_dir) else {
		return;
	};
	for entry in entries.flatten() {
		let file = entry.file_name().to_string_lossy().into_owned();
		let ours = PROGRAMS.iter().any(|program| {
			file.strip_prefix(&format!("{program}.old-"))
				.is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()))
		});
		if ours && entry.file_type().is_ok_and(|t| t.is_file()) {
			let _ = fs::remove_file(entry.path());
		}
	}
}

#[cfg(windows)]
use semver::Version;

#[cfg(windows)]
pub fn apply_msi(
	dir: &Path,
	msi: &Path,
	sha256_hex: &str,
	staging: PathBuf,
	version: &Version,
) -> Result<super::Relaunch> {
	let launch = dir.join("relay-desktop.exe");
	// The helper needs the MSI after this process is gone, so the directory is kept; it deletes it.
	crate::helper::powershell_relaunch(
		std::process::id(),
		&launch,
		Some((msi, sha256_hex)),
		&[],
		Some(&staging),
	)?;
	Ok(super::Relaunch {
		version: version.clone(),
		launch,
	})
}

#[cfg(windows)]
pub fn apply_portable(
	dir: &Path,
	zip: &Path,
	staging: PathBuf,
	asset_name: &str,
	version: &Version,
) -> Result<super::Relaunch> {
	let new_bin = prepare_portable(zip, &staging, asset_name)?;
	let old = swap_portable(dir, &new_bin)?;
	let launch = dir.join("relay-desktop.exe");
	let olds: Vec<&Path> = old.iter().map(PathBuf::as_path).collect();
	crate::helper::powershell_relaunch(std::process::id(), &launch, None, &olds, Some(&staging))?;
	Ok(super::Relaunch {
		version: version.clone(),
		launch,
	})
}
