//! Background jobs started from the UI: avatar fetch, connection info, the
//! shell's file picker and invite creation. Each runs on the tokio runtime,
//! drops its result into the [`Inbox`](crate::inbox::Inbox) and wakes the UI.

use crate::actions::Actions;
use crate::hotkey_settings::HotkeyValues;
use crate::imaging::{avatar_check, decode_for_texture};
use crate::inbox::{Invite, Picked};
use crate::limits_settings::LimitValues;
use crate::shell::PickedFile;
use crate::tr;

impl Actions {
	/// Start the standalone daemon (only when the user clicks). The window
	/// reconnects by itself once the endpoint is up.
	pub fn start_daemon(&self) {
		self.shared.autostart.user_start();
		self.daemon_job(false);
	}

	/// Ask the daemon to stop: calls end and the microphone is released. This
	/// is not [`Actions::quit_app`], which leaves the daemon running.
	pub fn quit_daemon(&self) {
		// Quitting on purpose is not a crash: nothing restarts it this session.
		self.shared.autostart.user_quit();
		self.daemon_job(true);
	}

	fn daemon_job(&self, stop: bool) {
		use std::sync::atomic::Ordering;
		if self.shared.daemon_busy.swap(true, Ordering::AcqRel) {
			return;
		}
		let (shared, shell, hub) = (self.shared.clone(), self.shell.clone(), self.hub.clone());
		self.runtime.spawn(async move {
			shared.clear_error();
			let result = if stop {
				shell
					.stop_daemon()
					.await
					.map(|()| tr!("daemon-stopped").to_owned())
			} else {
				shell.start_daemon().await
			};
			match result {
				Ok(note) => hub.update(|s| s.notice = Some(note)),
				Err(message) => shared.set_error(message),
			}
			shared.daemon_busy.store(false, Ordering::Release);
			shell.request_repaint();
		});
	}

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

	/// Read the hotkey preferences (`GetSettings`) for the settings panel.
	pub fn fetch_hotkey_settings(&self) {
		let (controller, shared, shell) = (
			self.controller.clone(),
			self.shared.clone(),
			self.shell.clone(),
		);
		self.runtime.spawn(async move {
			let result = controller
				.hotkey_settings()
				.await
				.map_err(|e| e.to_string());
			shared.inbox.set_hotkey_loaded(result);
			shell.request_repaint();
		});
	}

	/// Send the changed hotkey keys (`SetSettings`), then read them back.
	pub fn save_hotkey_settings(&self, stored: HotkeyValues, wanted: HotkeyValues) {
		let (controller, shared, shell) = (
			self.controller.clone(),
			self.shared.clone(),
			self.shell.clone(),
		);
		self.runtime.spawn(async move {
			let result = controller
				.save_hotkey_settings(&stored, &wanted)
				.await
				.map_err(|e| e.to_string());
			let sent = result.is_ok();
			shared.inbox.set_hotkey_saved(result);
			if sent {
				let reread = controller.hotkey_settings().await;
				shared
					.inbox
					.set_hotkey_loaded(reread.map_err(|e| e.to_string()));
			}
			shell.request_repaint();
		});
	}

	/// Read the storage limits (`GetSettings`) for the settings panel.
	pub fn fetch_limits(&self) {
		let (controller, shared, shell) = (
			self.controller.clone(),
			self.shared.clone(),
			self.shell.clone(),
		);
		self.runtime.spawn(async move {
			let result = controller
				.limits_settings()
				.await
				.map_err(|e| e.to_string());
			shared.inbox.set_limits_loaded(result);
			shell.request_repaint();
		});
	}

	/// Send the changed limits (`SetSettings`), then read them back.
	pub fn save_limits(&self, stored: LimitValues, wanted: LimitValues) {
		let (controller, shared, shell) = (
			self.controller.clone(),
			self.shared.clone(),
			self.shell.clone(),
		);
		self.runtime.spawn(async move {
			let result = controller
				.save_limits(&stored, &wanted)
				.await
				.map_err(|e| e.to_string());
			let sent = result.is_ok();
			shared.inbox.set_limits_saved(result);
			if sent {
				let reread = controller.limits_settings().await;
				shared
					.inbox
					.set_limits_loaded(reread.map_err(|e| e.to_string()));
			}
			shell.request_repaint();
		});
	}

	/// Turn call history recording on or off (`SetSettings`); the answer goes to the inbox.
	pub fn save_history_enabled(&self, enabled: bool) {
		let (controller, shared, shell) = (
			self.controller.clone(),
			self.shared.clone(),
			self.shell.clone(),
		);
		self.runtime.spawn(async move {
			let result = controller
				.set_history_enabled(enabled)
				.await
				.map_err(|e| e.to_string());
			shared.inbox.set_history_saved(result);
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

	/// The shell's QR scanner. The text goes to the inbox; the UI checks it like a
	/// pasted connection string and only fills in the address field.
	pub fn scan_qr(&self) {
		let (shared, shell) = (self.shared.clone(), self.shell.clone());
		self.runtime.spawn(async move {
			let result = shell.scan_qr().await;
			shared.inbox.set_scanned(result);
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
	let media_type = avatar_check(&file.bytes)?;
	let bytes = file.bytes;
	let for_preview = bytes.clone();
	let preview = tokio::task::spawn_blocking(move || decode_for_texture(&for_preview))
		.await
		.ok()
		.flatten()
		.ok_or_else(|| tr!("err-unreadable-image").to_owned())?;
	Ok(Picked {
		bytes,
		media_type,
		preview,
	})
}
