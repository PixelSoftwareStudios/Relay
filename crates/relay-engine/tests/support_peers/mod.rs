#![allow(dead_code)]

use std::sync::Arc;
use std::time::Duration;

use relay_core::PeerId;
use relay_engine::peers::{
	BackoffConfig, LinkTiming, PeerConfig, PeerHandle, PeerOutput, SharedTrust, peer_id, spawn,
};
use relay_net::AddressPolicy;
use relay_net::identity::Identity;
use tokio::sync::mpsc;
use tokio::time::timeout;

pub struct TestPeer {
	pub identity: Arc<Identity>,
	pub handle: PeerHandle,
	pub output: mpsc::Receiver<PeerOutput>,
	pub trust: SharedTrust,
}

impl TestPeer {
	pub fn id(&self) -> PeerId {
		peer_id(&self.identity.fingerprint())
	}
}

pub fn fast_config(name: &str) -> PeerConfig {
	let mut config = PeerConfig::new(name);
	config.listen = "127.0.0.1:0".parse().unwrap();
	config.policy = AddressPolicy::lan_and_loopback();
	config.discovery = None;
	config.timing = LinkTiming {
		keepalive: Duration::from_millis(100),
		idle: Duration::from_millis(600),
		hello: Duration::from_secs(2),
	};
	config.backoff = BackoffConfig {
		base: Duration::from_millis(20),
		max: Duration::from_millis(200),
	};
	config
}

pub async fn peer(name: &str) -> TestPeer {
	peer_with(fast_config(name), Arc::new(Identity::generate().unwrap())).await
}

pub async fn peer_with(config: PeerConfig, identity: Arc<Identity>) -> TestPeer {
	let trust = SharedTrust::in_memory();
	let (handle, output) = spawn(config, Arc::clone(&identity), trust.clone(), [7; 16])
		.await
		.unwrap();
	TestPeer {
		identity,
		handle,
		output,
		trust,
	}
}

/// Waits for an output matching `pick`, discarding others.
pub async fn expect<T>(
	output: &mut mpsc::Receiver<PeerOutput>,
	mut pick: impl FnMut(&PeerOutput) -> Option<T>,
) -> T {
	timeout(Duration::from_secs(5), async {
		loop {
			let item = output.recv().await.expect("output closed");
			if let Some(found) = pick(&item) {
				return found;
			}
		}
	})
	.await
	.expect("timed out waiting for peer output")
}

/// Brings up a pinned, trusted link between `a` and `b` (a dials b).
pub async fn linked_pair() -> (TestPeer, TestPeer) {
	use relay_core::PeerEvent;
	use relay_net::identity::TrustState;
	let mut a = peer("alpha").await;
	let mut b = peer("bravo").await;
	let dial = a.handle.dial(b.handle.local_addr(), None);
	let (found, ()) = tokio::join!(async { dial.await.unwrap() }, async {
		expect(&mut b.output, |o| {
			matches!(o, PeerOutput::TrustRequired { .. }).then_some(())
		})
		.await;
	});
	assert_eq!(found, b.id());
	b.handle
		.pin(a.id(), TrustState::UnverifiedFirstUse)
		.await
		.unwrap();
	expect(&mut b.output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::LinkUp(_))).then_some(())
	})
	.await;
	a.handle
		.pin(b.id(), TrustState::UnverifiedFirstUse)
		.await
		.unwrap();
	expect(&mut a.output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::LinkUp(_))).then_some(())
	})
	.await;
	(a, b)
}
