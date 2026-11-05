//! Stable device descriptors and selection policy.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
	Input,
	Output,
}

/// Sample formats the adapters understand.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SampleFormatKind {
	F32,
	I16,
	U16,
	I32,
}

/// Opaque handle valid only for the enumeration that produced it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DeviceHandle {
	pub(crate) enumeration: u64,
	pub(crate) index: u32,
}

/// Persistable identity: backend ID where available, never an array index.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StableDeviceKey {
	pub host: String,
	pub direction: Direction,
	pub backend_id: Option<String>,
	/// Descriptive fallback metadata, used only when no backend ID exists.
	pub name: String,
	pub manufacturer: Option<String>,
	pub interface: Option<String>,
}

impl StableDeviceKey {
	/// Whether `other` names the same physical device.
	pub fn matches(&self, other: &Self) -> bool {
		if self.host != other.host || self.direction != other.direction {
			return false;
		}
		match (&self.backend_id, &other.backend_id) {
			(Some(a), Some(b)) => a == b,
			_ => self.name == other.name && self.manufacturer == other.manufacturer,
		}
	}
}

/// Supported rate/channel/format range.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigRange {
	pub channels: u16,
	pub min_rate: u32,
	pub max_rate: u32,
	pub format: Option<SampleFormatKind>,
	pub max_buffer_frames: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceDescriptor {
	pub handle: DeviceHandle,
	pub key: StableDeviceKey,
	pub is_default: bool,
	pub configs: Vec<ConfigRange>,
}

/// What the user chose: the system default, or one particular device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "mode", content = "device", rename_all = "snake_case")]
pub enum DeviceSelection {
	FollowDefault,
	Pinned(StableDeviceKey),
}

/// Outcome of resolving a selection against an enumeration.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Resolution {
	Device(DeviceHandle),
	/// Following the default but the system has none.
	NoDefault,
	/// A pinned device is absent: pause and ask. Never substitute another.
	PinnedMissing,
}

/// One scan of the system's devices.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Enumeration {
	pub generation: u64,
	pub devices: Vec<DeviceDescriptor>,
}

impl Enumeration {
	pub fn get(&self, handle: DeviceHandle) -> Option<&DeviceDescriptor> {
		if handle.enumeration != self.generation {
			return None;
		}
		self.devices.get(handle.index as usize)
	}

	pub fn of_direction(&self, direction: Direction) -> impl Iterator<Item = &DeviceDescriptor> {
		self.devices
			.iter()
			.filter(move |d| d.key.direction == direction)
	}

	pub fn resolve(&self, direction: Direction, selection: &DeviceSelection) -> Resolution {
		match selection {
			DeviceSelection::FollowDefault => self
				.of_direction(direction)
				.find(|d| d.is_default)
				.map_or(Resolution::NoDefault, |d| Resolution::Device(d.handle)),
			DeviceSelection::Pinned(key) => self
				.of_direction(direction)
				.find(|d| d.key.matches(key))
				.map_or(Resolution::PinnedMissing, |d| Resolution::Device(d.handle)),
		}
	}
}
