//! What the media layer keeps so a stuck call can be diagnosed: a shared millisecond clock for
//! "seen in the last 500 ms" checks, the media path state and the device buffer sizes.

use std::net::SocketAddr;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

/// Milliseconds on a process-wide monotonic clock. Workers stamp packets with it; readers
/// compare against it. Costs one `Instant::now`, no allocation.
pub fn now_ms() -> u64 {
	static START: OnceLock<Instant> = OnceLock::new();
	let start = START.get_or_init(Instant::now);
	u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Where the UDP media path of a call stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PathState {
	/// Waiting for the remote's candidates or for a challenge to be echoed.
	Waiting,
	/// A path was validated; media flows over it.
	Connected,
	/// Validation or activation gave up; see the reason.
	Failed,
}

/// The media path of one call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PathStatus {
	/// State.
	pub state: PathState,
	/// Why it is waiting or failed; empty when connected.
	pub reason: String,
	/// Local socket address.
	pub local: Option<SocketAddr>,
	/// Validated (or candidate) remote address.
	pub remote: Option<SocketAddr>,
}

impl PathStatus {
	/// A path that is still being set up, with the thing it waits for.
	pub fn waiting(reason: &str) -> Self {
		Self {
			state: PathState::Waiting,
			reason: reason.to_owned(),
			local: None,
			remote: None,
		}
	}
}

/// Buffer sizes the device threads report when a stream opens.
#[derive(Debug, Default)]
pub struct DeviceBuffers {
	capture_frames: AtomicU32,
	capture_rate: AtomicU32,
	playback_frames: AtomicU32,
	playback_rate: AtomicU32,
}

impl DeviceBuffers {
	/// Records the microphone stream (`frames` is the callback or buffer size).
	pub fn set_capture(&self, frames: u32, rate: u32) {
		self.capture_frames.store(frames, Ordering::Relaxed);
		self.capture_rate.store(rate, Ordering::Relaxed);
	}

	/// Records the playback stream.
	pub fn set_playback(&self, frames: u32, rate: u32) {
		self.playback_frames.store(frames, Ordering::Relaxed);
		self.playback_rate.store(rate, Ordering::Relaxed);
	}

	/// `(frames, rate)` of the microphone stream; zeros while closed.
	pub fn capture(&self) -> (u32, u32) {
		(
			self.capture_frames.load(Ordering::Relaxed),
			self.capture_rate.load(Ordering::Relaxed),
		)
	}

	/// `(frames, rate)` of the playback stream; zeros while closed.
	pub fn playback(&self) -> (u32, u32) {
		(
			self.playback_frames.load(Ordering::Relaxed),
			self.playback_rate.load(Ordering::Relaxed),
		)
	}
}
