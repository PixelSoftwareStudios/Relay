//! User preferences: schema, device resolution, SQLite store and TOML export/import.

mod devices;
pub mod flat;
mod migrate;
mod model;
mod portable;
pub(crate) mod sql;
mod store;

pub use devices::DeviceChoice;
pub use model::{
	Audio, CURRENT_VERSION, DeviceSelection, Diagnostics, Discovery, Identity, KeyStorage, Network,
	NoiseBackend, Preferences, Privacy, Profile,
};
pub(crate) use portable::parse_toml;
pub use portable::{export_toml, import_toml};
pub use store::{Loaded, PreferenceStore};
