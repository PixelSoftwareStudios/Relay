//! Platform integration for Relay clients: global hotkeys and OS lock detection.
//!
//! Nothing here needs a GUI. A hotkey backend reports an honest [`BackendStatus`]
//! and emits press/release events. Turning those into push-to-talk leases is
//! `relay_client::ptt::PttWorker`'s job, shared by every client.

pub mod hotkey;
pub mod microphone;
pub mod session;
pub mod testing;

pub use hotkey::{
	BackendStatus, HotkeyBackend, HotkeyBinding, HotkeyError, HotkeyEvent, HotkeyStream,
	PermissionKind, detect_app_backend, detect_backend,
};
