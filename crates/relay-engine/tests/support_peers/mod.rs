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

/// A hand-driven remote peer for protocol-abuse tests.
pub struct Raw {
	pub identity: Identity,
	pub sender: relay_net::control::ControlSender<tokio::io::WriteHalf<tokio::net::TcpStream>>,
	pub receiver: relay_net::control::ControlReceiver<tokio::io::ReadHalf<tokio::net::TcpStream>>,
}

impl Raw {
	/// Handshakes with `addr` and exchanges `Hello`.
	pub async fn connect(addr: std::net::SocketAddr, name: &str) -> Self {
		Self::connect_as(Identity::generate().unwrap(), addr, name).await
	}

	/// Like `connect`, with a chosen identity (a second link of the same device).
	pub async fn connect_as(identity: Identity, addr: std::net::SocketAddr, name: &str) -> Self {
		use relay_net::control::{control_channel, new_envelope, new_hello};
		use relay_net::handshake::initiate;
		use relay_net::proto::envelope::Body;
		let mut stream = tokio::net::TcpStream::connect(addr).await.unwrap();
		let session = Arc::new(initiate(&mut stream, &identity, None).await.unwrap());
		let (mut sender, mut receiver) = control_channel(stream, session);
		let hello = new_hello(&identity.fingerprint(), name, [9; 16]);
		sender
			.send(&new_envelope(Body::Hello(hello)))
			.await
			.unwrap();
		receiver.recv().await.unwrap();
		Self {
			identity,
			sender,
			receiver,
		}
	}

	pub fn id(&self) -> PeerId {
		peer_id(&self.identity.fingerprint())
	}

	pub async fn send(&mut self, body: relay_net::proto::envelope::Body) {
		self.sender
			.send(&relay_net::control::new_envelope(body))
			.await
			.unwrap();
	}

	/// The next body that is neither a keepalive nor a profile, if one arrives within `wait`.
	pub async fn next_body(&mut self, wait: Duration) -> Option<relay_net::proto::envelope::Body> {
		use relay_net::proto::envelope::Body;
		timeout(wait, async {
			loop {
				let env = self.receiver.recv().await.ok()?;
				match env.body? {
					Body::Keepalive(_) | Body::Profile(_) => continue,
					other => return Some(other),
				}
			}
		})
		.await
		.ok()
		.flatten()
	}
}

struct Silent;

impl relay_engine::peers::AdvertSink for Silent {
	fn publish(&self, _advert: Option<&relay_net::discovery::Advertisement>) {}
}

/// A peer that advertises (a client attached) unless `hidden`: strangers may call it.
/// The returned sender changes its visibility.
pub async fn visible_peer(
	name: &str,
	hidden: bool,
) -> (
	TestPeer,
	tokio::sync::watch::Sender<relay_engine::peers::AdvertSpec>,
) {
	use relay_engine::peers::{AdvertSpec, DiscoverySetup, SinkHandle};
	let (spec, rx) = tokio::sync::watch::channel(AdvertSpec {
		present: true,
		hidden,
		..AdvertSpec::default()
	});
	let mut config = fast_config(name);
	config.advert = Some(rx);
	config.discovery = Some(DiscoverySetup {
		advertise: true,
		sink: Some(SinkHandle(Arc::new(Silent))),
		..DiscoverySetup::default()
	});
	let identity = Arc::new(Identity::generate().unwrap());
	(peer_with(config, identity).await, spec)
}
