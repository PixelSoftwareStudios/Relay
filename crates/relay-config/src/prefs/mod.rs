//! User preferences: schema, device resolution, SQLite store and TOML export/import.

mod devices;
pub mod flat;
mod model;
mod portable;
pub(crate) mod sql;
mod store;

pub use devices::DeviceChoice;
pub use model::{
	AdvertMode, Audio, CURRENT_VERSION, DeviceSelection, Diagnostics, Discovery, Hotkeys, Identity,
	InboundPolicy, KeyStorage, MAX_OFFER_SIZE_MIB, MAX_RELEASE_DELAY_MS, MAX_TRANSFER_RATE_MBPS,
	Messages, Network, NoiseBackend, Preferences, Privacy, Profile, Ptt,
};
pub use portable::{export_toml, import_toml};
pub use store::{Loaded, PreferenceStore};
