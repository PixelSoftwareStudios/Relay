//! User-tweakable storage and retention limits (`limits.*`, `docs/limits.md`).
//!
//! One table, [`SPECS`], holds every key with its default, range and unit; the struct, the
//! validation and the `relay limits` listing all read it, so a default lives in one place.
//! Security guards and protocol invariants are deliberately not here.

use serde::{Deserialize, Serialize};

use crate::avatar::limits::CACHE_MAX_BYTES;
use crate::history::{MAX_AGE_MS, MAX_ENTRIES};

const MIB: u64 = 1024 * 1024;
const DAY_MS: u64 = 24 * 60 * 60 * 1000;

/// One tweakable limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LimitSpec {
	/// Preference key (`limits.avatar_cache_mib`).
	pub key: &'static str,
	/// Value used until the user changes it.
	pub default: u32,
	/// Smallest accepted value.
	pub min: u32,
	/// Largest accepted value.
	pub max: u32,
	/// Unit of the value (`MiB`, `entries`, `days`, `transfers`).
	pub unit: &'static str,
}

/// Number of tweakable limits.
pub const COUNT: usize = 9;

/// Every tweakable limit, in display order.
pub const SPECS: [LimitSpec; COUNT] = [
	LimitSpec {
		key: "limits.avatar_cache_mib",
		default: (CACHE_MAX_BYTES / MIB) as u32,
		min: 16,
		max: 2048,
		unit: "MiB",
	},
	LimitSpec {
		key: "limits.messages_session_mib",
		default: 64,
		min: 8,
		max: 1024,
		unit: "MiB",
	},
	LimitSpec {
		key: "limits.messages_retained_mib",
		default: 256,
		min: 8,
		max: 1024,
		unit: "MiB",
	},
	LimitSpec {
		key: "limits.history_max_entries",
		default: MAX_ENTRIES as u32,
		min: 0,
		max: 5000,
		unit: "entries",
	},
	LimitSpec {
		key: "limits.history_max_age_days",
		default: (MAX_AGE_MS / DAY_MS) as u32,
		min: 1,
		max: 3650,
		unit: "days",
	},
	LimitSpec {
		key: "limits.transfer_concurrency",
		default: 2,
		min: 1,
		max: 8,
		unit: "transfers",
	},
	LimitSpec {
		key: "limits.messages_retained_calls",
		default: 3,
		min: 0,
		max: 20,
		unit: "calls",
	},
	LimitSpec {
		key: "limits.ringtone_max_kib",
		default: 1024,
		min: 64,
		max: 8192,
		unit: "KiB",
	},
	LimitSpec {
		key: "limits.ringtone_max_seconds",
		default: 30,
		min: 2,
		max: 120,
		unit: "seconds",
	},
];

/// The spec of `key`, if it is a tweakable limit.
pub fn spec(key: &str) -> Option<&'static LimitSpec> {
	SPECS.iter().find(|s| s.key == key)
}

/// Storage and retention limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Limits {
	/// Size bound of the disposable avatar cache.
	pub avatar_cache_mib: u32,
	/// Memory budget of one live session's chat transcript.
	pub messages_session_mib: u32,
	/// Memory budget of all ended chat transcripts together.
	pub messages_retained_mib: u32,
	/// Most call-history entries kept.
	pub history_max_entries: u32,
	/// Oldest call-history entry kept, in days.
	pub history_max_age_days: u32,
	/// File transfers running at once in one session.
	pub transfer_concurrency: u32,
	/// Most ended calls whose chat transcript is kept; `0` keeps nothing once a call ends.
	pub messages_retained_calls: u32,
	/// Largest custom ringtone file accepted, in KiB.
	pub ringtone_max_kib: u32,
	/// Longest custom ringtone accepted, in seconds.
	pub ringtone_max_seconds: u32,
}

impl Default for Limits {
	fn default() -> Self {
		let d = |i: usize| SPECS[i].default;
		Self {
			avatar_cache_mib: d(0),
			messages_session_mib: d(1),
			messages_retained_mib: d(2),
			history_max_entries: d(3),
			history_max_age_days: d(4),
			transfer_concurrency: d(5),
			messages_retained_calls: d(6),
			ringtone_max_kib: d(7),
			ringtone_max_seconds: d(8),
		}
	}
}

impl Limits {
	/// The values in [`SPECS`] order.
	pub fn values(&self) -> [u32; COUNT] {
		[
			self.avatar_cache_mib,
			self.messages_session_mib,
			self.messages_retained_mib,
			self.history_max_entries,
			self.history_max_age_days,
			self.transfer_concurrency,
			self.messages_retained_calls,
			self.ringtone_max_kib,
			self.ringtone_max_seconds,
		]
	}

	/// Reads the limits out of a flat `key -> text` map (`GetSettings`); a missing or
	/// unparsable entry keeps its default.
	pub fn from_map(map: &std::collections::BTreeMap<String, String>) -> Self {
		let mut values = Self::default().values();
		for (slot, spec) in values.iter_mut().zip(&SPECS) {
			if let Some(value) = map.get(spec.key).and_then(|v| v.parse().ok()) {
				*slot = value;
			}
		}
		let [
			avatar,
			session,
			retained,
			entries,
			days,
			transfers,
			calls,
			ring_kib,
			ring_secs,
		] = values;
		Self {
			avatar_cache_mib: avatar,
			messages_session_mib: session,
			messages_retained_mib: retained,
			history_max_entries: entries,
			history_max_age_days: days,
			transfer_concurrency: transfers,
			messages_retained_calls: calls,
			ringtone_max_kib: ring_kib,
			ringtone_max_seconds: ring_secs,
		}
	}

	/// The first problem with the values, naming the key, or `None`.
	pub fn check(&self) -> Option<String> {
		for (spec, value) in SPECS.iter().zip(self.values()) {
			if !(spec.min..=spec.max).contains(&value) {
				return Some(format!(
					"{} must be within {}..={} {}",
					spec.key, spec.min, spec.max, spec.unit
				));
			}
		}
		if self.messages_retained_mib < self.messages_session_mib {
			return Some(format!(
				"limits.messages_retained_mib ({}) must not be smaller than limits.messages_session_mib ({})",
				self.messages_retained_mib, self.messages_session_mib
			));
		}
		None
	}

	/// Avatar cache bound in bytes.
	pub fn avatar_cache_bytes(&self) -> u64 {
		u64::from(self.avatar_cache_mib) * MIB
	}

	/// One session transcript budget in bytes.
	pub fn session_bytes(&self) -> u64 {
		u64::from(self.messages_session_mib) * MIB
	}

	/// Ended-transcript budget in bytes.
	pub fn retained_bytes(&self) -> u64 {
		u64::from(self.messages_retained_mib) * MIB
	}

	/// Ended chat transcripts kept.
	pub fn retained_calls(&self) -> usize {
		self.messages_retained_calls as usize
	}

	/// Largest custom ringtone file in bytes.
	pub fn ringtone_max_bytes(&self) -> usize {
		self.ringtone_max_kib as usize * 1024
	}

	/// Maximum call-history age in milliseconds.
	pub fn history_max_age_ms(&self) -> u64 {
		u64::from(self.history_max_age_days) * DAY_MS
	}
}
