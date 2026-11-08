//! Ring budgets for callers nobody trusts. A key costs an attacker nothing, so the per-key
//! invite limit (`sessions.rs`) cannot stop a device that rotates keys. These budgets key on
//! what is not free: the source address, and the total across all strangers. Trusted contacts
//! never pass through here. All times are passed in, so tests drive a fake clock.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::time::{Duration, Instant};

/// Invitations one stranger source may send per window.
const PER_SOURCE: u32 = 6;
/// Invitations all strangers together may send per window.
const GLOBAL: u32 = 24;
const WINDOW: Duration = Duration::from_secs(60);
/// After the user declines a stranger, that source stays quiet this long.
const COOLDOWN: Duration = Duration::from_secs(15);
/// Sources remembered at once.
const SOURCES_MAX: usize = 256;

#[derive(Debug, Clone, Copy)]
struct Source {
	began: Instant,
	used: u32,
	quiet_until: Option<Instant>,
}

/// Per-source and global invite budgets for unknown callers.
#[derive(Debug, Default)]
pub struct RingBudget {
	sources: HashMap<IpAddr, Source>,
	global_began: Option<Instant>,
	global_used: u32,
}

impl RingBudget {
	/// Counts an invitation from an untrusted `source`; false means drop it unseen.
	pub fn admit(&mut self, source: IpAddr, now: Instant) -> bool {
		let key = source_key(source);
		if self.sources.len() >= SOURCES_MAX && !self.sources.contains_key(&key) {
			self.sources.retain(|_, s| s.live(now));
			if self.sources.len() >= SOURCES_MAX {
				return false;
			}
		}
		let entry = self.sources.entry(key).or_insert(Source {
			began: now,
			used: 0,
			quiet_until: None,
		});
		if entry.quiet_until.is_some_and(|until| now < until) {
			return false;
		}
		if now.saturating_duration_since(entry.began) >= WINDOW {
			entry.began = now;
			entry.used = 0;
		}
		if entry.used >= PER_SOURCE {
			return false;
		}
		if self
			.global_began
			.is_none_or(|b| now.saturating_duration_since(b) >= WINDOW)
		{
			self.global_began = Some(now);
			self.global_used = 0;
		}
		if self.global_used >= GLOBAL {
			return false;
		}
		entry.used += 1;
		self.global_used += 1;
		true
	}

	/// The user declined a stranger's call that came from `source`: it waits a moment.
	pub fn declined(&mut self, source: IpAddr, now: Instant) {
		let key = source_key(source);
		if self.sources.len() >= SOURCES_MAX && !self.sources.contains_key(&key) {
			self.sources.retain(|_, s| s.live(now));
		}
		if self.sources.len() < SOURCES_MAX || self.sources.contains_key(&key) {
			self.sources
				.entry(key)
				.or_insert(Source {
					began: now,
					used: 0,
					quiet_until: None,
				})
				.quiet_until = Some(now + COOLDOWN);
		}
	}
}

impl Source {
	fn live(&self, now: Instant) -> bool {
		now.saturating_duration_since(self.began) < WINDOW
			|| self.quiet_until.is_some_and(|u| now < u)
	}
}

/// An IPv4 address, or the /64 of an IPv6 one (a host owns the whole /64).
fn source_key(ip: IpAddr) -> IpAddr {
	match ip {
		IpAddr::V6(v6) => match v6.to_ipv4_mapped() {
			Some(v4) => IpAddr::V4(v4),
			None => {
				let mut octets = v6.octets();
				octets[8..].fill(0);
				IpAddr::V6(Ipv6Addr::from(octets))
			}
		},
		v4 => v4,
	}
}
