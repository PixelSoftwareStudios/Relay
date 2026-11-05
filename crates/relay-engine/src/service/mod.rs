//! The control service: one actor owning the domain state, and the local API
//! implementation that talks to it.

pub mod fakes;
pub mod ports;

pub use ports::{
	DeviceControl, DeviceDirection, DeviceInfo, LocalProfile, MediaControl, PeerControl, PeerInfo,
	PeerTrust, PortError, Ports, SettingsChange, SettingsStore, StoredSettings, TrustRequest,
};
