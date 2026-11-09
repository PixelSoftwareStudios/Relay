//! The desktop implementation of `relay_ui::shell::Shell`: egui viewport
//! commands, the egui clipboard and the native (rfd) file dialog.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use relay_ui::daemon_control;
use relay_ui::format::avatar_problem;
use relay_ui::host_options::HostOptions;
use relay_ui::hotkey_settings::HotkeyState;
use relay_ui::imaging::avatar_check;
use relay_ui::microphone::MicrophoneAccess;
use relay_ui::relay_link::resolve_endpoint;
use relay_ui::shell::{BoxFuture, FilePicker, PickedFile, Shell};

use crate::background::Background;
use crate::hotkey::HotkeyShared;
use crate::microphone::{MicrophoneGate, System};
use crate::options::DesktopOptions;
use crate::ptt::PttControl;
use relay_ui::tr;

pub struct DesktopShell {
	ctx: egui::Context,
	tray: AtomicBool,
	/// Set by [`Shell::hide_window`], cleared by [`Shell::show_window`].
	hidden: AtomicBool,
	options: Arc<DesktopOptions>,
	microphone: MicrophoneGate,
	/// The window had the keyboard focus on the last frame.
	focused: AtomicBool,
	/// Push-to-talk presses; without it the hold button and the key are not offered.
	ptt: Option<Arc<PttControl>>,
	hotkey: Arc<HotkeyShared>,
	/// `background.available` as last seen; closing hides to the tray only while it is on.
	background: Arc<Background>,
}

impl DesktopShell {
	pub fn new(ctx: egui::Context, options: Arc<DesktopOptions>) -> Self {
		let repaint = ctx.clone();
		Self {
			ctx,
			tray: AtomicBool::new(false),
			hidden: AtomicBool::new(false),
			options,
			microphone: MicrophoneGate::new(
				Arc::new(System),
				Box::new(move || repaint.request_repaint()),
			),
			focused: AtomicBool::new(false),
			ptt: None,
			hotkey: Arc::default(),
			background: Arc::default(),
		}
	}

	/// Share the background-availability state that the sink keeps current.
	pub fn with_background(mut self, background: Arc<Background>) -> Self {
		self.background = background;
		self
	}

	/// Offer push-to-talk: the hold button presses through `ptt` and the
	/// settings show what became of the key kept in `hotkey`.
	pub fn with_push_to_talk(mut self, ptt: Arc<PttControl>, hotkey: Arc<HotkeyShared>) -> Self {
		self.ptt = Some(ptt);
		self.hotkey = hotkey;
		self
	}

	/// Read the microphone permission and, if the user was never asked, show the
	/// system prompt now (called once at launch).
	pub fn start_microphone(&self) {
		self.microphone.start();
	}

	/// The window is on screen, not minimized and has the keyboard focus: the user can see an
	/// incoming call without a notification.
	pub fn in_front(&self) -> bool {
		!self.hidden.load(Ordering::Acquire)
			&& self.ctx.input(|i| {
				let viewport = i.viewport();
				!viewport.minimized.unwrap_or(false) && viewport.focused.unwrap_or(false)
			})
	}

	/// Record whether a tray icon exists (closing then hides the window).
	pub fn set_tray(&self, present: bool) {
		self.tray.store(present, Ordering::Release);
	}
}

impl FilePicker for DesktopShell {
	fn pick_image(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(async {
			let dialog = rfd::AsyncFileDialog::new()
				.set_title(tr!("dialog-profile-image"))
				.add_filter(
					tr!("dialog-image-filter"),
					&["png", "jpg", "jpeg", "webp", "gif", "bmp", "tif", "tiff"],
				)
				.pick_file()
				.await;
			match dialog {
				Some(file) => read_picked(file.path()).await.map(Some),
				None => Ok(None),
			}
		})
	}

	fn pick_audio(&self) -> BoxFuture<'_, Result<Option<PickedFile>, String>> {
		Box::pin(crate::ringtone_file::pick_audio())
	}

	fn pick_directory(&self, title: &str) -> BoxFuture<'_, Result<Option<PathBuf>, String>> {
		let dialog = rfd::AsyncFileDialog::new().set_title(title).pick_folder();
		Box::pin(dialog_path(dialog))
	}

	fn save_as(&self, suggested_name: &str) -> BoxFuture<'_, Result<Option<PathBuf>, String>> {
		let dialog = rfd::AsyncFileDialog::new()
			.set_title(tr!("dialog-export"))
			.set_file_name(suggested_name)
			.add_filter("JSON", &["json"])
			.save_file();
		Box::pin(dialog_path(dialog))
	}

	fn pick_file(&self, title: &str) -> BoxFuture<'_, Result<Option<PathBuf>, String>> {
		let dialog = rfd::AsyncFileDialog::new().set_title(title).pick_file();
		Box::pin(dialog_path(dialog))
	}
}

/// Native folder, save and file dialogs for messaging. The dialogs return
/// paths only: the daemon opens, writes and verifies the files itself.
async fn dialog_path(
	dialog: impl std::future::Future<Output = Option<rfd::FileHandle>>,
) -> Result<Option<PathBuf>, String> {
	Ok(dialog.await.map(|h| h.path().to_path_buf()))
}

