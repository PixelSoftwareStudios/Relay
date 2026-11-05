//! Poison-tolerant locking.
//!
//! A panicked worker must not wedge the daemon, so Relay recovers the guard
//! from a poisoned lock instead of propagating the panic. These traits state
//! that policy once.

use std::sync::{Mutex, MutexGuard, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// `Mutex` locking that ignores poisoning (`PoisonError::into_inner`).
pub trait LockExt<T: ?Sized> {
	/// Locks, recovering the guard if a previous holder panicked.
	fn lock_unpoisoned(&self) -> MutexGuard<'_, T>;
}

impl<T: ?Sized> LockExt<T> for Mutex<T> {
	fn lock_unpoisoned(&self) -> MutexGuard<'_, T> {
		self.lock().unwrap_or_else(|e| e.into_inner())
	}
}

/// `RwLock` locking that ignores poisoning (`PoisonError::into_inner`).
pub trait RwLockExt<T: ?Sized> {
	/// Read-locks, recovering the guard if a writer panicked.
	fn read_unpoisoned(&self) -> RwLockReadGuard<'_, T>;
	/// Write-locks, recovering the guard if a previous holder panicked.
	fn write_unpoisoned(&self) -> RwLockWriteGuard<'_, T>;
}

impl<T: ?Sized> RwLockExt<T> for RwLock<T> {
	fn read_unpoisoned(&self) -> RwLockReadGuard<'_, T> {
		self.read().unwrap_or_else(|e| e.into_inner())
	}

	fn write_unpoisoned(&self) -> RwLockWriteGuard<'_, T> {
		self.write().unwrap_or_else(|e| e.into_inner())
	}
}
