//! The configured push-to-talk accelerator.

use std::str::FromStr;

use global_hotkey::hotkey::HotKey;
use relay_config::Paths;
use relay_config::prefs::{Hotkeys, PreferenceStore};

/// Check that the platform backend can parse `accelerator`. Empty is allowed
/// (it disables the hotkey).
pub fn validate_accelerator(accelerator: &str) -> Result<(), String> {
	if accelerator.is_empty() {
		return Ok(());
	}
	HotKey::from_str(accelerator)
		.map(|_| ())
		.map_err(|e| format!("not a valid key combination: {e}"))
}

/// The accelerator for this run: the override, else the saved preference
/// (read without taking the settings lock), else the default.
/// With `ptt.enabled` off the saved preference reads as empty, so the key is not registered.
pub fn configured_binding(override_with: Option<&str>) -> String {
	if let Some(text) = override_with {
		return text.to_owned();
	}
	Paths::platform()
		.ok()
		.and_then(|paths| PreferenceStore::read_only(&paths).ok().flatten())
		.map_or_else(
			|| Hotkeys::default().push_to_talk,
			|p| {
				if p.ptt.enabled {
					p.hotkeys.push_to_talk
				} else {
					String::new()
				}
			},
		)
}
