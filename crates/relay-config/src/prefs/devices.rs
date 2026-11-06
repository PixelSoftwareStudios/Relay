//! Device resolution. A missing explicitly chosen device is never replaced by another.

use super::model::{Audio, DeviceSelection};

/// What the audio layer should open for an endpoint.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeviceChoice {
	/// Open the operating system default device.
	SystemDefault,
	/// Open exactly this device.
	Device(String),
	/// An explicit selection with no device id recorded; treated as unset.
	NotSelected,
	/// The chosen device is not present. The user must choose; there is no fallback.
	Unavailable(String),
}

fn resolve(selection: DeviceSelection, preferred: &str, available: &[&str]) -> DeviceChoice {
	match selection {
		DeviceSelection::SystemDefault => DeviceChoice::SystemDefault,
		DeviceSelection::Explicit if preferred.is_empty() => DeviceChoice::NotSelected,
		DeviceSelection::Explicit if available.contains(&preferred) => {
			DeviceChoice::Device(preferred.to_owned())
		}
		DeviceSelection::Explicit => DeviceChoice::Unavailable(preferred.to_owned()),
	}
}

impl Audio {
	/// Resolves the microphone against currently available device ids.
	pub fn resolve_input(&self, available: &[&str]) -> DeviceChoice {
		resolve(self.input_selection, &self.preferred_input_id, available)
	}

	/// Resolves the playback device against currently available device ids.
	pub fn resolve_output(&self, available: &[&str]) -> DeviceChoice {
		resolve(self.output_selection, &self.preferred_output_id, available)
	}
}
