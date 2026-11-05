//! Round-trip time of a control link, measured with its authenticated keepalives.
//!
//! The writer stamps each keepalive it sends, the reader matches the peer's echo (same nonce,
//! `reply` set) against the last stamp. Both travel inside the Noise session, so a third party
//! can neither forge nor replay a sample. The probe shares the TCP control connection with
//! signaling, so the figure includes queueing behind control traffic: it is an upper estimate
//! of the media path, never a measurement of it.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

/// Smoothing: the new sample counts one eighth.
const WEIGHT: u64 = 8;

/// Shared by the writer and reader tasks of one link.
#[derive(Debug)]
pub struct RttCell {
	base: Instant,
	sent_nonce: AtomicU64,
	sent_micros: AtomicU64,
	/// Smoothed round trip in microseconds; 0 until the first echo.
	smoothed: AtomicU64,
}

impl PartialEq for RttCell {
	fn eq(&self, other: &Self) -> bool {
		std::ptr::eq(self, other)
	}
}

impl Eq for RttCell {}

impl Default for RttCell {
	fn default() -> Self {
		Self {
			base: Instant::now(),
			sent_nonce: AtomicU64::new(0),
			sent_micros: AtomicU64::new(0),
			smoothed: AtomicU64::new(0),
		}
	}
}

impl RttCell {
	fn now_micros(&self) -> u64 {
		u64::try_from(self.base.elapsed().as_micros()).unwrap_or(u64::MAX)
	}

	/// A keepalive with this nonce is being sent now.
	pub fn probe_sent(&self, nonce: u64) {
		self.sent_micros.store(self.now_micros(), Ordering::Relaxed);
		self.sent_nonce.store(nonce, Ordering::Release);
	}

	/// The peer echoed `nonce`. Echoes of anything but the latest probe are ignored.
	pub fn probe_echoed(&self, nonce: u64) {
		if nonce == 0 || self.sent_nonce.load(Ordering::Acquire) != nonce {
			return;
		}
		let sample = self
			.now_micros()
			.saturating_sub(self.sent_micros.load(Ordering::Relaxed))
			.max(1);
		let old = self.smoothed.load(Ordering::Relaxed);
		let next = if old == 0 {
			sample
		} else {
			(old * (WEIGHT - 1) + sample) / WEIGHT
		};
		self.smoothed.store(next.max(1), Ordering::Relaxed);
		// One echo answers one probe.
		self.sent_nonce.store(0, Ordering::Release);
	}

	/// The smoothed round trip, once an echo has arrived.
	pub fn get(&self) -> Option<Duration> {
		match self.smoothed.load(Ordering::Relaxed) {
			0 => None,
			micros => Some(Duration::from_micros(micros)),
		}
	}
}

impl super::PeerHandle {
	/// Control-link round trip to a linked peer, if a keepalive echo has been timed. The
	/// timer is read live; the sample itself is as fresh as the last keepalive echo
	/// (every keepalive interval, 5 s by default).
	pub fn rtt(&self, peer: relay_core::PeerId) -> Option<Duration> {
		self.peers()
			.iter()
			.find(|p| p.id == Some(peer))
			.and_then(|p| p.probe.as_ref()?.get())
	}
}
