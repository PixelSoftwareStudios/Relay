//! Failed-attempt accounting for admission. Sans-IO: callers pass a monotonic `Duration`.
//!
//! Per-peer lockout alone is bypassable because a LAN attacker can mint unlimited Noise
//! identities, so a per-channel failure budget (and a per-peer one across channels) backs it.
//! The channel budget lets attackers lock legitimate joiners out (review F-12), so while it
//! cools down a separate reserved lane admits identities with a clean record (no failure on
//! any channel in the window, no entry for the pair). Each attacker identity can use the lane
//! once per window, so the lane is not drained by a few identities retrying, and it adds at
//! most `RESERVE_FAILURES` guesses per cool-down to the channel's ceiling.

use std::collections::{HashMap, VecDeque};
use std::time::Duration;

use relay_core::channel::ChannelId;

use super::limits::{
	CHANNEL_COOLDOWN, CHANNEL_FAILURES, FAILURE_WINDOW, LIMITER_ENTRIES_MAX, LOCKOUT_BASE,
	LOCKOUT_MAX, PEER_CHANNEL_FAILURES, PEER_FAILURES, RESERVE_FAILURES,
};
use crate::identity::Fingerprint;

/// Bucket for attempts on channels the device does not admit for. It has a per-peer lockout
/// like any channel but no shared budget, so it cannot be used to probe for existence.
pub const UNKNOWN_CHANNEL: ChannelId = ChannelId::from_bytes([0; 16]);

/// Admission is refused until `retry_after` has passed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Locked {
	pub retry_after: Duration,
}

#[derive(Default)]
struct Window {
	failures: VecDeque<Duration>,
	locked_until: Duration,
}

impl Window {
	fn prune(&mut self, now: Duration) {
		while self
			.failures
			.front()
			.is_some_and(|t| now.saturating_sub(*t) >= FAILURE_WINDOW)
		{
			self.failures.pop_front();
		}
	}

	fn record(&mut self, now: Duration, max: usize) {
		self.prune(now);
		self.failures.push_back(now);
		if self.failures.len() >= max {
			self.locked_until = now + CHANNEL_COOLDOWN;
			self.failures.clear();
		}
	}

	fn remaining(&self, now: Duration) -> Option<Duration> {
		self.locked_until.checked_sub(now).filter(|d| !d.is_zero())
	}

	fn idle(&self, now: Duration) -> bool {
		self.failures.is_empty() && self.remaining(now).is_none()
	}
}

#[derive(Default)]
struct PairEntry {
	failures: u32,
	strikes: u32,
	/// The last charge for this pair went to the reserved lane, not the channel window.
	reserved: bool,
	locked_until: Duration,
	last_failure: Duration,
}

#[derive(Default)]
pub struct AttemptLimiter {
	pairs: HashMap<(Fingerprint, ChannelId), PairEntry>,
	channels: HashMap<ChannelId, Window>,
	reserve: HashMap<ChannelId, Window>,
	peers: HashMap<Fingerprint, Window>,
}

impl AttemptLimiter {
	pub fn new() -> Self {
		Self::default()
	}

	/// Whether `peer` may attempt `channel` now.
	pub fn check(
		&mut self,
		now: Duration,
		peer: &Fingerprint,
		channel: &ChannelId,
	) -> Result<(), Locked> {
		let mut longest = Duration::ZERO;
		if let Some(entry) = self.pairs.get(&(*peer, *channel)) {
			longest = longest.max(entry.locked_until.saturating_sub(now));
		}
		if let Some(window) = self.channels.get(channel) {
			let cooling = window.remaining(now).unwrap_or_default();
			if !cooling.is_zero() && self.eligible(now, peer, channel) {
				let lane = self.reserve.get(channel).and_then(|w| w.remaining(now));
				longest = longest.max(lane.unwrap_or_default());
			} else {
				longest = longest.max(cooling);
			}
		}
		if let Some(window) = self.peers.get(peer) {
			longest = longest.max(window.remaining(now).unwrap_or_default());
		}
		if longest.is_zero() {
			Ok(())
		} else {
			Err(Locked {
				retry_after: longest,
			})
		}
	}

