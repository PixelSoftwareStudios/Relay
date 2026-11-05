//! Hotkey backends with capability probing and honest status.

mod global;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(all(target_os = "linux", feature = "wayland"))]
mod portal;

use tokio::sync::mpsc;

pub use global::{GlobalHotkeyBackend, run_main_loop};
#[cfg(all(target_os = "linux", feature = "wayland"))]
pub use portal::PortalBackend;

/// OS permission a backend needs before it can deliver keys.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PermissionKind {
	/// macOS Accessibility (event taps, media keys).
	Accessibility,
	/// macOS Input Monitoring (listening to key events).
	InputMonitoring,
}

/// What a backend can do right now. Never reports success it cannot back up.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BackendStatus {
	/// Keys can be registered and delivered.
	Available,
	/// The user must grant a permission first.
	NeedsPermission(PermissionKind),
	/// This backend cannot work in the current session.
	Unsupported(String),
}

impl BackendStatus {
	/// True only for [`BackendStatus::Available`].
	pub fn is_available(&self) -> bool {
		matches!(self, Self::Available)
	}
}

/// Edge of the physical key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HotkeyEvent {
	/// Key went down.
	Pressed,
	/// Key went up.
	Released,
}

/// The push-to-talk key to register.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HotkeyBinding {
	/// Stable id (used by the Wayland portal).
	pub id: String,
	/// Human description shown by compositors.
	pub description: String,
	/// Accelerator such as `"Ctrl+Shift+Space"`.
	pub accelerator: String,
}

impl HotkeyBinding {
	/// A binding with the default Relay id.
	pub fn push_to_talk(accelerator: impl Into<String>) -> Self {
		Self {
			id: "relay-ptt".into(),
			description: "Relay push-to-talk".into(),
			accelerator: accelerator.into(),
		}
	}
}

/// Receiving side of a started backend; closes when the backend stops or dies.
pub type HotkeyStream = mpsc::UnboundedReceiver<HotkeyEvent>;

/// Hotkey backend errors.
#[derive(Debug, thiserror::Error)]
pub enum HotkeyError {
	/// The probe did not report [`BackendStatus::Available`].
	#[error("hotkey backend unavailable: {0:?}")]
	Unavailable(BackendStatus),
	/// The accelerator could not be parsed.
	#[error("invalid hotkey {0:?}: {1}")]
	InvalidBinding(String, String),
	/// The OS refused the registration (for example the combination is taken).
	#[error("hotkey registration failed: {0}")]
	Registration(String),
	/// The backend must be started from the process main thread.
	#[error("this backend must be started on the main thread")]
	NotMainThread,
	/// `start` was called twice without `stop`.
	#[error("backend already started")]
	AlreadyStarted,
}

/// A source of push-to-talk key edges.
pub trait HotkeyBackend: Send {
	/// Short backend name for diagnostics.
	fn name(&self) -> &'static str;
	/// Capability probe; cheap and side-effect free.
	fn probe(&self) -> BackendStatus;
	/// Register the key and start delivering edges. Fails unless [`probe`](Self::probe)
	/// is `Available`.
	fn start(&mut self, binding: &HotkeyBinding) -> Result<HotkeyStream, HotkeyError>;
	/// Unregister and close the stream. Idempotent.
	fn stop(&mut self);
}

/// Pick the backend for this session. Always returns something; inspect
/// [`HotkeyBackend::probe`] for the honest status.
pub fn detect_backend() -> Box<dyn HotkeyBackend> {
	#[cfg(all(target_os = "linux", feature = "wayland"))]
	if global::is_wayland_session() {
		return Box::new(PortalBackend::new());
	}
	Box::new(GlobalHotkeyBackend::new())
}
