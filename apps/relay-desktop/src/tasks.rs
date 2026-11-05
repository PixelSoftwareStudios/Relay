//! Background jobs started from the UI: avatar fetch, connection info, the
//! native file dialog and invite creation. Each runs on the tokio runtime,
//! drops its result into the [`Inbox`](crate::inbox::Inbox) and wakes the UI.

use crate::controller::AVATAR_MAX_BYTES;
use crate::format::{avatar_problem, media_type_for};
use crate::imaging::decode_for_texture;
use crate::inbox::{Invite, Picked};
use crate::shell::Actions;

impl Actions {
	/// Fetch and decode an avatar once; the UI caches the texture by hash.
	pub fn fetch_avatar(&self, hash: String) {
		let (controller, shared, ctx) = (
			self.controller.clone(),
			self.shared.clone(),
			self.ctx.clone(),
		);
		self.runtime.spawn(async move {
			let image = match controller.avatar_png(&hash).await {
				Ok(png) => tokio::task::spawn_blocking(move || decode_for_texture(&png))
					.await
					.ok()
					.flatten(),
				Err(_) => None,
			};
			shared.inbox.push_avatar(hash, image);
			ctx.request_repaint();
		});
	}

	/// Ask the engine for the connection string and addresses.
	pub fn fetch_connection(&self) {
		let (controller, shared, ctx) = (
			self.controller.clone(),
			self.shared.clone(),
			self.ctx.clone(),
		);
		self.runtime.spawn(async move {
			let result = controller
				.connection_info()
				.await
				.map_err(|e| e.to_string());
			shared.inbox.set_connection(result);
			ctx.request_repaint();
		});
	}

	/// Native file dialog, then a size check, read and bounded preview decode.
	pub fn pick_avatar(&self) {
		let (shared, ctx) = (self.shared.clone(), self.ctx.clone());
		self.runtime.spawn(async move {
			let dialog = rfd::AsyncFileDialog::new()
				.set_title("Choose a profile image")
				.add_filter("PNG or JPEG image", &["png", "jpg", "jpeg"])
				.pick_file()
				.await;
			if let Some(file) = dialog {
				match load_picked(file.path()).await {
					Ok(picked) => shared.inbox.set_picked(picked),
					Err(message) => shared.set_error(message),
				}
				ctx.request_repaint();
			}
		});
	}

	/// Create an invite; the token goes to the inbox, never to logs.
	pub fn create_invite(&self, channel_id: String, ttl_seconds: u32, uses: u32) {
		let (controller, shared, ctx) = (
			self.controller.clone(),
			self.shared.clone(),
			self.ctx.clone(),
		);
		self.runtime.spawn(async move {
			shared.clear_error();
			match controller
				.create_invite(&channel_id, ttl_seconds, uses)
				.await
			{
				Ok(token) => shared.inbox.set_invite(Invite { channel_id, token }),
				Err(e) => shared.set_error(e.to_string()),
			}
			ctx.request_repaint();
		});
	}
}

async fn load_picked(path: &std::path::Path) -> Result<Picked, String> {
	let media_type = media_type_for(&path.to_string_lossy());
	// Check the size before reading so a huge file is never loaded.
	let size = tokio::fs::metadata(path)
		.await
		.map_err(|e| format!("Cannot read the file: {e}"))?
		.len();
	let size = usize::try_from(size)
		.unwrap_or(usize::MAX)
		.min(AVATAR_MAX_BYTES + 1);
	if let Some(problem) = avatar_problem(size, media_type) {
		return Err(problem.to_owned());
	}
	let bytes = tokio::fs::read(path)
		.await
		.map_err(|e| format!("Cannot read the file: {e}"))?;
	let for_preview = bytes.clone();
	let preview = tokio::task::spawn_blocking(move || decode_for_texture(&for_preview))
		.await
		.ok()
		.flatten()
		.ok_or_else(|| "That file is not a readable PNG or JPEG image.".to_owned())?;
	Ok(Picked {
		bytes,
		media_type,
		preview,
	})
}