	/// Whether `peer` has a clean record: no failure anywhere in the window and no lockout.
	fn eligible(&self, now: Duration, peer: &Fingerprint, channel: &ChannelId) -> bool {
		if *channel == UNKNOWN_CHANNEL {
			return false;
		}
		let pair_clean = self.pairs.get(&(*peer, *channel)).is_none_or(|e| {
			e.locked_until <= now && now.saturating_sub(e.last_failure) >= FAILURE_WINDOW
		});
		let peer_clean = self.peers.get(peer).is_none_or(|w| {
			w.remaining(now).is_none()
				&& w.failures
					.iter()
					.all(|t| now.saturating_sub(*t) >= FAILURE_WINDOW)
		});
		pair_clean && peer_clean
	}

	pub fn record_failure(&mut self, now: Duration, peer: &Fingerprint, channel: &ChannelId) {
		self.make_room(now);
		let on_reserve = self
			.channels
			.get(channel)
			.is_some_and(|w| w.remaining(now).is_some())
			&& self.eligible(now, peer, channel);
		let entry = self.pairs.entry((*peer, *channel)).or_default();
		entry.failures += 1;
		entry.last_failure = now;
		entry.reserved = on_reserve;
		if entry.failures >= PEER_CHANNEL_FAILURES {
			entry.strikes = entry.strikes.saturating_add(1);
			entry.failures = 0;
			let factor = 1u32 << entry.strikes.saturating_sub(1).min(16);
			entry.locked_until = now + (LOCKOUT_BASE * factor).min(LOCKOUT_MAX);
		}
		if on_reserve {
			self.reserve
				.entry(*channel)
				.or_default()
				.record(now, RESERVE_FAILURES);
		} else if *channel != UNKNOWN_CHANNEL {
			self.channels
				.entry(*channel)
				.or_default()
				.record(now, CHANNEL_FAILURES);
		}
		self.peers
			.entry(*peer)
			.or_default()
			.record(now, PEER_FAILURES);
	}

	/// Refunds the attempt that [`Self::record_failure`] charged up front for a join that then
	/// completed, and clears the pair's strikes. Earlier failures stay in the windows, so an
	/// attacker cannot reset the shared budgets by also succeeding somewhere.
	pub fn record_success(&mut self, peer: &Fingerprint, channel: &ChannelId) {
		let reserved = self
			.pairs
			.remove(&(*peer, *channel))
			.is_some_and(|e| e.reserved);
		let shared = if reserved {
			self.reserve.get_mut(channel)
		} else {
			self.channels.get_mut(channel)
		};
		if let Some(window) = shared {
			window.failures.pop_back();
		}
		if let Some(window) = self.peers.get_mut(peer) {
			window.failures.pop_back();
		}
	}

	/// Keeps memory bounded: drops idle entries, then the oldest pairs.
	fn make_room(&mut self, now: Duration) {
		if self.pairs.len() < LIMITER_ENTRIES_MAX {
			return;
		}
		self.pairs.retain(|_, e| {
			e.locked_until > now || now.saturating_sub(e.last_failure) < FAILURE_WINDOW
		});
		self.channels.retain(|_, w| {
			w.prune(now);
			!w.idle(now)
		});
		self.reserve.retain(|_, w| {
			w.prune(now);
			!w.idle(now)
		});
		self.peers.retain(|_, w| {
			w.prune(now);
			!w.idle(now)
		});
		while self.pairs.len() >= LIMITER_ENTRIES_MAX {
			let oldest = self
				.pairs
				.iter()
				.min_by_key(|(_, e)| e.last_failure)
				.map(|(k, _)| *k);
			match oldest {
				Some(key) => self.pairs.remove(&key),
				None => break,
			};
		}
	}
}
