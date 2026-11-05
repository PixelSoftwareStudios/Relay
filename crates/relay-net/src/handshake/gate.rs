//! Admission of unauthenticated connections: a global cap, a per-source cap and eviction.
//!
//! A source is an IPv4 address or an IPv6 /64 (IPv4-mapped addresses are unwrapped), so one host
//! cannot hold every slot, and when the gate is full the oldest slot of the busiest source is
//! evicted instead of refusing a newcomer. An evicted handshake sees [`HandshakePermit::evicted`]
//! complete and must abandon the connection; [`super::respond_gated`] does that.

use std::collections::HashMap;
use std::net::{IpAddr, Ipv6Addr};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::sync::Notify;

use super::HandshakeError;
use crate::limits::{PENDING_HANDSHAKES_MAX, PENDING_HANDSHAKES_PER_SOURCE};

pub(crate) type SourceKey = (u8, [u8; 8]);

/// Groups addresses that one host can cheaply rotate through.
pub(crate) fn source_key(ip: IpAddr) -> SourceKey {
	let ip = match ip {
		IpAddr::V6(v6) => v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4),
		v4 => v4,
	};
	match ip {
		IpAddr::V4(v4) => {
			let mut key = [0u8; 8];
			key[..4].copy_from_slice(&v4.octets());
			(4, key)
		}
		IpAddr::V6(v6) => (6, v6_prefix(v6)),
	}
}

fn v6_prefix(ip: Ipv6Addr) -> [u8; 8] {
	ip.octets()[..8].try_into().expect("8 bytes")
}

struct Slot {
	source: SourceKey,
	since: Instant,
	evicted: Arc<Flag>,
}

#[derive(Default)]
struct Flag {
	set: AtomicBool,
	notify: Notify,
}

#[derive(Default)]
struct Slots {
	next: u64,
	slots: HashMap<u64, Slot>,
}

/// Caps concurrent unauthenticated handshakes. Acquire before any expensive work.
#[derive(Clone)]
pub struct HandshakeGate {
	state: Arc<Mutex<Slots>>,
	capacity: usize,
	per_source: usize,
}

impl std::fmt::Debug for HandshakeGate {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("HandshakeGate")
			.field("capacity", &self.capacity)
			.finish_non_exhaustive()
	}
}

/// Holds one pending-handshake slot until dropped.
pub struct HandshakePermit {
	id: u64,
	flag: Arc<Flag>,
	state: Arc<Mutex<Slots>>,
}

impl std::fmt::Debug for HandshakePermit {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("HandshakePermit")
	}
}

impl HandshakePermit {
	/// Completes when this slot was evicted to make room for a newcomer.
	pub async fn evicted(&self) {
		loop {
			let notified = self.flag.notify.notified();
			if self.flag.set.load(Ordering::Acquire) {
				return;
			}
			notified.await;
		}
	}

	pub fn is_evicted(&self) -> bool {
		self.flag.set.load(Ordering::Acquire)
	}
}

impl Drop for HandshakePermit {
	fn drop(&mut self) {
		self.state
			.lock()
			.unwrap_or_else(|p| p.into_inner())
			.slots
			.remove(&self.id);
	}
}

impl HandshakeGate {
	pub fn new() -> Self {
		Self::with_limits(PENDING_HANDSHAKES_MAX, PENDING_HANDSHAKES_PER_SOURCE)
	}

	pub fn with_capacity(capacity: usize) -> Self {
		Self::with_limits(capacity, PENDING_HANDSHAKES_PER_SOURCE)
	}

	pub fn with_limits(capacity: usize, per_source: usize) -> Self {
		Self {
			state: Arc::default(),
			capacity,
			per_source,
		}
	}

	/// Takes a slot for a connection from `source`. Refused when that source already holds its
	/// share; otherwise a full gate evicts the oldest slot of the busiest source.
	pub fn try_enter(&self, source: IpAddr) -> Result<HandshakePermit, HandshakeError> {
		let key = source_key(source);
		let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
		let held = state.slots.values().filter(|s| s.source == key).count();
		if held >= self.per_source || self.capacity == 0 {
			return Err(HandshakeError::Overloaded);
		}
		if state.slots.len() >= self.capacity {
			let mut counts: HashMap<SourceKey, usize> = HashMap::new();
			for slot in state.slots.values() {
				*counts.entry(slot.source).or_default() += 1;
			}
			let victim = state
				.slots
				.iter()
				.max_by_key(|(_, s)| (counts[&s.source], std::cmp::Reverse(s.since)))
				.map(|(id, _)| *id);
			if let Some(slot) = victim.and_then(|id| state.slots.remove(&id)) {
				slot.evicted.set.store(true, Ordering::Release);
				slot.evicted.notify.notify_waiters();
			}
		}
		let id = state.next;
		state.next += 1;
		let flag = Arc::new(Flag::default());
		state.slots.insert(
			id,
			Slot {
				source: key,
				since: Instant::now(),
				evicted: Arc::clone(&flag),
			},
		);
		Ok(HandshakePermit {
			id,
			flag,
			state: Arc::clone(&self.state),
		})
	}

	pub fn in_use(&self) -> usize {
		self.state
			.lock()
			.unwrap_or_else(|p| p.into_inner())
			.slots
			.len()
	}
}

impl Default for HandshakeGate {
	fn default() -> Self {
		Self::new()
	}
}
