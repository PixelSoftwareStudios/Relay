//! Overlay (100.64.0.0/10) media: the address comes from the authenticated link.

use relay_net::AddressPolicy;

use super::PeerConfig;

impl PeerConfig {
	/// The overlay address media may use for a link whose authenticated remote
	/// socket is `remote_ip`: only an address in 100.64.0.0/10, and only with
	/// the opt-in. Never derived from advertised, discovered or offered data.
	pub fn overlay_peer(&self, remote_ip: std::net::IpAddr) -> Option<std::net::IpAddr> {
		(self.allow_overlay_manual && relay_net::is_shared_address_space(remote_ip))
			.then_some(remote_ip)
	}

	/// The policy for media candidates of a link whose remote socket is `remote_ip`.
	pub fn link_policy(&self, remote_ip: std::net::IpAddr) -> AddressPolicy {
		match self.overlay_peer(remote_ip) {
			Some(ip) => self.policy.with_overlay_peer(ip),
			None => self.policy,
		}
	}
}
