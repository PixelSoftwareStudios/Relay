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

/// What a daemon job does.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Job {
	Start,
	Stop,
	Restart,
}

impl Actions {
	/// Start the standalone daemon (only when the user clicks). The window
	/// reconnects by itself once the endpoint is up.
	pub fn start_daemon(&self) {
		self.shared.autostart.user_start();
		self.daemon_job(Job::Start);
	}

	/// Ask the daemon to stop: calls end and the microphone is released. This
	/// is not [`Actions::quit_app`], which leaves the daemon running.
	pub fn quit_daemon(&self) {
		// Quitting on purpose is not a crash: nothing restarts it this session.
		self.shared.autostart.user_quit();
		self.daemon_job(Job::Stop);
	}

	/// Stop the daemon and start it again, so a newer build takes over. The
	/// same control path as Start and Quit; calls end, so the UI offers it
	/// only when none is active.
	pub fn restart_daemon(&self) {
		self.shared.autostart.user_quit();
		self.daemon_job(Job::Restart);
	}

	/// Read the system's secure storage again (the "Try again" button of the start-up card).
	pub fn retry_secure_storage(&self) {
		self.storage_job(false);
	}

	/// Keep secrets in the file-backed store (after the user confirmed it is less secure).
	pub fn use_file_storage(&self) {
		self.storage_job(true);
	}

	/// Delete the local database with no daemon involved, then start the daemon again. Shares
	/// the daemon busy flag, so it cannot overlap a start or stop.
	pub fn reset_local_data(&self) {
		use std::sync::atomic::Ordering;
		if self.shared.daemon_busy.swap(true, Ordering::AcqRel) {
			return;
		}
		let (shared, shell, hub) = (self.shared.clone(), self.shell.clone(), self.hub.clone());
		self.runtime.spawn(async move {
			shared.clear_error();
			match shell.reset_local_data().await {
				Ok(()) => {
					shared.autostart.user_start();
					match shell.start_daemon().await {
						Ok(_) => hub.update(|s| s.notice = Some(tr!("reset-done").to_owned())),
						Err(message) => shared.set_error(message),
					}
				}
				Err(message) => shared.set_error(message),
			}
			shared.daemon_busy.store(false, Ordering::Release);
			shell.request_repaint();
		});
	}

	fn storage_job(&self, file_store: bool) {
		let (shared, shell, hub) = (self.shared.clone(), self.shell.clone(), self.hub.clone());
		self.runtime.spawn(async move {
			shared.clear_error();
			let result = if file_store {
				shell.use_file_storage().await
			} else {
				shell.retry_secure_storage().await
			};
			match result {
				// Calm again at once; the next poll shows what the service really says.
				Ok(()) => hub.update(|s| {
					if let Some(startup) = &mut s.startup {
						startup.failed = false;
					}
				}),
				Err(message) => shared.set_error(message),
			}
			shell.request_repaint();
		});
	}

	fn daemon_job(&self, job: Job) {
		use std::sync::atomic::Ordering;
		if self.shared.daemon_busy.swap(true, Ordering::AcqRel) {
			return;
		}
		let (shared, shell, hub) = (self.shared.clone(), self.shell.clone(), self.hub.clone());
		self.runtime.spawn(async move {
			shared.clear_error();
			let stopped = match job {
				Job::Start => Ok(()),
				Job::Stop | Job::Restart => shell.stop_daemon().await,
			};
			let result = match (job, stopped) {
				(_, Err(message)) => Err(message),
				(Job::Stop, Ok(())) => Ok(tr!("daemon-stopped").to_owned()),
				(Job::Start | Job::Restart, Ok(())) => {
					if job == Job::Restart {
						shared.autostart.user_start();
					}
					shell.start_daemon().await
				}
			};
			match result {
				Ok(note) if note.is_empty() => {}
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
			if let Ok(values) = &result {
				shell.hotkey_accelerator(&values.accelerator);
			}
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
				if let Ok(values) = &reread {
					shell.hotkey_accelerator(&values.accelerator);
				}
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
	/// pasted connection string and then trusts the device (`connect_and_trust`).
	pub fn scan_qr(&self) {
		let (shared, shell) = (self.shared.clone(), self.shell.clone());
		self.runtime.spawn(async move {
			let result = shell.scan_qr().await;
			shared.inbox.set_scanned(result);
			shell.request_repaint();
		});
	}

	/// Dial a scanned connection string and trust the device when its key matches
	/// the fingerprint in the code. The outcome goes to the inbox.
	pub fn connect_and_trust(&self, text: String) {
		use crate::controller::ControlError;
		use crate::qr_scan::ScanTrust;
		let (controller, shared, shell) = (
			self.controller.clone(),
			self.shared.clone(),
			self.shell.clone(),
		);
		self.runtime.spawn(async move {
			let result = match controller.connect_and_trust(&text).await {
				Ok(()) => ScanTrust::Added(ScanTrust::digest_of(&text)),
				Err(ControlError::Engine(message)) => ScanTrust::from_failure(&message),
				Err(other) => ScanTrust::Failed(other.to_string()),
			};
			shared.inbox.set_scan_trusted(result);
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
	let (bytes, preview) = tokio::task::spawn_blocking(move || {
		let preview = decode_for_texture(&bytes);
		(bytes, preview)
	})
	.await
	.ok()
	.ok_or_else(|| tr!("err-unreadable-image").to_owned())?;
	let preview = preview.ok_or_else(|| tr!("err-unreadable-image").to_owned())?;
	Ok(Picked {
		bytes,
		media_type,
		preview,
	})
}
