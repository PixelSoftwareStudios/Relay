//! User preferences: schema, migrations, device resolution and the store.

mod devices;
mod migrate;
mod model;
mod store;

pub use devices::DeviceChoice;
pub use model::{
	Audio, CURRENT_VERSION, DeviceSelection, Diagnostics, Discovery, Identity, KeyStorage, Network,
	NoiseBackend, Preferences, Privacy, Profile,
};
pub use store::{Loaded, PreferenceStore};
