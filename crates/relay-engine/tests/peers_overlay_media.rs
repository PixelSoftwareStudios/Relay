//! Overlay media: the candidate's address is the authenticated link's remote IP, only with the
//! opt-in, and only for the exact overlay address of that link.

use std::net::{IpAddr, SocketAddr};

use relay_core::PeerId;
use relay_engine::peers::{
	Inbound, LinkNet, MediaSignal, PeerConfig, SessionTable, decode, encode_media,
};
use relay_net::proto::envelope::Body;

const OVERLAY: &str = "100.64.1.2";

fn ip(text: &str) -> IpAddr {
	text.parse().unwrap()
}

fn offer(addr: &str) -> Body {
	let candidates = vec![addr.parse::<SocketAddr>().unwrap()];
	encode_media(&MediaSignal::Offer {
		epoch: [3; 16],
		candidates,
	})
	.body
	.unwrap()
}

fn candidates(config: &PeerConfig, remote: &str, offered: &str) -> Vec<SocketAddr> {
	let remote = ip(remote);
	let net = LinkNet {
		remote_ip: remote,
		subnet: None,
	};
	let mut table = SessionTable::default();
	let peer = PeerId::from_bytes([1; 32]);
	match decode(
		peer,
		offer(offered),
		&mut table,
		&config.link_policy(remote),
		&net,
	) {
		Inbound::Media(MediaSignal::Offer { candidates, .. }) => candidates,
		other => panic!("unexpected {other:?}"),
	}
}

fn opted_in() -> PeerConfig {
	let mut config = PeerConfig::new("t");
	config.allow_overlay_manual = true;
	config
}

#[test]
fn overlay_link_with_opt_in_yields_the_link_address() {
	let config = opted_in();
	assert_eq!(config.overlay_peer(ip(OVERLAY)), Some(ip(OVERLAY)));
	let got = candidates(&config, OVERLAY, "100.64.1.2:40000");
	assert_eq!(got, vec!["100.64.1.2:40000".parse::<SocketAddr>().unwrap()]);
}

#[test]
fn default_setting_keeps_overlay_refused() {
	let config = PeerConfig::new("t");
	assert_eq!(config.overlay_peer(ip(OVERLAY)), None);
	assert!(candidates(&config, OVERLAY, "100.64.1.2:40000").is_empty());
}

#[test]
fn other_overlay_addresses_and_lan_links_are_not_widened() {
	let config = opted_in();
	// A different overlay host than the link's remote is never admitted.
	assert!(candidates(&config, OVERLAY, "100.64.9.9:40000").is_empty());
	// A LAN link grants no overlay exception.
	assert_eq!(config.overlay_peer(ip("192.168.1.9")), None);
	assert!(candidates(&config, "192.168.1.9", "100.64.1.2:40000").is_empty());
	// Public addresses stay refused even on an overlay link.
	assert!(candidates(&config, OVERLAY, "8.8.8.8:40000").is_empty());
}
