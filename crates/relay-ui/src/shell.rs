//! What the platform shell (desktop window, Android activity) provides to the
//! shared UI. The UI never touches a windowing library, the clipboard or a
//! native dialog directly; it asks the shell through these traits.

use std::future::Future;
use std::pin::Pin;

/// A boxed, sendable future, so the traits stay object safe.
pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// An image the user chose. The shell reads it, after checking the size
/// against `controller::AVATAR_MAX_BYTES` so a huge file is never loaded.
#[derive(Debug)]
pub struct PickedFile {
	/// File name, used only to infer the media type.
	pub name: String,
	pub bytes: Vec<u8>,
}

/// Native "choose an image" dialog (a file dialog on desktop, the system
/// picker on Android).
pub trait FilePicker: Send + Sync {
	/// `Ok(None)` when the user cancelled; `Err` carries a message for the UI.
	fn pick_image(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>>;
}

/// Services the shell implements for the shared UI. Methods may be called from
/// the UI thread or from a runtime thread, so they must not block.
pub trait Shell: FilePicker {
	/// Put text on the system clipboard.
	fn copy_text(&self, text: &str);
	/// Ask for one more frame. Called by writers after new data, never per frame.
	fn request_repaint(&self);
	/// Whether the window is on screen (not minimized or hidden). Meters are
	/// produced only while this is true.
	fn window_visible(&self) -> bool;
	/// Bring the window back (tray "Show"). Mobile shells may ignore it.
	fn show_window(&self);
	/// Hide the window, keeping the process.
	fn hide_window(&self);
	/// Quit this app only; the daemon is a separate process.
	fn quit(&self);
	/// True when closing the window hides it (a tray exists), so the UI can say so.
	fn hides_to_tray(&self) -> bool;
}
