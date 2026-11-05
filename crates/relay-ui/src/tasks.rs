//! Background jobs started from the UI: avatar fetch, connection info, the
//! shell's file picker and invite creation. Each runs on the tokio runtime,
//! drops its result into the [`Inbox`](crate::inbox::Inbox) and wakes the UI.

use crate::actions::Actions;
use crate::format::{avatar_problem, media_type_for};
use crate::imaging::decode_for_texture;
use crate::inbox::{Invite, Picked};
use crate::shell::PickedFile;

impl Actions {
	/// Fetch and decode an avatar once; the UI caches the texture by hash.
	pub fn fetch_avatar(&self, hash: String) {
		let (controller, shared, shell) = (
			self.controller.clone(),
			self.shared.clone(),
			self.shell.clone(),
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
			shell.request_repaint();
		});
	}

	/// Ask the engine for the connection string and addresses.
	pub fn fetch_connection(&self) {
		let (controller, shared, shell) = (
			self.controller.clone(),
			self.shared.clone(),
			self.shell.clone(),
		);
		self.runtime.spawn(async move {
			let result = controller
				.connection_info()
				.await
				.map_err(|e| e.to_string());
			shared.inbox.set_connection(result);
			shell.request_repaint();
		});
	}

	/// The shell's picker, then a size check and bounded preview decode.
	pub fn pick_avatar(&self) {
		let (shared, shell) = (self.shared.clone(), self.shell.clone());
		self.runtime.spawn(async move {
			match shell.pick_image().await {
				Ok(Some(file)) => match load_picked(file).await {
					Ok(picked) => shared.inbox.set_picked(picked),
					Err(message) => shared.set_error(message),
				},
				Ok(None) => return,
				Err(message) => shared.set_error(message),
			}
			shell.request_repaint();
		});
	}

	/// Create an invite; the token goes to the inbox, never to logs.
	pub fn create_invite(&self, channel_id: String, ttl_seconds: u32, uses: u32) {
		let (controller, shared, shell) = (
			self.controller.clone(),
			self.shared.clone(),
			self.shell.clone(),
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
			shell.request_repaint();
		});
	}
}

async fn load_picked(file: PickedFile) -> Result<Picked, String> {
	let media_type = media_type_for(&file.name);
	if let Some(problem) = avatar_problem(file.bytes.len(), media_type) {
		return Err(problem.to_owned());
	}
	let bytes = file.bytes;
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
