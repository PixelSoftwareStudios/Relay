//! Per-user directories. Everything resolves inside the current OS user's
//! profile or runtime area, so users of a shared PC never share state.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;

use crate::error::{IoError, Result};

const QUALIFIER: &str = "";
const ORGANIZATION: &str = "Relay";
const APPLICATION: &str = "Relay";
const SOCKET_DIR: &str = "relay";

/// The platform's config and data roots for this user.
#[derive(Debug, Clone)]
pub struct UserDirs {
	config: PathBuf,
	data: PathBuf,
	cache: PathBuf,
}

impl UserDirs {
	/// Resolves the platform defaults.
	pub fn platform() -> Result<Self> {
		let dirs = ProjectDirs::from(QUALIFIER, ORGANIZATION, APPLICATION)
			.ok_or(IoError::NoPlatformDir)?;
		Ok(Self {
			config: dirs.config_dir().to_path_buf(),
			data: dirs.data_local_dir().to_path_buf(),
			cache: dirs.cache_dir().to_path_buf(),
		})
	}

	/// Preferences and the writer lock.
	pub fn config_dir(&self) -> &Path {
		&self.config
	}

	/// The database, identity and channel secrets.
	pub fn data_dir(&self) -> &Path {
		&self.data
	}

	/// Disposable files.
	pub fn cache_dir(&self) -> &Path {
		&self.cache
	}
}

/// The directory holding the engine endpoint on Windows (where there are no Unix
/// sockets): `%LOCALAPPDATA%\Relay\run`, inside the user's own profile, whose
/// inherited ACL excludes other users. `None` when `%LOCALAPPDATA%` is not an
/// absolute path; callers fail instead of falling back to a shared `%TEMP%`.
pub fn windows_runtime_dir_from(local_app_data: Option<OsString>) -> Option<PathBuf> {
	let base = PathBuf::from(local_app_data?);
	base.is_absolute()
		.then(|| base.join(ORGANIZATION).join("run"))
}

/// [`windows_runtime_dir_from`] with the real environment.
pub fn windows_runtime_dir() -> Result<PathBuf> {
	windows_runtime_dir_from(std::env::var_os("LOCALAPPDATA")).ok_or(IoError::NoPlatformDir)
}

/// The directory holding the engine endpoint: `$XDG_RUNTIME_DIR/relay` when that
/// is absolute, else `<tmp>/relay-<uid>` (macOS has no runtime dir).
pub fn runtime_dir_from(xdg_runtime_dir: Option<OsString>, tmp: &Path, uid: u32) -> PathBuf {
	match xdg_runtime_dir.map(PathBuf::from) {
		Some(dir) if dir.is_absolute() => dir.join(SOCKET_DIR),
		_ => tmp.join(format!("relay-{uid}")),
	}
}

/// Where a detached daemon's log goes by default, from explicit environment values:
/// `~/Library/Logs/Relay` (macOS), `%LOCALAPPDATA%\Relay` (Windows),
/// `$XDG_STATE_HOME/relay` or `~/.local/state/relay` (elsewhere).
pub fn log_dir_from(
	home: Option<OsString>,
	local_app_data: Option<OsString>,
	xdg_state_home: Option<OsString>,
	tmp: &Path,
) -> PathBuf {
	let home = |relative: &str| {
		home.map_or_else(|| tmp.to_path_buf(), PathBuf::from)
			.join(relative)
	};
	if cfg!(windows) {
		local_app_data
			.map_or_else(|| tmp.to_path_buf(), PathBuf::from)
			.join("Relay")
	} else if cfg!(target_os = "macos") {
		home("Library/Logs/Relay")
	} else if let Some(state) = xdg_state_home {
		PathBuf::from(state).join("relay")
	} else {
		home(".local/state/relay")
	}
}

/// [`log_dir_from`] with the real environment.
pub fn log_dir() -> PathBuf {
	log_dir_from(
		std::env::var_os("HOME"),
		std::env::var_os("LOCALAPPDATA"),
		std::env::var_os("XDG_STATE_HOME"),
		&std::env::temp_dir(),
	)
}
