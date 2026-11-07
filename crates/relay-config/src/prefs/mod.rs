//! User preferences: schema, device resolution, SQLite store and TOML export/import.

mod devices;
pub mod flat;
pub mod limits;
mod model;
mod portable;
pub(crate) mod sql;
mod store;

pub use devices::DeviceChoice;
pub use limits::{LimitSpec, Limits};
pub use model::{
	AdvertMode, Audio, CURRENT_VERSION, Calls, DeviceSelection, Diagnostics, Discovery, History,
	Hotkeys, Identity, InboundPolicy, KeyStorage, MAX_OFFER_SIZE_MIB, MAX_RELEASE_DELAY_MS,
	MAX_TRANSFER_RATE_MBPS, Messages, Network, NoiseLevel, Preferences, Privacy, Profile, Ptt,
	TransmitModePref,
};
pub use portable::{export_toml, import_toml};
pub use store::{Loaded, PreferenceStore};
