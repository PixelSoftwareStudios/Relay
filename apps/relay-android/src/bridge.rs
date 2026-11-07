//! The types that cross the Rust/Kotlin boundary (UniFFI). Kotlin implements
//! [`HostBridge`]; Rust calls it for everything that needs the Android framework.

use std::collections::HashMap;

/// A failure reported by the Kotlin side.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum HostError {
	#[error("{reason}")]
	Failed { reason: String },
	#[error("host callback failed unexpectedly: {reason}")]
	Unexpected { reason: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for HostError {
	fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
		Self::Unexpected {
			reason: error.reason,
		}
	}
}

/// Why the engine could not start or stop.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum EngineError {
	#[error("engine: {reason}")]
	Failed { reason: String },
}

/// Where the app keeps its files.
#[derive(Debug, Clone, uniffi::Record)]
pub struct EngineConfig {
	/// `Context.getFilesDir()`: the app-private directory. Never a shared path.
	pub files_dir: String,
	/// The name shown until the user chooses one: the device name ("Galaxy S25
	/// Ultra"), because an Android hostname is just `localhost`. Chosen in Kotlin.
	pub default_display_name: Option<String>,
}

/// What the foreground service shows about the call, if any.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct CallSummary {
	/// A call exists that is not over (ringing, connecting or connected).
	pub active: bool,
	/// An incoming call is waiting to be answered.
	pub incoming: bool,
	/// The microphone is muted for the call.
	pub muted: bool,
	/// The other side's display name; empty when unknown.
	pub peer: String,
}

impl CallSummary {
	pub fn idle() -> Self {
		Self {
			active: false,
			incoming: false,
			muted: false,
			peer: String::new(),
		}
	}
}

/// The microphone permission as the UI needs to tell it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum MicrophoneState {
	/// Granted, or the system dialog is on screen.
	Ready,
	/// Not asked yet.
	NotAsked,
	/// Refused; the system will show the dialog again.
	Denied,
	/// Refused for good: only the app settings can change it.
	Blocked,
}

/// What to pick with the system document picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PickKind {
	/// An image for the profile avatar; Kotlin enforces the size limit before reading.
	Image,
	/// Any file to offer to a peer; Kotlin copies it into the app cache.
	File,
	/// An audio file for the ringtone; Kotlin checks the size, reads the bytes and the engine
	/// decodes them. Nothing is stored by the picker.
	Audio,
}

/// How a picker request ended.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum PickOutcome {
	Cancelled,
	Image { name: String, bytes: Vec<u8> },
	File { path: String },
	Audio { name: String, bytes: Vec<u8> },
	Failed { reason: String },
}

/// How a QR scan request ended. The text is untrusted; Rust checks it.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum ScanOutcome {
	Text { text: String },
	Cancelled,
	CameraDenied,
	CameraBlocked,
	Unavailable,
	Failed { reason: String },
}

/// System bar and keyboard sizes in physical pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, uniffi::Record)]
pub struct InsetsPx {
	pub top: f32,
	pub bottom: f32,
	pub left: f32,
	pub right: f32,
	/// Soft keyboard height; 0 when hidden.
	pub keyboard: f32,
}

/// A service the platform browser (`NsdManager`) resolved.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FoundService {
	pub fullname: String,
	pub port: u16,
	pub addresses: Vec<String>,
	pub txt: HashMap<String, String>,
}

/// Everything Rust asks of the Android framework. Implemented in Kotlin.
#[uniffi::export(with_foreign)]
pub trait HostBridge: Send + Sync {
	/// Encrypts with the Android Keystore key (AES-GCM). `context` is bound as
	/// associated data so a sealed value cannot be moved to another slot.
	fn seal(&self, context: String, plain: Vec<u8>) -> Result<Vec<u8>, HostError>;
	/// Reverses [`HostBridge::seal`].
	fn open(&self, context: String, sealed: Vec<u8>) -> Result<Vec<u8>, HostError>;
	fn copy_text(&self, text: String);
	fn open_url(&self, url: String);
	/// Starts the document picker; the answer comes back through
	/// `RelayEngine::complete_pick` with the same id.
	fn pick(&self, request_id: u64, kind: PickKind);
	/// Opens the QR scanner (asking for the camera on first use); the answer
	/// comes back through `RelayEngine::complete_scan` with the same id.
	fn scan_qr(&self, request_id: u64);
	/// The call changed. Kotlin starts or stops the foreground service and
	/// asks for the microphone permission when a call begins without it.
	fn call_changed(&self, summary: CallSummary);
	/// The engine's audio streams are about to open (`true`) or the last one
	/// is gone (`false`). Kotlin enters or leaves the call audio state
	/// (mode, focus, route) and returns only once it is in place, so the
	/// stream that opens next is not disconnected by it. Called from the
	/// engine's audio device thread.
	fn audio_streams_wanted(&self, wanted: bool);
	/// Show the system microphone dialog (the UI's "Allow microphone").
	fn request_microphone(&self);
	/// Open this app's page in the system settings.
	fn open_app_settings(&self);
	/// Close the activity and the app (the UI's quit action).
	fn quit_app(&self);
}
