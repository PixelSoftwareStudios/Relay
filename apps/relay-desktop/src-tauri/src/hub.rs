//! Shared host state: the latest view plus dirty flags for the emitter.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};

use crate::model::{AppState, Meters};

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
	m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Shared between the supervisor (writer), commands and the emitter.
#[derive(Default)]
pub struct Hub {
	state: Mutex<AppState>,
	meters: Mutex<Meters>,
	state_dirty: AtomicBool,
	meters_dirty: AtomicBool,
}

impl Hub {
	/// Mutate the state and schedule an emission.
	pub fn update<R>(&self, f: impl FnOnce(&mut AppState) -> R) -> R {
		let out = f(&mut lock(&self.state));
		self.state_dirty.store(true, Ordering::Release);
		out
	}

	/// Read without scheduling anything.
	pub fn read<R>(&self, f: impl FnOnce(&AppState) -> R) -> R {
		f(&lock(&self.state))
	}

	pub fn state(&self) -> AppState {
		lock(&self.state).clone()
	}

	pub fn set_meters(&self, meters: Meters) {
		let mut guard = lock(&self.meters);
		let seq = guard.seq;
		*guard = Meters { seq, ..meters };
		self.meters_dirty.store(true, Ordering::Release);
	}

	pub fn clear_meters(&self) {
		self.set_meters(Meters::default());
	}

	pub fn meters(&self) -> Meters {
		lock(&self.meters).clone()
	}

	/// Next state to emit, with its sequence number, if anything changed.
	pub fn take_state(&self) -> Option<AppState> {
		if !self.state_dirty.swap(false, Ordering::AcqRel) {
			return None;
		}
		let mut guard = lock(&self.state);
		guard.seq += 1;
		Some(guard.clone())
	}

	pub fn take_meters(&self) -> Option<Meters> {
		if !self.meters_dirty.swap(false, Ordering::AcqRel) {
			return None;
		}
		let mut guard = lock(&self.meters);
		guard.seq += 1;
		Some(guard.clone())
	}
}
