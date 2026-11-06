//! What the platform shell (desktop window, Android activity) provides to the
//! shared UI. The UI never touches a windowing library, the clipboard or a
//! native dialog directly; it asks the shell through these traits.

use crate::tr;
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
		Box::pin(async { Err(no_paths().to_owned()) })
	}

	/// Native "save as" dialog returning the chosen new file path (export of a
	/// transcript). The daemon creates the file; the UI never writes it.
	/// Same stub rule as [`FilePicker::pick_directory`].
	fn save_as(&self, _suggested_name: &str) -> BoxFuture<'_, Result<Option<PathBuf>, String>> {
		Box::pin(async { Err(no_paths().to_owned()) })
	}

	/// Native "choose a file" dialog for a file offer. Only the absolute path
	/// is returned (the daemon opens it; the UI never reads the file).
	/// Same stub rule as [`FilePicker::pick_directory`].
	fn pick_file(&self, _title: &str) -> BoxFuture<'_, Result<Option<PathBuf>, String>> {
		Box::pin(async { Err(no_paths().to_owned()) })
	}
}

/// Shown when a shell cannot hand the daemon a filesystem path.
pub fn no_paths() -> &'static str {
	tr!("err-no-paths")
}

/// Screen areas covered by system UI, in points: status bar and cutout (top),
/// gesture or navigation bar (bottom), rounded corners and notches (sides).
/// All zero on a desktop window.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Insets {
	pub top: f32,
	pub bottom: f32,
	pub left: f32,
	pub right: f32,
}

/// Services the shell implements for the shared UI. Methods may be called from
/// the UI thread or from a runtime thread, so they must not block.
pub trait Shell: FilePicker {
	/// Put text on the system clipboard.
	fn copy_text(&self, text: &str);
	/// Open an http(s) link in the system browser. Called only after the user
	/// confirmed the real host in a dialog; never for an unconfirmed link. The
	/// default does nothing (a shell with no browser).
	fn open_url(&self, _url: &str) {}
	/// The zone message times are shown in. The default is the system zone;
	/// a shell (or a test) may return a fixed one.
	fn time_zone(&self) -> relay_io::localtime::LocalZone {
		relay_io::localtime::LocalZone::system()
	}
	/// Now, in unix milliseconds, for relative times such as "5 min ago". The default is the
	/// system clock; a shell (or a test) may return a fixed one.
	fn now_unix_ms(&self) -> u64 {
		std::time::SystemTime::now()
			.duration_since(std::time::UNIX_EPOCH)
			.map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
	}
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
		Box::pin(async { Err(tr!("err-cannot-start-daemon").to_owned()) })
	}
	/// Ask the background service to read the system's secure storage again, after it could
	/// not (the user may have allowed access meanwhile).
	fn retry_secure_storage(&self) -> BoxFuture<'_, Result<(), String>> {
		Box::pin(async { Err(tr!("err-cannot-start-daemon").to_owned()) })
	}
	/// Tell the background service to keep identity and secrets in the file-backed store
	/// instead of the system's secure storage. Only called after the user confirmed.
	fn use_file_storage(&self) -> BoxFuture<'_, Result<(), String>> {
		Box::pin(async { Err(tr!("err-cannot-start-daemon").to_owned()) })
	}
	/// Ask the daemon to stop. Distinct from [`Shell::quit`], which only
	/// closes this app.
	fn stop_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
		Box::pin(async { Err(tr!("err-cannot-stop-daemon").to_owned()) })
	}
	/// True when closing the window hides it (a tray exists), so the UI can say so.
	fn hides_to_tray(&self) -> bool;
	/// Desktop-only switches (call notifications, start at login). `None` hides them.
	fn host_options(&self) -> Option<&dyn crate::host_options::HostOptions> {
		None
	}
	/// Areas the system UI covers, in points. The app keeps its content out of
	/// them. Called every frame, so it must be cheap.
	fn safe_area_insets(&self) -> Insets {
		Insets::default()
	}
	/// Height in points of the soft keyboard when shown, else 0. The app keeps
	/// the focused text field visible above it.
	fn keyboard_height(&self) -> f32 {
		0.0
	}
	/// Whether the shell registers a system-wide push-to-talk key. When false
	/// the settings drop the global-hotkey wording and panel.
	fn has_global_hotkey(&self) -> bool {
		true
	}
	/// Whether leaving the app (closing its window or sending it away) keeps it
	/// and its calls running without a window.
	fn runs_in_background(&self) -> bool {
		false
	}
	/// Whether the UI shows an on-screen hold-to-talk button on calls in
	/// push-to-talk mode (for devices without a physical key).
	fn has_talk_button(&self) -> bool {
		false
	}
	/// The on-screen hold-to-talk button went down (`true`) or up (`false`) for
	/// the call `call_id` (hex). The shell forwards it to the engine's
	/// push-to-talk input; the default ignores it.
	fn push_to_talk(&self, _call_id: &str, _pressed: bool) {}
	/// What became of the system-wide push-to-talk key this shell registers.
	/// Shells without one keep the default.
	fn hotkey_state(&self) -> crate::hotkey_settings::HotkeyState {
		crate::hotkey_settings::HotkeyState::Unsupported
	}
	/// The stored push-to-talk accelerator was read or saved (empty turns the
	/// key off). A shell that registers the key follows it at once.
	fn hotkey_accelerator(&self, _accelerator: &str) {}
	/// Whether the app may use the microphone, as the OS says. Called per
	/// frame, so it must be cheap. The default needs no permission.
	fn microphone_access(&self) -> crate::microphone::MicrophoneAccess {
		crate::microphone::MicrophoneAccess::Ready
	}
	/// Show the OS permission prompt (the user pressed "Allow microphone").
	/// Never blocks; the answer arrives through [`Shell::microphone_access`].
	fn request_microphone(&self) {}
	/// Open the OS page where the microphone permission can be turned on.
	fn open_microphone_settings(&self) {}
	/// Whether the shell can scan a QR code with the camera. The Scan button is
	/// shown only when this is true. The default has no scanner.
	fn can_scan_qr(&self) -> bool {
		false
	}
	/// Scan one QR code and return its text. The shell shows its own scanner
	/// (and asks for the camera permission on first use, not at startup).
	/// `Ok(None)` when the user cancelled. The text is untrusted: the UI checks
	/// it like a pasted connection string.
	fn scan_qr(&self) -> BoxFuture<'_, Result<Option<String>, crate::qr_scan::ScanError>> {
		Box::pin(async { Ok(None) })
	}
	/// Open the OS page where the camera permission can be turned on.
	fn open_camera_settings(&self) {}
}
