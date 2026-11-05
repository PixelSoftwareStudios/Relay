//! The desktop implementation of `relay_ui::shell::Shell`: egui viewport
//! commands, the egui clipboard and the native (rfd) file dialog.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use relay_ui::daemon_control;
use relay_ui::format::avatar_problem;
use relay_ui::host_options::HostOptions;
use relay_ui::imaging::avatar_check;
use relay_ui::relay_link::resolve_endpoint;
use relay_ui::shell::{BoxFuture, FilePicker, PickedFile, Shell};

use crate::options::DesktopOptions;
use relay_ui::tr;

pub struct DesktopShell {
	ctx: egui::Context,
	tray: AtomicBool,
	/// Set by [`Shell::hide_window`], cleared by [`Shell::show_window`].
	hidden: AtomicBool,
	options: Arc<DesktopOptions>,
}

impl DesktopShell {
	pub fn new(ctx: egui::Context, options: Arc<DesktopOptions>) -> Self {
		Self {
			ctx,
			tray: AtomicBool::new(false),
			hidden: AtomicBool::new(false),
			options,
		}
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
		self.tray.load(Ordering::Acquire)
	}

	fn can_manage_daemon(&self) -> bool {
		true
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

	fn stop_daemon(&self) -> BoxFuture<'_, Result<(), String>> {
		Box::pin(async {
			let endpoint = resolve_endpoint().map_err(|e| e.to_string())?;
			daemon_control::stop(&endpoint).await
		})
	}
}
