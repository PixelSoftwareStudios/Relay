//! Token-bucket pacing of file bytes.
//!
//! Each data link has its own bucket (the per-transfer cap) and may share a second bucket with
//! every other link of the hub (the global cap). Time comes from `tokio::time`, so tests drive
//! it with a paused clock. The rate can be changed while a transfer runs, which is how the
//! engine slows files down during a call.

use relay_io::LockExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_messages::rate::TokenBucket;
use tokio::time::Instant;

use super::limits::DATA_CHUNK_MAX_BYTES;

/// Burst: at least two chunks, otherwise 50 ms of traffic.
fn burst_for(rate: u64) -> u64 {
	(rate / 20).max(2 * DATA_CHUNK_MAX_BYTES as u64)
}

#[derive(Debug)]
struct Limiter {
	bucket: TokenBucket,
	rate: u64,
	burst: u64,
}

impl Limiter {
	fn new(rate: u64, now_ms: u64) -> Self {
		let rate = rate.max(1);
		let burst = burst_for(rate);
		Self {
			bucket: TokenBucket::new(rate, burst, now_ms),
			rate,
			burst,
		}
	}

	/// Milliseconds until `n` bytes are available; 0 means available now.
	fn wait_ms(&mut self, n: u64, now_ms: u64) -> u64 {
		let n = n.min(self.burst);
		let avail = self.bucket.available(now_ms);
		if avail >= n {
			0
		} else {
			((n - avail) * 1000).div_ceil(self.rate).max(1)
		}
	}

	fn take(&mut self, n: u64, now_ms: u64) {
		let _ = self.bucket.try_take(n.min(self.burst), now_ms);
	}
}

/// A cap shared by several links.
#[derive(Clone, Debug)]
pub struct SharedLimit {
	start: Instant,
	inner: Arc<Mutex<Option<Limiter>>>,
}

impl SharedLimit {
	/// Unlimited until [`Self::set_rate`].
	pub fn unlimited() -> Self {
		Self {
			start: Instant::now(),
			inner: Arc::new(Mutex::new(None)),
		}
	}

	/// Sets or clears (`None`) the aggregate cap in bytes per second.
	pub fn set_rate(&self, rate: Option<u64>) {
		let now = elapsed_ms(self.start);
		*self.inner.lock_unpoisoned() = rate.map(|r| Limiter::new(r, now));
	}
}

fn elapsed_ms(start: Instant) -> u64 {
	u64::try_from(start.elapsed().as_millis()).unwrap_or(u64::MAX)
}

/// Paces one link: its own cap plus the hub-wide cap.
#[derive(Clone, Debug)]
pub struct Pacer {
	start: Instant,
	local: Arc<Mutex<Option<Limiter>>>,
	global: Option<SharedLimit>,
}

impl Pacer {
	/// `rate` is bytes per second; `None` means unpaced (apart from the global cap).
	pub fn new(rate: Option<u64>, global: Option<SharedLimit>) -> Self {
		let start = Instant::now();
		Self {
			start,
			local: Arc::new(Mutex::new(rate.map(|r| Limiter::new(r, 0)))),
			global,
		}
	}

	/// Changes this link's cap while it runs.
	pub fn set_rate(&self, rate: Option<u64>) {
		let now = elapsed_ms(self.start);
		*self.local.lock_unpoisoned() = rate.map(|r| Limiter::new(r, now));
	}

	/// Waits until `n` bytes may be sent, then charges both buckets.
	pub async fn acquire(&self, n: usize) {
		let n = n as u64;
		loop {
			let wait = self.try_charge(n);
			if wait == 0 {
				return;
			}
			tokio::time::sleep(Duration::from_millis(wait)).await;
		}
	}

	/// Charges and returns 0, or returns the milliseconds to wait without charging.
	fn try_charge(&self, n: u64) -> u64 {
		let now = elapsed_ms(self.start);
		let mut local = self.local.lock_unpoisoned();
		let mut shared = self.global.as_ref().map(|g| g.inner.lock_unpoisoned());
		let global_now = self.global.as_ref().map_or(0, |g| elapsed_ms(g.start));
		let wait_local = local.as_mut().map_or(0, |l| l.wait_ms(n, now));
		let wait_global = shared
			.as_mut()
			.and_then(|g| g.as_mut())
			.map_or(0, |l| l.wait_ms(n, global_now));
		let wait = wait_local.max(wait_global);
		if wait == 0 {
			if let Some(l) = local.as_mut() {
				l.take(n, now);
			}
			if let Some(l) = shared.as_mut().and_then(|g| g.as_mut()) {
				l.take(n, global_now);
			}
		}
		wait
	}
}
