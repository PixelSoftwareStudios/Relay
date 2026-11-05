//! Canonical link election when two links to the same peer exist, e.g. after
//! simultaneous dialling. Both sides evaluate the same pure rule on facts both
//! can see, so they keep the same link without negotiating.

use relay_net::identity::Fingerprint;

/// What an engine knows about one established link.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LinkFacts {
	/// Whether the local engine dialled this link.
	pub dialled_locally: bool,
	/// The link epoch both sides learned from the handshake preface.
	pub epoch: [u8; 16],
}

/// Which of two candidate links survives.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
	/// Keep the first argument.
	KeepFirst,
	/// Keep the second argument.
	KeepSecond,
}

/// The link dialled by the peer with the lower fingerprint wins. If both were
/// dialled by the same side, the higher epoch wins; identical facts keep the
/// existing (first) link. Closing the loser never ends a call: signaling is
/// addressed by peer, not by link.
pub fn elect(
	local: &Fingerprint,
	remote: &Fingerprint,
	first: LinkFacts,
	second: LinkFacts,
) -> Verdict {
	let dialler = |facts: LinkFacts| {
		if facts.dialled_locally {
			*local.as_bytes()
		} else {
			*remote.as_bytes()
		}
	};
	let (a, b) = (dialler(first), dialler(second));
	if a != b {
		return if a < b {
			Verdict::KeepFirst
		} else {
			Verdict::KeepSecond
		};
	}
	if second.epoch > first.epoch {
		Verdict::KeepSecond
	} else {
		Verdict::KeepFirst
	}
}
