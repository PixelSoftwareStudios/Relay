//! Platform integration for headless Relay clients: global hotkeys, push-to-talk
//! lease driving and OS lock detection.
//!
//! Nothing here needs a GUI. A hotkey backend reports an honest [`BackendStatus`]
//! and emits press/release events; the [`ptt::PttDriver`] turns those into
//! `InputControl` messages with renewable leases and fails closed on any error.

pub mod hotkey;
pub mod ptt;
pub mod session;
pub mod testing;

pub use hotkey::{
	BackendStatus, HotkeyBackend, HotkeyBinding, HotkeyError, HotkeyEvent, HotkeyStream,
	PermissionKind, detect_backend,
};
pub use ptt::{PttConfig, PttDriver, PttError, PttInput, PttState, PttTarget};
