//! The types that cross the Rust/Kotlin boundary (UniFFI). Kotlin implements
//! [`HostBridge`]; Rust calls it for everything that needs the Android framework.

use std::collections::HashMap;

/// A failure reported by the Kotlin side.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum HostError {
	#[error("{message}")]
	Failed { message: String },
	#[error("host callback failed unexpectedly: {message}")]
	Unexpected { message: String },
}

impl From<uniffi::UnexpectedUniFFICallbackError> for HostError {
	fn from(error: uniffi::UnexpectedUniFFICallbackError) -> Self {
		Self::Unexpected {
			message: error.reason,
		}
	}
}

/// Why the engine could not start or stop.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum EngineError {
	#[error("engine: {message}")]
	Failed { message: String },
}

/// Where the app keeps its files.
#[derive(Debug, Clone, uniffi::Record)]
pub struct EngineConfig {
	/// `Context.getFilesDir()`: the app-private directory. Never a shared path.
	pub files_dir: String,
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

/// What to pick with the system document picker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum PickKind {
	/// An image for the profile avatar; Kotlin enforces the size limit before reading.
	Image,
	/// Any file to offer to a peer; Kotlin copies it into the app cache.
	File,
}

/// How a picker request ended.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum PickOutcome {
	Cancelled,
	Image { name: String, bytes: Vec<u8> },
	File { path: String },
	Failed { message: String },
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
	/// The call changed. Kotlin starts or stops the foreground service and
	/// asks for the microphone permission when a call begins without it.
	fn call_changed(&self, summary: CallSummary);
	/// Close the activity and the app (the UI's quit action).
	fn quit_app(&self);
}
