//! A trusted name that shows up with another key: the refusal is remembered, the contact can
//! be forgotten by either key, and only an exact, typed fingerprint replaces the pinned key.

mod support_peers;

use std::sync::Arc;
use std::time::SystemTime;

use relay_config::WriterLock;
use relay_config::db::Database;
use relay_config::favorites::Favorites;
use relay_config::identity::TrustStore as ConfigStore;
use relay_core::PeerEvent;
use relay_engine::peers::{ConfigTrust, PeerOutput, SharedTrust, spawn};
use relay_engine::service::{PeerSelector, PortError};
use relay_net::identity::{Identity, TrustRecord, TrustState};
use support_peers::{TestPeer, expect, fast_config, peer};

fn pre_trust(on: &TestPeer, other: &TestPeer, name: &str) {
	on.trust
		.lock()
		.put(TrustRecord {
			fingerprint: other.identity.fingerprint(),
			state: TrustState::UnverifiedFirstUse,
			display_name: name.to_owned(),
			trusted_at: SystemTime::now(),
		})
		.unwrap();
}

/// `impostor` (same name, new key) dials `node` and is refused.
async fn refused(node: &mut TestPeer, impostor: &TestPeer) {
	let _ = impostor.handle.dial(node.handle.local_addr(), None).await;
	let seen = expect(&mut node.output, |o| match o {
		PeerOutput::KeyChanged { peer, .. } => Some(*peer),
		_ => None,
	})
	.await;
	assert_eq!(seen, impostor.id());
}

fn full(peer: &TestPeer) -> String {
	hex::encode(peer.identity.fingerprint().as_bytes())
}

/// A node, its old contact and the new key under the same name, refused once.
async fn changed() -> (TestPeer, TestPeer, TestPeer) {
	let mut node = peer("bravo").await;
	let old = peer("alpha").await;
	pre_trust(&node, &old, "alpha");
	let new = peer("alpha").await;
	refused(&mut node, &new).await;
	(node, old, new)
}

#[tokio::test]
async fn a_refused_key_is_remembered_with_the_key_it_would_replace() {
	let (node, old, new) = changed().await;
	let change = node.handle.key_change(new.id()).expect("remembered");
	assert_eq!(change.previous, old.id());
	assert_eq!(change.previous_name, "alpha");
	assert!(node.handle.key_change(old.id()).is_none());
}

#[tokio::test]
async fn forgetting_by_the_offending_key_removes_the_old_pin_and_the_state() {
	let (node, old, new) = changed().await;
	node.handle.ensure_link(new.id()).unwrap();
	let gone = node.handle.forget(&PeerSelector::Id(new.id())).unwrap();
	assert_eq!((gone.id, gone.name.as_str()), (old.id(), "alpha"));
	assert_eq!(gone.key_changed, vec![new.id()]);
	assert!(node.handle.trusted().is_empty());
	assert!(node.handle.key_change(new.id()).is_none());
	assert!(matches!(
		node.handle.forget(&PeerSelector::Id(new.id())),
		Err(PortError::NotFound(_))
	));
}

#[tokio::test]
async fn forgetting_by_the_pinned_key_or_the_offending_prefix_also_works() {
	let (node, old, new) = changed().await;
	let gone = node.handle.forget(&PeerSelector::Id(old.id())).unwrap();
	assert_eq!(gone.key_changed, vec![new.id()]);
	assert!(node.handle.key_change(new.id()).is_none());

	let (node, old, new) = changed().await;
	let prefix = full(&new)[..12].to_owned();
	let gone = node.handle.forget(&PeerSelector::Query(prefix)).unwrap();
	assert_eq!(gone.id, old.id());
	assert!(node.handle.trusted().is_empty());
}

#[tokio::test]
async fn forgetting_an_unrelated_id_and_ambiguous_names_behave_as_before() {
	let (node, _, new) = changed().await;
	let other = peer("charlie").await;
	assert!(matches!(
		node.handle.forget(&PeerSelector::Id(other.id())),
		Err(PortError::NotFound(_))
	));
	assert_eq!(node.handle.trusted().len(), 1);
	assert!(node.handle.key_change(new.id()).is_some());

	let twin = peer("twin").await;
	let twin_b = peer("Twin").await;
	pre_trust(&node, &twin, "twin");
	pre_trust(&node, &twin_b, "Twin");
	assert!(matches!(
		node.handle.forget(&PeerSelector::Query("TWIN".to_owned())),
		Err(PortError::Ambiguous(_))
	));
}

