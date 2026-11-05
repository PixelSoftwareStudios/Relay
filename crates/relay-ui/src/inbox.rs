//! Results of background work waiting for the UI thread to pick up. Every
//! slot is bounded; the UI drains it in `logic()` and then holds textures.

use std::sync::{Mutex, MutexGuard};

use relay_client::Secret;

use crate::imaging::Decoded;
use crate::model::ConnectionView;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
	m.lock().unwrap_or_else(|e| e.into_inner())
}

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
}

/// Decoded avatars queued before the UI catches up; older results drop.
const MAX_QUEUED_AVATARS: usize = 32;

impl Inbox {
	pub fn push_avatar(&self, hash: String, image: Option<Decoded>) {
		let mut q = lock(&self.avatars);
		if q.len() >= MAX_QUEUED_AVATARS {
			q.remove(0);
		}
		q.push((hash, image));
	}

	pub fn take_avatars(&self) -> Vec<(String, Option<Decoded>)> {
		std::mem::take(&mut *lock(&self.avatars))
	}

	pub fn set_connection(&self, result: Result<ConnectionView, String>) {
		*lock(&self.connection) = Some(result);
	}

	pub fn take_connection(&self) -> Option<Result<ConnectionView, String>> {
		lock(&self.connection).take()
	}

	pub fn set_picked(&self, picked: Picked) {
		*lock(&self.picked) = Some(picked);
	}

	pub fn take_picked(&self) -> Option<Picked> {
		lock(&self.picked).take()
	}

	pub fn set_invite(&self, invite: Invite) {
		*lock(&self.invite) = Some(invite);
	}

	pub fn take_invite(&self) -> Option<Invite> {
		lock(&self.invite).take()
	}
}
