//! Which audio devices are in use, derived from the saved choice and the
//! current device list.
//!
//! Nothing here touches the hardware: it describes the device the engine
//! opens next. A stream that is already open is not moved when the system
//! default changes (see `docs/audio.md`).

use std::collections::BTreeMap;

use super::ports::{DeviceDirection, DeviceInfo};

/// One direction's resolved choice.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EndpointUse {
	/// Nothing is pinned: the system default is followed.
	pub is_default: bool,
	/// Name of the device that is opened (empty when there is none).
	pub name: String,
	/// A pinned device is not present, so the system default is used.
	pub pinned_missing: bool,
}

/// Both directions.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeviceUse {
	/// Microphone.
	pub input: EndpointUse,
	/// Playback.
	pub output: EndpointUse,
}

fn endpoint(
	devices: &BTreeMap<String, DeviceInfo>,
	pinned: &str,
	direction: DeviceDirection,
) -> EndpointUse {
	let default_name = || {
		devices
			.values()
			.find(|d| d.direction == direction && d.system_default)
			.map(|d| d.name.clone())
			.unwrap_or_default()
	};
	if pinned.is_empty() {
		return EndpointUse {
			is_default: true,
			name: default_name(),
			pinned_missing: false,
		};
	}
	match devices.get(pinned) {
		Some(device) => EndpointUse {
			is_default: false,
			name: device.name.clone(),
			pinned_missing: false,
		},
		None => EndpointUse {
			is_default: false,
			name: default_name(),
			pinned_missing: true,
		},
	}
}

/// Resolves the saved ids against the device list.
pub fn device_use(devices: &BTreeMap<String, DeviceInfo>, input: &str, output: &str) -> DeviceUse {
	DeviceUse {
		input: endpoint(devices, input, DeviceDirection::Input),
		output: endpoint(devices, output, DeviceDirection::Output),
	}
}
