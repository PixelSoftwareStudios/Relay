//! A scanned code repairs a key-changed contact only when it names the refused key itself.

mod support_peers;

use relay_engine::peers::{PeerAdapter, PeerOutput};
use relay_engine::service::{PeerControl, PortError};
use relay_net::AddressPolicy;
use relay_net::identity::{TrustRecord, TrustState};
use support_peers::{expect, peer};

fn code(port: u16, digits: &str) -> String {
	format!("relay://127.0.0.1:{port}#{digits}")
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_new_codes_fingerprint_finds_the_key_changed_contact() {
	let mut node = peer("bravo").await;
	let old = peer("alpha").await;
	node.trust
		.lock()
		.put(TrustRecord {
			fingerprint: old.identity.fingerprint(),
			state: TrustState::UnverifiedFirstUse,
			display_name: "alpha".to_owned(),
			trusted_at: std::time::SystemTime::now(),
		})
		.unwrap();
	let new = peer("alpha").await;
	let _ = new.handle.dial(node.handle.local_addr(), None).await;
	expect(&mut node.output, |o| {
		matches!(o, PeerOutput::KeyChanged { .. }).then_some(())
	})
	.await;

	let adapter = PeerAdapter::new(
		node.handle.clone(),
		tokio::runtime::Handle::current(),
		AddressPolicy::lan_and_loopback(),
		AddressPolicy::lan_and_loopback(),
	);
	let port = new.handle.local_addr().port();
	let digits = |p: &support_peers::TestPeer| hex::encode(p.identity.fingerprint().as_bytes());

	// The old key's code, a stranger's code and the matching name alone never qualify.
	assert!(adapter.key_change_for(&code(port, &digits(&old))).is_none());
	assert!(
		adapter
			.key_change_for(&code(port, &"ab".repeat(32)))
			.is_none()
	);
	assert!(
		adapter
			.key_change_for(&code(port, &digits(&new)[..16]))
			.is_none()
	);
	assert!(adapter.key_change_for("not a code").is_none());
	let text = code(port, &digits(&new));
	assert_eq!(adapter.key_change_for(&text), Some(new.id()));

	// The adapter refuses a short or wrong fingerprint even for the right device.
	let refused = adapter.replace_pinned_key(new.id(), &digits(&old), true, Some(&text));
	assert!(matches!(refused, Err(PortError::Invalid(_))));
	let done = adapter
		.replace_pinned_key(new.id(), &digits(&new), true, Some(&text))
		.unwrap();
	assert_eq!((done.id, done.previous), (new.id(), old.id()));
	assert!(adapter.key_change_for(&text).is_none());
}
