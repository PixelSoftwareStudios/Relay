//! Live messaging preferences.

use std::sync::Arc;

use arc_swap::ArcSwap;
use relay_config::prefs::{MAX_OFFER_SIZE_MIB, MAX_TRANSFER_RATE_MBPS, Preferences};

const MIB: u64 = 1024 * 1024;

/// The values the messaging layer reads on every decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MessagingConfig {
	/// `messages.enabled`.
	pub enabled: bool,
	/// `messages.max_offer_size_mib`.
	pub max_offer_size_mib: u32,
	/// `messages.transfer_rate_limit_mbps` (0 = unlimited).
	pub transfer_rate_limit_mbps: u32,
}

impl Default for MessagingConfig {
	fn default() -> Self {
		let defaults = relay_config::prefs::Messages::default();
		Self {
			enabled: defaults.enabled,
			max_offer_size_mib: defaults.max_offer_size_mib,
			transfer_rate_limit_mbps: defaults.transfer_rate_limit_mbps,
		}
	}
}

impl MessagingConfig {
	/// The stored preferences.
	pub fn from_prefs(prefs: &Preferences) -> Self {
		Self {
			enabled: prefs.messages.enabled,
			max_offer_size_mib: prefs.messages.max_offer_size_mib,
			transfer_rate_limit_mbps: prefs.messages.transfer_rate_limit_mbps,
		}
	}

	/// Largest file offered or accepted, in bytes.
	pub fn max_offer_bytes(self) -> u64 {
		u64::from(self.max_offer_size_mib) * MIB
	}

	/// Pacing cap of one transfer in bytes per second; `None` removes it.
	pub fn rate_bytes_per_sec(self) -> Option<u64> {
		(self.transfer_rate_limit_mbps > 0).then(|| u64::from(self.transfer_rate_limit_mbps) * MIB)
	}

	/// Range check shared by the API and the stored preferences.
	pub fn validate(self) -> Result<(), &'static str> {
		if !(1..=MAX_OFFER_SIZE_MIB).contains(&self.max_offer_size_mib) {
			return Err("max_offer_size_mib must be within 1..=4096");
		}
		if self.transfer_rate_limit_mbps > MAX_TRANSFER_RATE_MBPS {
			return Err("transfer_rate_limit_mbps must be within 0..=1024");
		}
		Ok(())
	}
}

/// A cell the API layer and the actor both read without asking the actor.
pub type SharedConfig = Arc<ArcSwap<MessagingConfig>>;

/// Persists changed messaging preferences.
pub trait ConfigStore: Send + Sync {
	/// Saves the values; an error leaves the live values unchanged.
	fn save(&self, config: MessagingConfig) -> Result<(), String>;
}

/// A store that keeps nothing (tests, fake adapters).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoStore;

impl ConfigStore for NoStore {
	fn save(&self, _: MessagingConfig) -> Result<(), String> {
		Ok(())
	}
}
