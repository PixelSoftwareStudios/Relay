//! macOS: replace `Relay.app` with the one from `relay-<v>-macos-<arch>-update.zip`.
//!
//! Steps: preflight (where does the app run), extract next to the bundle (same volume), check the
//! new bundle (plist, executable, `codesign --verify`), swap by two renames with rollback, and
//! let a detached helper open the new bundle once this process has exited.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use semver::Version;

use super::{Relaunch, random_suffix};
use crate::error::{Error, Result};
use crate::plist;
use crate::zipx::{self, Limits};

/// `CFBundleIdentifier` of `apps/relay-desktop/resources/Info.plist`.
pub const BUNDLE_ID: &str = "dev.relay.relay-desktop";
/// `CFBundleExecutable` written by `scripts/packaging/package_macos.py`.
pub const EXECUTABLE: &str = "relay-desktop";
/// The code identifier every program in the bundle is signed with (keychain identity).
pub const CODE_IDENTIFIER: &str = "dev.relay.relayd";

/// Refuses locations where an in-place swap cannot work or would be undone.
pub fn preflight(bundle: &Path) -> Result<()> {
	if bundle.to_string_lossy().contains("/AppTranslocation/") {
		return Err(Error::AppTranslocated);
	}
	let parent = bundle
		.parent()
		.ok_or_else(|| Error::BundleInvalid("bundle has no parent".into()))?;
	if rustix::fs::statvfs(parent)
		.is_ok_and(|s| s.f_flag.contains(rustix::fs::StatVfsMountFlags::RDONLY))
	{
		return Err(Error::ReadOnlyVolume(parent.to_path_buf()));
	}
	if rustix::fs::access(parent, rustix::fs::Access::WRITE_OK).is_err() {
		return Err(Error::ParentNotWritable(parent.to_path_buf()));
	}
	Ok(())
}

/// Extracts `zip` below `staging` and checks the new bundle. `codesign` is injectable so tests
/// need no signed app; production passes [`run_codesign`]. Returns the path of the new `.app`.
pub fn prepare(
	zip: &Path,
	staging: &Path,
	version: &Version,
	codesign: &dyn Fn(&Path) -> Result<()>,
) -> Result<PathBuf> {
	let target = staging.join("extracted");
	fs::create_dir(&target).map_err(Error::io("create the extraction directory"))?;
	let app = zipx::extract(zip, &target, "Relay.app", &Limits::APP)?;
	check_bundle(&app, version)?;
	codesign(&app)?;
	Ok(app)
}

fn check_bundle(app: &Path, version: &Version) -> Result<()> {
	let bad = |why: &str| Error::BundleInvalid(why.to_owned());
	let plist_path = app.join("Contents/Info.plist");
	let meta = fs::symlink_metadata(&plist_path).map_err(|_| bad("no Info.plist"))?;
	if !meta.is_file() || meta.len() > 256 * 1024 {
		return Err(bad("Info.plist is not a plain small file"));
	}
	let xml = fs::read_to_string(&plist_path).map_err(|_| bad("Info.plist is not XML text"))?;
	let value = |key| plist::string_value(&xml, key);
	if value("CFBundleIdentifier").as_deref() != Some(BUNDLE_ID) {
		return Err(bad("unexpected bundle identifier"));
	}
	let numeric = format!("{}.{}.{}", version.major, version.minor, version.patch);
	if value("CFBundleShortVersionString").as_deref() != Some(numeric.as_str()) {
		return Err(bad("bundle version differs from the release version"));
	}
	if value("CFBundleExecutable").as_deref() != Some(EXECUTABLE) {
		return Err(bad("unexpected bundle executable"));
	}
	let exe = app.join("Contents/MacOS").join(EXECUTABLE);
	let meta = fs::symlink_metadata(&exe).map_err(|_| bad("bundle executable is missing"))?;
	if !meta.is_file() || !is_executable(&meta) {
		return Err(bad("bundle executable is not an executable file"));
	}
	Ok(())
}

