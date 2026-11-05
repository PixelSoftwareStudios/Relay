//! The desktop implementation of `relay_ui::shell::Shell`: egui viewport
//! commands, the egui clipboard and the native (rfd) file dialog.

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use relay_ui::daemon_control;
use relay_ui::format::{avatar_problem, media_type_for};
use relay_ui::relay_link::resolve_endpoint;
use relay_ui::shell::{BoxFuture, FilePicker, PickedFile, Shell};

pub struct DesktopShell {
	ctx: egui::Context,
	tray: AtomicBool,
}

impl DesktopShell {
	pub fn new(ctx: egui::Context) -> Self {
		Self {
			ctx,
			tray: AtomicBool::new(false),
		}
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
				.set_title("Choose a profile image")
				.add_filter("PNG or JPEG image", &["png", "jpg", "jpeg"])
				.pick_file()
				.await;
			match dialog {
				Some(file) => read_picked(file.path()).await.map(Some),
				None => Ok(None),
			}
		})
	}
}

/// Check the size before reading so a huge file is never loaded.
pub async fn read_picked(path: &Path) -> Result<PickedFile, String> {
	let name = path.to_string_lossy().into_owned();
	let size = tokio::fs::metadata(path)
		.await
		.map_err(|e| format!("Cannot read the file: {e}"))?
		.len();
	let size = usize::try_from(size)
		.unwrap_or(usize::MAX)
		.min(relay_ui::controller::AVATAR_MAX_BYTES + 1);
	if let Some(problem) = avatar_problem(size, media_type_for(&name)) {
		return Err(problem.to_owned());
	}
	let bytes = tokio::fs::read(path)
		.await
		.map_err(|e| format!("Cannot read the file: {e}"))?;
	Ok(PickedFile { name, bytes })
}

impl Shell for DesktopShell {
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
		self.ctx
			.send_viewport_cmd(egui::ViewportCommand::Visible(true));
		self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
	}

	fn hide_window(&self) {
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
