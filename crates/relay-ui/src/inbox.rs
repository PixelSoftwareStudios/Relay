//! Results of background work waiting for the UI thread to pick up. Every
//! slot is bounded; the UI drains it in `logic()` and then holds textures.

use std::sync::Mutex;

use relay_io::LockExt;

use relay_client::Secret;

use crate::engine::SettingResult;
use crate::hotkey_settings::HotkeyValues;
use crate::imaging::Decoded;
use crate::limits_settings::Loaded;
use crate::model::ConnectionView;

/// A profile image the user chose, ready to preview and upload.
pub struct Picked {
	pub bytes: Vec<u8>,
	pub media_type: &'static str,
	pub preview: Decoded,
}

/// A freshly minted invite; `Debug` redacts it.
pub struct Invite {
	pub channel_id: String,
	pub token: Secret,
}

#[derive(Default)]
pub struct Inbox {
	avatars: Mutex<Vec<(String, Option<Decoded>)>>,
	connection: Mutex<Option<Result<ConnectionView, String>>>,
	picked: Mutex<Option<Picked>>,
	invite: Mutex<Option<Invite>>,
	hotkey_loaded: Mutex<Option<Result<HotkeyValues, String>>>,
	hotkey_saved: Mutex<Option<Result<Vec<SettingResult>, String>>>,
	history_saved: Mutex<Option<Result<Vec<SettingResult>, String>>>,
	limits_loaded: Mutex<Option<Result<Loaded, String>>>,
	limits_saved: Mutex<Option<Result<Vec<SettingResult>, String>>>,
}

/// Decoded avatars queued before the UI catches up; older results drop.
const MAX_QUEUED_AVATARS: usize = 32;

impl Inbox {
	pub fn push_avatar(&self, hash: String, image: Option<Decoded>) {
		let mut q = self.avatars.lock_unpoisoned();
		if q.len() >= MAX_QUEUED_AVATARS {
			q.remove(0);
		}
		q.push((hash, image));
	}

	pub fn take_avatars(&self) -> Vec<(String, Option<Decoded>)> {
		std::mem::take(&mut *self.avatars.lock_unpoisoned())
	}

	pub fn set_connection(&self, result: Result<ConnectionView, String>) {
		*self.connection.lock_unpoisoned() = Some(result);
	}

	pub fn take_connection(&self) -> Option<Result<ConnectionView, String>> {
		self.connection.lock_unpoisoned().take()
	}

	pub fn set_picked(&self, picked: Picked) {
		*self.picked.lock_unpoisoned() = Some(picked);
	}

	pub fn take_picked(&self) -> Option<Picked> {
		self.picked.lock_unpoisoned().take()
	}

	pub fn set_invite(&self, invite: Invite) {
		*self.invite.lock_unpoisoned() = Some(invite);
	}

	pub fn set_hotkey_loaded(&self, result: Result<HotkeyValues, String>) {
		*self.hotkey_loaded.lock_unpoisoned() = Some(result);
	}

	pub fn take_hotkey_loaded(&self) -> Option<Result<HotkeyValues, String>> {
		self.hotkey_loaded.lock_unpoisoned().take()
	}

	pub fn set_hotkey_saved(&self, result: Result<Vec<SettingResult>, String>) {
		*self.hotkey_saved.lock_unpoisoned() = Some(result);
	}

	pub fn take_hotkey_saved(&self) -> Option<Result<Vec<SettingResult>, String>> {
		self.hotkey_saved.lock_unpoisoned().take()
	}

	pub fn set_history_saved(&self, result: Result<Vec<SettingResult>, String>) {
		*self.history_saved.lock_unpoisoned() = Some(result);
	}

	pub fn take_history_saved(&self) -> Option<Result<Vec<SettingResult>, String>> {
		self.history_saved.lock_unpoisoned().take()
	}

	pub fn set_limits_loaded(&self, result: Result<Loaded, String>) {
		*self.limits_loaded.lock_unpoisoned() = Some(result);
	}

	pub fn take_limits_loaded(&self) -> Option<Result<Loaded, String>> {
		self.limits_loaded.lock_unpoisoned().take()
	}

	pub fn set_limits_saved(&self, result: Result<Vec<SettingResult>, String>) {
		*self.limits_saved.lock_unpoisoned() = Some(result);
	}

	pub fn take_limits_saved(&self) -> Option<Result<Vec<SettingResult>, String>> {
		self.limits_saved.lock_unpoisoned().take()
	}

	pub fn take_invite(&self) -> Option<Invite> {
		self.invite.lock_unpoisoned().take()
	}
}