/// Check the size before reading so a huge file is never loaded.
pub async fn read_picked(path: &Path) -> Result<PickedFile, String> {
	let name = path.to_string_lossy().into_owned();
	let size = tokio::fs::metadata(path)
		.await
		.map_err(|e| tr!("err-read-file", error = e.to_string()))?
		.len();
	let size = usize::try_from(size)
		.unwrap_or(usize::MAX)
		.min(relay_ui::controller::AVATAR_MAX_BYTES + 1);
	if let Some(problem) = avatar_problem(size) {
		return Err(problem.to_owned());
	}
	let bytes = tokio::fs::read(path)
		.await
		.map_err(|e| tr!("err-read-file", error = e.to_string()))?;
	// The format comes from the bytes, not the name.
	avatar_check(&bytes)?;
	Ok(PickedFile { name, bytes })
}

impl Shell for DesktopShell {
	fn open_url(&self, url: &str) {
		self.ctx.open_url(egui::OpenUrl::new_tab(url));
	}

	fn copy_text(&self, text: &str) {
		self.ctx.copy_text(text.to_owned());
	}

	fn request_repaint(&self) {
		self.ctx.request_repaint();
	}

	fn window_visible(&self) -> bool {
		!self.ctx.input(|i| i.viewport().minimized.unwrap_or(false))
	}

	fn show_window(&self) {
		self.hidden.store(false, Ordering::Release);
		self.ctx
			.send_viewport_cmd(egui::ViewportCommand::Visible(true));
		self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
	}

	fn hide_window(&self) {
		self.hidden.store(true, Ordering::Release);
		self.ctx
			.send_viewport_cmd(egui::ViewportCommand::Visible(false));
	}

	fn quit(&self) {
		self.ctx.send_viewport_cmd(egui::ViewportCommand::Close);
	}

	fn hides_to_tray(&self) -> bool {
		self.tray.load(Ordering::Acquire) && self.background.enabled()
	}

	/// Re-read the permission each time the window gains focus: the user may have
	/// turned it on in System Settings while the app ran.
	fn microphone_access(&self) -> MicrophoneAccess {
		let focused = self.ctx.input(|i| i.viewport().focused.unwrap_or(false));
		if focused && !self.focused.swap(true, Ordering::AcqRel) {
			self.microphone.refresh();
		} else if !focused {
			self.focused.store(false, Ordering::Release);
		}
		self.microphone.access()
	}

	fn request_microphone(&self) {
		self.microphone.ask();
	}

	/// The Privacy pane for the microphone (the OS has no per-app deep link).
	fn open_microphone_settings(&self) {
		open_microphone_settings();
	}

	fn can_manage_daemon(&self) -> bool {
		true
	}

	fn has_global_hotkey(&self) -> bool {
		self.ptt.is_some()
	}

	fn has_talk_button(&self) -> bool {
		self.ptt.is_some()
	}

	fn push_to_talk(&self, call_id: &str, pressed: bool) {
		let Some(ptt) = &self.ptt else {
			return;
		};
		if pressed {
			ptt.press(call_id);
		} else {
			ptt.release();
		}
	}

	fn hotkey_state(&self) -> HotkeyState {
		self.hotkey.state()
	}

	fn hotkey_accelerator(&self, accelerator: &str) {
		if self.hotkey.set_accelerator(accelerator) {
			self.ctx.request_repaint();
		}
	}

	fn host_options(&self) -> Option<&dyn HostOptions> {
		Some(self.options.as_ref())
	}

	/// Launches the separate `relayd` binary itself; the `relay` command line
	/// is never needed.
	fn start_daemon(&self) -> BoxFuture<'_, Result<String, String>> {
		Box::pin(async {
			let endpoint = resolve_endpoint().map_err(|e| e.to_string())?;
			daemon_control::start(&endpoint).await
		})
	}

	fn can_reset_data(&self) -> bool {
		true
	}

	fn reset_local_data(&self) -> BoxFuture<'_, Result<(), String>> {
		Box::pin(daemon_control::reset_local_data())
	}

	fn retry_secure_storage(&self) -> BoxFuture<'_, Result<(), String>> {
		Box::pin(async {
			let endpoint = resolve_endpoint().map_err(|e| e.to_string())?;
			daemon_control::retry_secure_storage(&endpoint).await
		})
	}

	fn use_file_storage(&self) -> BoxFuture<'_, Result<(), String>> {
		Box::pin(async {
			let endpoint = resolve_endpoint().map_err(|e| e.to_string())?;
			daemon_control::use_file_storage(&endpoint).await
		})
	}

	fn stop_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
		Box::pin(async {
			let endpoint = resolve_endpoint().map_err(|e| e.to_string())?;
			daemon_control::stop(&endpoint).await
		})
	}
}

/// The system page where the microphone permission lives.
#[cfg(target_os = "macos")]
fn open_microphone_settings() {
	const PANE: &str = "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone";
	if let Err(e) = std::process::Command::new("open").arg(PANE).spawn() {
		tracing::warn!(error = %e, "could not open System Settings");
	}
}

#[cfg(not(target_os = "macos"))]
fn open_microphone_settings() {}
