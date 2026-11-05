//! Shared messaging state between the supervisor (writer), actions and the
//! UI thread. Kept apart from [`Hub`](crate::hub::Hub): transcripts can be
//! large, and `Hub::update` compares whole snapshots.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use tokio::sync::Notify;

use super::engine::MsgOps;
use super::state::{Feed, MessagesState};
use crate::imaging::Decoded;

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
	m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Decoded images waiting for the UI thread; older ones drop.
const MAX_QUEUED_IMAGES: usize = 32;

#[derive(Default)]
pub struct MsgHub {
	state: Mutex<MessagesState>,
	enabled: AtomicBool,
	/// Counts opt-in changes, so a connection can tell its opt-in was withdrawn.
	epoch: AtomicU64,
	/// The user switched messages off on purpose; opening the panel must not undo it.
	declined: AtomicBool,
	toggled: Notify,
	ops: Mutex<Option<Arc<dyn MsgOps>>>,
	images: Mutex<Vec<(String, Result<Decoded, String>)>>,
	/// Drafts whose send failed, given back to the compose box.
	returned: Mutex<Vec<(String, String)>>,
}

impl MsgHub {
	/// Read the state. Do not call back into the hub from `f`.
	pub fn read<R>(&self, f: impl FnOnce(&MessagesState) -> R) -> R {
		f(&lock(&self.state))
	}

	pub fn update<R>(&self, f: impl FnOnce(&mut MessagesState) -> R) -> R {
		f(&mut lock(&self.state))
	}

	/// Whether the user allows this app to hold the `messages` capability.
	pub fn enabled(&self) -> bool {
		self.enabled.load(Ordering::Acquire)
	}

	/// Opt in or out. Opting out drops every transcript held here at once.
	pub fn set_enabled(&self, on: bool) {
		self.declined.store(!on, Ordering::Release);
		if self.enabled.swap(on, Ordering::AcqRel) == on {
			return;
		}
		self.epoch.fetch_add(1, Ordering::AcqRel);
		if !on {
			*lock(&self.ops) = None;
			self.update(|s| {
				s.forget_all();
				s.feed = Feed::Off;
				s.notice = None;
			});
		} else {
			self.update(|s| s.feed = Feed::Connecting);
		}
		self.toggled.notify_one();
	}

	/// Opening the Messages panel opts in, unless the user turned it off on purpose.
	pub fn open_default(&self) {
		if !self.enabled() && !self.declined.load(Ordering::Acquire) {
			self.set_enabled(true);
		}
	}

	/// Changes whenever the opt-in changes.
	pub fn epoch(&self) -> u64 {
		self.epoch.load(Ordering::Acquire)
	}

	/// Resolves after the next [`MsgHub::set_enabled`] change (or at once if one is pending).
	pub async fn changed(&self) {
		self.toggled.notified().await;
	}

	/// Resolves once the opt-in differs from `epoch`, ignoring stale wake-ups.
	pub async fn changed_from(&self, epoch: u64) {
		while self.epoch() == epoch {
			self.toggled.notified().await;
		}
	}

	pub fn set_ops(&self, ops: Arc<dyn MsgOps>) {
		*lock(&self.ops) = Some(ops);
	}

	pub fn clear_ops(&self) {
		*lock(&self.ops) = None;
	}

	pub fn ops(&self) -> Option<Arc<dyn MsgOps>> {
		lock(&self.ops).clone()
	}

	pub fn set_notice(&self, text: Option<String>) {
		self.update(|s| s.notice = text);
	}

	pub fn push_image(&self, key: String, image: Result<Decoded, String>) {
		let mut q = lock(&self.images);
		if q.len() >= MAX_QUEUED_IMAGES {
			drop(q.remove(0));
		}
		q.push((key, image));
	}

	pub fn return_draft(&self, session: String, text: String) {
		let mut q = lock(&self.returned);
		q.retain(|(s, _)| *s != session);
		q.push((session, text));
	}

	pub fn take_returned(&self) -> Vec<(String, String)> {
		std::mem::take(&mut *lock(&self.returned))
	}

	pub fn take_images(&self) -> Vec<(String, Result<Decoded, String>)> {
		std::mem::take(&mut *lock(&self.images))
	}
}
