//! Names a peer claims are display data. When two different keys claim names that
//! look the same, each is shown with its short fingerprint, so a claimed name never
//! stands in for the key.

use relay_net::discovery::name_skeleton;
use relay_net::identity::Fingerprint;

use super::manager::Actor;

/// `name`, with the short fingerprint appended when another key claims a look-alike.
pub fn disambiguated(name: &str, own: &Fingerprint, rivals: &[Fingerprint]) -> String {
	if rivals.iter().all(|rival| rival == own) {
		name.to_owned()
	} else {
		format!("{name} ({})", own.short())
	}
}

impl Actor {
	/// The name to show for `peer`: trusted records and live links of other keys that
	/// carry the same skeleton make it ambiguous.
	pub(super) fn shown_name(&self, peer: &Fingerprint, name: &str) -> String {
		let mut rivals: Vec<Fingerprint> = self
			.trust
			.lock()
			.find_by_name(name)
			.into_iter()
			.map(|r| r.fingerprint)
			.collect();
		let skeleton = name_skeleton(name);
		rivals.extend(
			self.canonical
				.iter()
				.filter(|(fp, _)| *fp != peer)
				.filter(|(_, id)| {
					self.links
						.get(*id)
						.is_some_and(|e| name_skeleton(&e.link.name) == skeleton)
				})
				.map(|(fp, _)| *fp),
		);
		disambiguated(name, peer, &rivals)
	}
}
