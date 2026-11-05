//! What the platform shell (desktop window, Android activity) provides to the
//! shared UI. The UI never touches a windowing library, the clipboard or a
//! native dialog directly; it asks the shell through these traits.

use std::future::Future;
use std::path::PathBuf;
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

	/// Native "choose a folder" dialog, for downloading a received file. The
	/// daemon writes into the folder, so a real absolute path is returned.
	/// `Ok(None)` when cancelled.
	///
	/// The default is the documented stub for shells without filesystem paths
	/// (compact or Android shells that only get content URIs): it reports that
	/// the device cannot do it, and the UI then hides nothing but shows the text.
	fn pick_directory(&self, _title: &str) -> BoxFuture<'_, Result<Option<PathBuf>, String>> {
		Box::pin(async { Err(NO_PATHS.to_owned()) })
	}

	/// Native "save as" dialog returning the chosen new file path (export of a
	/// transcript). The daemon creates the file; the UI never writes it.
	/// Same stub rule as [`FilePicker::pick_directory`].
	fn save_as(&self, _suggested_name: &str) -> BoxFuture<'_, Result<Option<PathBuf>, String>> {
		Box::pin(async { Err(NO_PATHS.to_owned()) })
	}

	/// Native "choose a file" dialog for a file offer. Only the absolute path
	/// is returned (the daemon opens it; the UI never reads the file).
	/// Same stub rule as [`FilePicker::pick_directory`].
	fn pick_file(&self, _title: &str) -> BoxFuture<'_, Result<Option<PathBuf>, String>> {
		Box::pin(async { Err(NO_PATHS.to_owned()) })
	}
}

/// Shown when a shell cannot hand the daemon a filesystem path.
pub const NO_PATHS: &str = "This device cannot choose files or folders for the Relay daemon.";

/// Services the shell implements for the shared UI. Methods may be called from
/// the UI thread or from a runtime thread, so they must not block.
pub trait Shell: FilePicker {
	/// Put text on the system clipboard.
	fn copy_text(&self, text: &str);
	/// Open an http(s) link in the system browser. Called only after the user
	/// confirmed the real host in a dialog; never for an unconfirmed link. The
	/// default does nothing (a shell with no browser).
	fn open_url(&self, _url: &str) {}
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
	/// Whether this device can start and stop the standalone daemon (desktops
	/// can; a phone cannot).
	fn can_manage_daemon(&self) -> bool {
		false
	}
	/// Start the daemon in the background; the text becomes a notice.
	fn start_daemon(&self) -> BoxFuture<'_, Result<String, String>> {
		Box::pin(async { Err("This device cannot start the Relay daemon.".to_owned()) })
	}
	/// Ask the daemon to stop. Distinct from [`Shell::quit`], which only
	/// closes this app.
	fn stop_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
		Box::pin(async { Err("This device cannot stop the Relay daemon.".to_owned()) })
	}
	/// True when closing the window hides it (a tray exists), so the UI can say so.
	fn hides_to_tray(&self) -> bool;
}
