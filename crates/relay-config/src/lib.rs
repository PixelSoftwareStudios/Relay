//! Local persistence for Relay: preferences, trust and avatars in one per-user SQLite
//! database; identity keys in the OS credential store; diagnostics.
//!
//! Nothing here silently weakens a security posture. Damage found while loading is
//! reported as a [`Notice`], microphone selection never falls back to another
//! device, and the identity key only reaches a plaintext file if the user chose that.

pub mod avatar;
pub mod channels;
pub mod db;
pub mod diagnostics;
pub mod error;
pub mod identity;
pub mod keychain;
pub mod notice;
pub mod paths;
pub mod prefs;
pub mod storage;
pub mod vault;
mod writer_lock;

pub use error::{AvatarError, ConfigError, Result};
pub use notice::Notice;
pub use paths::Paths;
pub use storage::Storage;
pub use writer_lock::WriterLock;
