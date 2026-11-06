//! A device seen only through discovery has no key until a handshake, so the manager
//! dials it; without that, People stays empty although both devices see each other.

mod support_peers;

use std::sync::Arc;
use std::time::Duration;

use relay_engine::peers::{
	AdvertSink, DiscoverySetup, Hint, HintSender, PeerOutput, SinkHandle, hint_channel,
};
use relay_net::discovery::{Advertisement, PlatformRecord};
use support_peers::{TestPeer, expect, fast_config, peer, peer_with};
use tokio::time::{sleep, timeout};

struct Silent;

impl AdvertSink for Silent {
	fn publish(&self, _advert: Option<&Advertisement>) {}
}

async fn browsing(name: &str) -> (TestPeer, HintSender) {
	let (hints, source) = hint_channel();
	let mut config = fast_config(name);
	config.discovery = Some(DiscoverySetup {
		sink: Some(SinkHandle(Arc::new(Silent))),
		hints: Some(source),
		..DiscoverySetup::default()
	});
	let identity = Arc::new(relay_net::identity::Identity::generate().unwrap());
	(peer_with(config, identity).await, hints)
}

/// The shape Android's `NsdManager` delivered for a Mac on the same Wi-Fi: a renamed
/// instance ("(4)"), an empty `caps` and a link-local address next to the IPv4 one.
fn record_of(other: &TestPeer) -> PlatformRecord {
	let fingerprint = other.identity.fingerprint();
	let txt = |k: &str, v: String| (k.to_owned(), v);
	PlatformRecord {
		fullname: "relay-e56eece53ac6 (4)._praxis-relay._tcp.local.".to_owned(),
		port: other.handle.local_addr().port(),
		addresses: vec!["127.0.0.1".parse().unwrap()],
		txt: vec![
			txt("v", "1".to_owned()),
			txt("fp", hex::encode(&fingerprint.as_bytes()[..10])),
			txt("id", "e56eece53ac60de86c104075090f947c".to_owned()),
			txt("caps", String::new()),
			txt("n", "Dantes-Laptop.local".to_owned()),
		],
	}
}

#[tokio::test]
async fn a_discovered_device_is_met_and_listed() {
	let (a, hints) = browsing("alpha").await;
	let mut b = peer("bravo").await;
	hints.send(Hint::Found(record_of(&b)));
	let seen = expect(&mut b.output, |o| match o {
		PeerOutput::TrustRequired { name, peer, .. } => Some((name.clone(), *peer)),
		_ => None,
	})
	.await;
	assert_eq!(seen, ("alpha".to_owned(), a.id()));
	timeout(Duration::from_secs(5), async {
		loop {
			if a.handle.peers().iter().any(|p| p.id == Some(b.id())) {
				break;
			}
			sleep(Duration::from_millis(20)).await;
		}
	})
	.await
	.expect("the discovered device never got an id");
	let listed = a.handle.peers();
	let entry = listed.iter().find(|p| p.id == Some(b.id())).unwrap();
	assert!(!entry.linked, "met, not trusted");
	assert_eq!(listed.iter().filter(|p| p.id.is_none()).count(), 0);
}

#[tokio::test]
async fn our_own_record_is_not_dialled() {
	let (a, hints) = browsing("alpha").await;
	let mut b = peer("bravo").await;
	// The record points at b but carries a's own key hint, as a device's own
	// advertisement does when it browses its own network.
	let mut record = record_of(&a);
	record.port = b.handle.local_addr().port();
	hints.send(Hint::Found(record));
	let seen = timeout(Duration::from_millis(600), async {
		loop {
			if let Some(PeerOutput::TrustRequired { .. }) = b.output.recv().await {
				return;
			}
		}
	})
	.await;
	assert!(seen.is_err(), "own record was dialled");
}
