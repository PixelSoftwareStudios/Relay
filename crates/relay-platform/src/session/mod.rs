//! Session events that must drop push-to-talk, and OS lock detection.
//!
//! Detection coverage:
//! - macOS: screen lock by polling the console session dictionary. Suspend is not
//!   detected here.
//! - Linux, Windows: not implemented. Lease expiry is the backstop: a key-up lost to
//!   a lock or suspend leaves the lease to lapse within `lease_ms` (300 ms default).

#[cfg(target_os = "macos")]
mod macos;

use tokio::sync::mpsc::UnboundedSender;

/// Something that must stop transmission.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionEvent {
	/// Screen locked.
	Locked,
	/// Screen unlocked.
	Unlocked,
	/// System is about to sleep.
	Suspending,
	/// System woke up.
	Resumed,
	/// The window or terminal that owns PTT lost focus.
	FocusLost,
	/// Focus returned.
	FocusGained,
	/// The engine connection dropped.
	Disconnected,
}

/// Whether lock detection works on this OS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LockDetection {
	/// Lock changes are reported.
	Detected,
	/// Not detected; lease expiry is the only protection.
	Unsupported(&'static str),
}

/// Report lock-detection coverage honestly.
pub fn lock_detection() -> LockDetection {
	#[cfg(target_os = "macos")]
	return LockDetection::Detected;
	#[cfg(not(target_os = "macos"))]
	LockDetection::Unsupported("no lock detection on this OS; relying on lease expiry")
}

/// Stops the lock monitor on drop.
pub struct LockMonitor {
	stop: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

impl Drop for LockMonitor {
	fn drop(&mut self) {
		self.stop.store(true, std::sync::atomic::Ordering::SeqCst);
	}
}

/// Start sending [`SessionEvent::Locked`]/[`SessionEvent::Unlocked`] on changes.
/// Returns `None` where [`lock_detection`] is `Unsupported`.
pub fn spawn_lock_monitor(tx: UnboundedSender<SessionEvent>) -> Option<LockMonitor> {
	#[cfg(target_os = "macos")]
	return Some(macos::spawn(tx));
	#[cfg(not(target_os = "macos"))]
	{
		drop(tx);
		None
	}
}
