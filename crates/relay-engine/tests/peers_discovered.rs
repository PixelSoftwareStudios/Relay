//! A device seen only through discovery has no key until a handshake, so the manager
//! dials it; without that, People stays empty although both devices see each other.

mod support_peers;

use std::sync::Arc;
use std::time::Duration;

use relay_engine::peers::{
	AdvertSink, AdvertSpec, DiscoverySetup, Hint, HintSender, PeerOutput, SinkHandle, hint_channel,
};
use relay_net::discovery::{Advertisement, PlatformRecord};
use support_peers::{TestPeer, expect, fast_config, peer, peer_with};
use tokio::net::TcpListener;
use tokio::sync::watch;
use tokio::time::{sleep, timeout};

struct Silent;

impl AdvertSink for Silent {
	fn publish(&self, _advert: Option<&Advertisement>) {}
}

/// A device whose visibility the test controls: attached (present) and not private.
async fn browsing_as(name: &str, present: bool, hidden: bool) -> Browser {
	let (hints, source) = hint_channel();
	let (spec, rx) = watch::channel(AdvertSpec {
		present,
		hidden,
		..AdvertSpec::default()
	});
	let mut config = fast_config(name);
	config.advert = Some(rx);
	config.discovery = Some(DiscoverySetup {
		advertise: true,
		sink: Some(SinkHandle(Arc::new(Silent))),
		hints: Some(source),
		..DiscoverySetup::default()
	});
	let identity = Arc::new(relay_net::identity::Identity::generate().unwrap());
	Browser {
		peer: peer_with(config, identity).await,
		hints,
		spec,
	}
}

struct Browser {
	peer: TestPeer,
	hints: HintSender,
	spec: watch::Sender<AdvertSpec>,
}

async fn contacted(b: &mut TestPeer, wait: Duration) -> bool {
	timeout(wait, async {
		loop {
			if let Some(PeerOutput::TrustRequired { .. }) = b.output.recv().await {
				return;
			}
		}
	})
	.await
	.is_ok()
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
	let Browser {
		peer: a,
		hints,
		spec: _spec,
	} = browsing_as("alpha", true, false).await;
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
	let Browser {
		peer: a,
		hints,
		spec: _spec,
	} = browsing_as("alpha", true, false).await;
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

#[tokio::test]
async fn nothing_is_dialled_while_no_client_is_attached() {
	let a = browsing_as("alpha", false, false).await;
	let mut b = peer("bravo").await;
	a.hints.send(Hint::Found(record_of(&b)));
	assert!(!contacted(&mut b, Duration::from_millis(600)).await);
	// A client attaching makes the device visible, and what is known is probed at once.
	a.spec.send_modify(|spec| spec.present = true);
	assert!(contacted(&mut b, Duration::from_secs(3)).await);
}

#[tokio::test]
async fn nothing_is_dialled_in_private_mode() {
	let a = browsing_as("alpha", true, true).await;
	let mut b = peer("bravo").await;
	a.hints.send(Hint::Found(record_of(&b)));
	assert!(!contacted(&mut b, Duration::from_millis(600)).await);
	a.spec.send_modify(|spec| spec.hidden = false);
	assert!(contacted(&mut b, Duration::from_secs(3)).await);
}

#[tokio::test]
async fn going_private_stops_new_dials_but_keeps_links() {
	let a = browsing_as("alpha", true, false).await;
	let mut b = peer("bravo").await;
	a.hints.send(Hint::Found(record_of(&b)));
	assert!(contacted(&mut b, Duration::from_secs(3)).await);
	a.spec.send_modify(|spec| spec.hidden = true);
	sleep(Duration::from_millis(200)).await;
	assert!(
		a.peer.handle.peers().iter().any(|p| p.id == Some(b.id())),
		"an established link survives going private"
	);
}

/// A listener that counts connections and never answers.
async fn silent_listener() -> (std::net::SocketAddr, Arc<std::sync::atomic::AtomicUsize>) {
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();
	let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
	let seen = Arc::clone(&count);
	tokio::spawn(async move {
		let mut held = Vec::new();
		while let Ok((stream, _)) = listener.accept().await {
			seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
			held.push(stream);
		}
	});
	(addr, count)
}

fn numbered(addr: std::net::SocketAddr, n: u8) -> PlatformRecord {
	let fp = [n; 10];
	PlatformRecord {
		fullname: format!("relay-{n:012x}._praxis-relay._tcp.local."),
		port: addr.port(),
		addresses: vec![addr.ip()],
		txt: vec![
			("v".to_owned(), "1".to_owned()),
			("fp".to_owned(), hex::encode(fp)),
			("id".to_owned(), hex::encode([n; 16])),
		],
	}
}

#[tokio::test]
async fn at_most_eight_dials_run_at_once() {
	let a = browsing_as("alpha", true, false).await;
	let (addr, count) = silent_listener().await;
	for n in 1..=12 {
		a.hints.send(Hint::Found(numbered(addr, n)));
	}
	sleep(Duration::from_millis(700)).await;
	assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 8);
}

#[tokio::test]
async fn an_instance_is_not_redialled_within_the_retry_pause() {
	let a = browsing_as("alpha", true, false).await;
	let (addr, count) = silent_listener().await;
	a.hints.send(Hint::Found(numbered(addr, 1)));
	sleep(Duration::from_millis(300)).await;
	// The same instance announced again (a refresh) does not start another dial.
	for _ in 0..3 {
		a.hints.send(Hint::Found(numbered(addr, 1)));
	}
	sleep(Duration::from_millis(500)).await;
	assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
}
