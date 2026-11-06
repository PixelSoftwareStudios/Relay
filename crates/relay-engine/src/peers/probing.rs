//! Meeting discovered devices: a discovery record carries only a short key hint, so a
//! device has no peer id (and is not listed, nor callable) until a handshake has shown
//! its key. The manager therefore dials each unlinked discovered device once in a while;
//! the link that results is untrusted and waits for the user's decision like any other.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use relay_net::identity::Fingerprint;

use super::manager::Actor;

/// Pause before the same discovered instance is dialled again after an attempt.
const PROBE_RETRY: Duration = Duration::from_secs(30);
/// Most unauthenticated dials running at once.
const PROBES_MAX: usize = 8;

/// When each discovered instance (by service full name) was last dialled.
pub(super) type Probed = HashMap<String, Instant>;

impl Actor {
	/// Dials discovered devices that are not ours, not linked and not tried recently.
	/// Records without a key hint (hidden devices) cannot be told from a linked peer
	/// and are left alone.
	pub(super) fn probe_discovered(&mut self) {
		let now = Instant::now();
		let mut running = self.dialling.values().filter(|e| e.is_none()).count();
		let due: Vec<_> = self
			.table
			.iter()
			.filter(|found| {
				found.fingerprint_hint.is_some_and(|hint| {
					!hint.matches(&self.local) && !self.linked_by_hint(|fp| hint.matches(fp))
				})
			})
			.filter(|found| {
				self.probed
					.get(&found.fullname)
					.is_none_or(|at| now.saturating_duration_since(*at) >= PROBE_RETRY)
			})
			.map(|found| (found.fullname.clone(), found.addresses.clone()))
			.collect();
		for (name, addresses) in due {
			if running >= PROBES_MAX {
				break;
			}
			if addresses.is_empty() {
				continue;
			}
			self.probed.insert(name, now);
			self.dial(addresses, None);
			running += 1;
		}
	}

	fn linked_by_hint(&self, matches: impl Fn(&Fingerprint) -> bool) -> bool {
		self.canonical.keys().any(matches)
	}

	/// Forgets attempt times of instances that left the table.
	pub(super) fn prune_probed(&mut self) {
		let live: std::collections::HashSet<&str> =
			self.table.iter().map(|f| f.fullname.as_str()).collect();
		self.probed.retain(|name, _| live.contains(name.as_str()));
	}
}
