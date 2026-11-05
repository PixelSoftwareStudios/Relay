//! Shared host state: the latest view plus dirty flags for the emitter.

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use relay_io::LockExt;

use crate::model::{AppState, Meters};

/// Shared between the supervisor (writer), commands and the emitter.
#[derive(Default)]
pub struct Hub {
	state: Mutex<AppState>,
	meters: Mutex<Meters>,
	state_dirty: AtomicBool,
	meters_dirty: AtomicBool,
	/// Whether anyone can see the meters; hidden windows get none.
	hidden: AtomicBool,
}

impl Hub {
	/// Mutate the state; an emission is scheduled only if it changed, so a
	/// daemon that stays down does not wake the UI on every retry.
	pub fn update<R>(&self, f: impl FnOnce(&mut AppState) -> R) -> R {
		let mut guard = self.state.lock_unpoisoned();
		let before = guard.clone();
		let out = f(&mut guard);
		if *guard != before {
			self.state_dirty.store(true, Ordering::Release);
		}
		out
	}

	/// Read without scheduling anything.
	pub fn read<R>(&self, f: impl FnOnce(&AppState) -> R) -> R {
		f(&self.state.lock_unpoisoned())
	}

	pub fn state(&self) -> AppState {
		self.state.lock_unpoisoned().clone()
	}

	/// Meters are only produced while the window is visible.
	pub fn set_visible(&self, visible: bool) {
		self.hidden.store(!visible, Ordering::Release);
	}

	pub fn visible(&self) -> bool {
		!self.hidden.load(Ordering::Acquire)
	}

	pub fn set_meters(&self, meters: Meters) {
		if self.visible() {
			self.store_meters(meters);
		}
	}

	fn store_meters(&self, meters: Meters) {
		let mut guard = self.meters.lock_unpoisoned();
		let seq = guard.seq;
		*guard = Meters { seq, ..meters };
		self.meters_dirty.store(true, Ordering::Release);
	}

	pub fn clear_meters(&self) {
		self.store_meters(Meters::default());
	}

	pub fn meters(&self) -> Meters {
		self.meters.lock_unpoisoned().clone()
	}

	/// Next state to emit, with its sequence number, if anything changed.
	pub fn take_state(&self) -> Option<AppState> {
		if !self.state_dirty.swap(false, Ordering::AcqRel) {
			return None;
		}
		let mut guard = self.state.lock_unpoisoned();
		guard.seq += 1;
		Some(guard.clone())
	}

	pub fn take_meters(&self) -> Option<Meters> {
		if !self.meters_dirty.swap(false, Ordering::AcqRel) {
			return None;
		}
		let mut guard = self.meters.lock_unpoisoned();
		guard.seq += 1;
		Some(guard.clone())
	}
}
