//! Settings only this app keeps, in a small TOML file in the per-user config directory.
//! The daemon's preferences are not used: it knows nothing about windows or notifications.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Largest prefs file read; anything bigger is ignored as damaged.
const MAX_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct DesktopPrefs {
	/// Show a system notification for a call that rings while the window is out of sight.
	pub notify_incoming: bool,
}

impl Default for DesktopPrefs {
	fn default() -> Self {
		Self {
			notify_incoming: true,
		}
	}
}

impl DesktopPrefs {
	/// `<config dir>/desktop.toml`, or `None` when the platform has no home directory.
	pub fn default_path() -> Option<PathBuf> {
		directories::ProjectDirs::from("", "", "relay")
			.map(|dirs| dirs.config_dir().join("desktop.toml"))
	}

	/// Read the file; a missing, oversized or malformed one gives the defaults.
	pub fn load(path: &Path) -> Self {
		let Ok(meta) = std::fs::metadata(path) else {
			return Self::default();
		};
		if meta.len() > MAX_BYTES {
			return Self::default();
		}
		std::fs::read_to_string(path)
			.ok()
			.and_then(|text| toml::from_str(&text).ok())
			.unwrap_or_default()
	}

	/// Write the file, creating its directory.
	pub fn save(&self, path: &Path) -> std::io::Result<()> {
		if let Some(dir) = path.parent() {
			std::fs::create_dir_all(dir)?;
		}
		let text = toml::to_string(self).map_err(std::io::Error::other)?;
		std::fs::write(path, text)
	}
}