#[tokio::test]
async fn only_the_exact_fingerprint_of_the_new_key_replaces_the_pin() {
	let (node, old, new) = changed().await;
	let digits = full(&new);
	let me = |typed: &str, verified: bool| {
		node.handle
			.replace_pinned_key(new.id(), typed, verified, Vec::new())
	};
	assert!(matches!(me(&digits, false), Err(PortError::Invalid(_))));
	assert!(matches!(
		me(&digits[..16], true),
		Err(PortError::Invalid(_))
	));
	assert!(matches!(
		me("not hex at all", true),
		Err(PortError::Invalid(_))
	));
	assert!(matches!(me(&full(&old), true), Err(PortError::Invalid(_))));
	let mut flipped = digits.clone();
	flipped.replace_range(63.., if digits.ends_with('0') { "1" } else { "0" });
	assert!(matches!(me(&flipped, true), Err(PortError::Invalid(_))));
	assert_eq!(node.handle.trusted(), vec![old.identity.fingerprint()]);
	assert!(node.handle.key_change(new.id()).is_some());

	// Spaces, dashes and capitals are what a person types.
	let grouped: Vec<String> = digits
		.as_bytes()
		.chunks(4)
		.map(|c| String::from_utf8_lossy(c).to_uppercase())
		.collect();
	let done = me(&grouped.join(" "), true).unwrap();
	assert_eq!((done.id, done.previous), (new.id(), old.id()));
	assert_eq!(done.name, "alpha");
	assert_eq!(node.handle.trusted(), vec![new.identity.fingerprint()]);
	let record = node.trust.lock().get(&new.identity.fingerprint()).unwrap();
	assert_eq!(record.state, TrustState::Verified);
	assert!(node.handle.key_change(new.id()).is_none());
}

#[tokio::test]
async fn an_unrelated_or_unremembered_peer_is_refused_and_nothing_changes() {
	let (node, old, _) = changed().await;
	let stranger = peer("alpha").await;
	let result = node
		.handle
		.replace_pinned_key(stranger.id(), &full(&stranger), true, Vec::new());
	assert!(matches!(result, Err(PortError::NotFound(_))));
	// The pinned key itself has no key change to verify either.
	let result = node
		.handle
		.replace_pinned_key(old.id(), &full(&old), true, Vec::new());
	assert!(matches!(result, Err(PortError::NotFound(_))));
	assert_eq!(node.handle.trusted(), vec![old.identity.fingerprint()]);
}

#[tokio::test]
async fn a_second_key_change_replaces_the_first_entry() {
	let (mut node, old, first) = changed().await;
	let second = peer("alpha").await;
	refused(&mut node, &second).await;
	assert!(node.handle.key_change(first.id()).is_none());
	assert_eq!(
		node.handle.key_change(second.id()).unwrap().previous,
		old.id()
	);
	assert!(matches!(
		node.handle
			.replace_pinned_key(first.id(), &full(&first), true, Vec::new()),
		Err(PortError::NotFound(_))
	));
	node.handle
		.replace_pinned_key(second.id(), &full(&second), true, Vec::new())
		.unwrap();
	assert_eq!(node.handle.trusted(), vec![second.identity.fingerprint()]);
}

#[tokio::test]
async fn a_replaced_contact_links_up_trusted_and_keeps_its_favorite() {
	let dir = tempfile::tempdir().unwrap();
	let lock = Arc::new(WriterLock::acquire(&dir.path().join("writer.lock")).unwrap());
	let opened = Database::open(dir.path().join("relay.db"), lock).unwrap();
	let database = opened.database;
	let trust = SharedTrust::new(ConfigTrust::new(ConfigStore::open(&database).unwrap()));
	let identity = Arc::new(Identity::generate().unwrap());
	let (handle, mut output) = spawn(fast_config("bravo"), identity, trust.clone(), [7; 16])
		.await
		.unwrap();

	// The old contact: pinned (the key must have been seen once to be stored).
	let old = Identity::generate().unwrap();
	let other = Identity::generate().unwrap();
	for (key, name) in [(&old, "alpha"), (&other, "delta")] {
		let mut store = trust.lock();
		store.observe(key.public());
		store
			.put(TrustRecord {
				fingerprint: key.fingerprint(),
				state: TrustState::UnverifiedFirstUse,
				display_name: name.to_owned(),
				trusted_at: SystemTime::now(),
			})
			.unwrap();
	}
	let favorites = Favorites::open(&database);
	let key_of = |i: &Identity| *i.public().as_bytes();
	favorites.set(&[key_of(&other), key_of(&old)]).unwrap();

	let new = peer("alpha").await;
	let _ = new.handle.dial(handle.local_addr(), None).await;
	expect(&mut output, |o| {
		matches!(o, PeerOutput::KeyChanged { .. }).then_some(())
	})
	.await;
	handle
		.replace_pinned_key(new.id(), &full(&new), true, Vec::new())
		.unwrap();

	assert_eq!(
		favorites.list().unwrap(),
		vec![key_of(&other), key_of(&new.identity)]
	);
	assert!(handle.trusted().contains(&new.identity.fingerprint()));
	assert!(!handle.trusted().contains(&old.fingerprint()));
	// The device calls again with its new key and is trusted at once.
	new.handle.dial(handle.local_addr(), None).await.unwrap();
	expect(&mut output, |o| {
		matches!(o, PeerOutput::Core(PeerEvent::LinkUp(id)) if *id == new.id()).then_some(())
	})
	.await;
}