#[cfg(unix)]
fn is_executable(meta: &fs::Metadata) -> bool {
	use std::os::unix::fs::PermissionsExt;
	meta.permissions().mode() & 0o100 != 0
}

#[cfg(not(unix))]
fn is_executable(_: &fs::Metadata) -> bool {
	true
}

/// `codesign --verify --deep --strict` plus the code identifier the keychain identity relies on.
pub fn run_codesign(app: &Path) -> Result<()> {
	let run = |args: &[&str]| {
		Command::new("/usr/bin/codesign")
			.args(args)
			.arg(app)
			.output()
			.map_err(|e| Error::CodesignFailed(e.to_string()))
	};
	let verify = run(&["--verify", "--deep", "--strict"])?;
	if !verify.status.success() {
		return Err(Error::CodesignFailed(brief(&verify.stderr)));
	}
	let display = run(&["--display", "--verbose=2"])?;
	let text = String::from_utf8_lossy(&display.stderr);
	if !text
		.lines()
		.any(|l| l == format!("Identifier={CODE_IDENTIFIER}"))
	{
		return Err(Error::CodesignFailed(format!(
			"code identifier is not {CODE_IDENTIFIER}"
		)));
	}
	Ok(())
}

fn brief(bytes: &[u8]) -> String {
	String::from_utf8_lossy(bytes)
		.trim()
		.chars()
		.take(300)
		.collect()
}

/// Puts `new_app` where `bundle` is. The old bundle moves to `.<name>.old-<random>` first; if
/// the second rename fails the first is undone. Returns the path the old bundle now has.
pub fn swap(bundle: &Path, new_app: &Path) -> Result<PathBuf> {
	let parent = bundle
		.parent()
		.ok_or_else(|| Error::BundleInvalid("bundle has no parent".into()))?;
	let name = bundle
		.file_name()
		.ok_or_else(|| Error::BundleInvalid("bundle has no name".into()))?;
	let mut aside = std::ffi::OsString::from(".");
	aside.push(name);
	aside.push(format!(".old-{}", random_suffix()));
	let old = parent.join(aside);
	fs::rename(bundle, &old).map_err(Error::io("move the old app aside"))?;
	if let Err(error) = fs::rename(new_app, bundle) {
		// The old app is the only copy in place; put it back before reporting.
		let _ = fs::rename(&old, bundle);
		return Err(Error::Io {
			context: "move the new app into place",
			source: error,
		});
	}
	Ok(old)
}

/// Deletes `.<name>.old-*` siblings left by earlier updates (the helper removes the old bundle
/// after relaunch; this covers a helper that never ran).
pub fn cleanup_stale(bundle: &Path) {
	let (Some(parent), Some(name)) = (bundle.parent(), bundle.file_name()) else {
		return;
	};
	let prefix = format!(".{}.old-", name.to_string_lossy());
	let Ok(entries) = fs::read_dir(parent) else {
		return;
	};
	for entry in entries.flatten() {
		let file = entry.file_name();
		let file = file.to_string_lossy();
		let suffix = file.strip_prefix(&prefix);
		let is_ours =
			suffix.is_some_and(|s| s.len() == 32 && s.bytes().all(|b| b.is_ascii_hexdigit()));
		if is_ours
			&& entry
				.file_type()
				.is_ok_and(|t| t.is_dir() && !t.is_symlink())
		{
			let _ = fs::remove_dir_all(entry.path());
		}
	}
}

/// The whole macOS step; `staging` is the verified download's private directory.
pub fn apply(bundle: &Path, zip: &Path, staging: &Path, version: &Version) -> Result<Relaunch> {
	preflight(bundle)?;
	let new_app = prepare(zip, staging, version, &run_codesign)?;
	let old = swap(bundle, &new_app)?;
	if let Err(error) = crate::helper::relaunch_open(std::process::id(), bundle, &old) {
		// Without a helper nothing would start the new app; keep the old one.
		let _ = fs::rename(bundle, &new_app).and_then(|()| fs::rename(&old, bundle));
		return Err(error);
	}
	Ok(Relaunch {
		version: version.clone(),
		launch: bundle.to_path_buf(),
	})
}
