mod support_peers;

use std::sync::Arc;
use std::time::{Duration, SystemTime};

use relay_config::avatar::{AvatarCache, process_avatar};
use relay_engine::peers::{LocalProfile, PeerOutput};
use relay_net::discovery::{DiscoveredPeer, FingerprintHint};
use relay_net::identity::{Identity, TrustRecord, TrustState};
use relay_net::proto::{AvatarChunk, Profile, envelope::Body};
use support_peers::{Raw, expect, fast_config, peer, peer_with};

/// A noisy 128x128 PNG, big enough to need several chunks.
fn noisy_png() -> Vec<u8> {
	let mut state = 0x1234_5678u32;
	let img = image::RgbImage::from_fn(128, 128, |_, _| {
		let mut next = || {
			state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
			(state >> 24) as u8
		};
		image::Rgb([next(), next(), next()])
	});
	let mut out = Vec::new();
	img.write_to(&mut std::io::Cursor::new(&mut out), image::ImageFormat::Png)
		.unwrap();
	out
}

fn trust(on: &support_peers::TestPeer, key: &Identity, name: &str) {
	on.trust
		.lock()
		.put(TrustRecord {
			fingerprint: key.fingerprint(),
			state: TrustState::UnverifiedFirstUse,
			display_name: name.to_owned(),
			trusted_at: SystemTime::now(),
		})
		.unwrap();
}

struct WithCache {
	peer: support_peers::TestPeer,
	cache: Arc<AvatarCache>,
	_dir: tempfile::TempDir,
}

async fn with_cache(name: &str, avatar: Option<[u8; 32]>, seed: Option<&AvatarCache>) -> WithCache {
	let dir = tempfile::tempdir().unwrap();
	let cache = Arc::new(AvatarCache::open(dir.path()).unwrap());
	if let Some(seed) = seed {
		let _ = seed;
	}
	let mut config = fast_config(name);
	config.avatars = Some(Arc::clone(&cache));
	config.avatar = avatar;
	let peer = peer_with(config, Arc::new(Identity::generate().unwrap())).await;
	WithCache {
		peer,
		cache,
		_dir: dir,
	}
}

#[tokio::test]
async fn a_discovered_name_is_shown_before_any_connection_and_is_sanitized() {
	let node = peer("bravo").await;
	let target = Identity::generate().unwrap();
	node.handle
		.note_discovered(DiscoveredPeer {
			fullname: "relay-1._praxis-relay._tcp.local.".into(),
			instance_id: Default::default(),
			display_name: "Kit\u{202E}chen\u{0007}<b>".into(),
			fingerprint_hint: Some(FingerprintHint::of(&target.fingerprint())),
			capabilities: String::new(),
			addresses: vec!["127.0.0.1:9".parse().unwrap()],
			txt: Default::default(),
		})
		.unwrap();
	tokio::time::sleep(Duration::from_millis(100)).await;
	let peers = node.handle.peers();
	let found = peers.iter().find(|p| p.id.is_none()).expect("listed");
	assert_eq!(found.name, "Kitchenb");
	assert!(!found.linked && found.trust.is_none() && found.avatar.is_none());
}

#[tokio::test]
async fn profile_updates_reach_a_trusted_peer() {
	let (a, mut b) = support_peers::linked_pair().await;
	let first = expect(&mut b.output, |o| match o {
		PeerOutput::Profile { name, .. } => Some(name.clone()),
		_ => None,
	})
	.await;
	assert_eq!(first, "alpha");
	a.handle
		.set_profile(LocalProfile {
			display_name: "Alpha \u{200B}Two".into(),
			avatar: None,
		})
		.unwrap();
	let second = expect(&mut b.output, |o| match o {
		PeerOutput::Profile { name, .. } => Some(name.clone()),
		_ => None,
	})
	.await;
	assert_eq!(second, "Alpha Two");
	tokio::time::sleep(Duration::from_millis(50)).await;
	let seen = b
		.handle
		.peers()
		.iter()
		.find(|p| p.linked)
		.map(|p| p.name.clone());
	assert_eq!(seen.as_deref(), Some("Alpha Two"));
}

#[tokio::test]
async fn an_avatar_is_fetched_in_chunks_verified_and_cached() {
	let avatar = process_avatar(&noisy_png()).unwrap();
	let sender_dir = tempfile::tempdir().unwrap();
	let sender_cache = Arc::new(AvatarCache::open(sender_dir.path()).unwrap());
	sender_cache.put(&avatar).unwrap();
	let mut a_config = fast_config("alpha");
	a_config.avatars = Some(Arc::clone(&sender_cache));
	a_config.avatar = Some(avatar.hash);
	let mut a = peer_with(a_config, Arc::new(Identity::generate().unwrap())).await;
	let mut b = with_cache("bravo", None, None).await;
	trust(&a, &b.peer.identity, "bravo");
	trust(&b.peer, &a.identity, "alpha");
	b.peer
		.handle
		.add_address(a.id(), a.handle.local_addr())
		.unwrap();
	b.peer.handle.ensure_link(a.id()).unwrap();
	let hash = expect(&mut b.peer.output, |o| match o {
		PeerOutput::AvatarReady { hash, .. } => Some(*hash),
		PeerOutput::AvatarRejected { reason, .. } => panic!("rejected: {reason}"),
		_ => None,
	})
	.await;
	assert_eq!(hash, avatar.hash);
	assert_eq!(b.cache.get(&hash).unwrap().unwrap(), avatar.png);
	assert!(avatar.png.len() > 8192, "the transfer used several chunks");
	let _ = &mut a.output;
}

async fn announce(raw: &mut Raw, hash: [u8; 32], size: u32) {
	raw.send(Body::Profile(Profile {
		display_name: "mallory".into(),
		avatar_sha256: hash.to_vec(),
		avatar_size: size,
	}))
	.await;
}

fn chunk(hash: [u8; 32], offset: u32, data: Vec<u8>, last: bool) -> Body {
	Body::AvatarChunk(AvatarChunk {
		avatar_sha256: hash.to_vec(),
		offset,
		total_size: 1,
		data,
		final_chunk: last,
	})
}

async fn trusted_target() -> (WithCache, Raw) {
	let target = with_cache("target", None, None).await;
	let mut raw = Raw::connect(target.peer.handle.local_addr(), "mallory").await;
	target
		.peer
		.handle
		.pin(raw.id(), TrustState::UnverifiedFirstUse)
		.await
		.unwrap();
	let _ = &mut raw;
	(target, raw)
}

#[tokio::test]
async fn malformed_avatar_bytes_are_rejected_and_not_cached() {
	let (mut target, mut raw) = trusted_target().await;
	let hash = [7u8; 32];
	announce(&mut raw, hash, 100).await;
	assert!(matches!(
		raw.next_body(Duration::from_secs(2)).await,
		Some(Body::AvatarRequest(r)) if r.avatar_sha256 == hash
	));
	raw.send(chunk(hash, 0, vec![0xAB; 100], true)).await;
	let reason = expect(&mut target.peer.output, |o| match o {
		PeerOutput::AvatarRejected { reason, .. } => Some(reason.clone()),
		PeerOutput::AvatarReady { .. } => panic!("garbage accepted"),
		_ => None,
	})
	.await;
	assert!(!reason.is_empty());
	assert!(target.cache.get(&hash).unwrap().is_none());
	assert_eq!(target.cache.size().unwrap(), 0);
	// The same hash is not requested again.
	announce(&mut raw, hash, 100).await;
	assert!(raw.next_body(Duration::from_millis(300)).await.is_none());
}

#[tokio::test]
async fn more_bytes_than_announced_abort_the_transfer() {
	let (mut target, mut raw) = trusted_target().await;
	let hash = [8u8; 32];
	announce(&mut raw, hash, 100).await;
	assert!(matches!(
		raw.next_body(Duration::from_secs(2)).await,
		Some(Body::AvatarRequest(_))
	));
	raw.send(chunk(hash, 0, vec![1; 200], false)).await;
	expect(&mut target.peer.output, |o| {
		matches!(o, PeerOutput::AvatarRejected { .. }).then_some(())
	})
	.await;
}

#[tokio::test]
async fn a_valid_image_under_a_lying_hash_is_rejected() {
	let (mut target, mut raw) = trusted_target().await;
	let real = process_avatar(&noisy_png()).unwrap();
	let lie = [9u8; 32];
	announce(&mut raw, lie, real.png.len() as u32).await;
	assert!(matches!(
		raw.next_body(Duration::from_secs(2)).await,
		Some(Body::AvatarRequest(_))
	));
	for (i, part) in real.png.chunks(8192).enumerate() {
		let last = (i + 1) * 8192 >= real.png.len();
		raw.send(chunk(lie, (i * 8192) as u32, part.to_vec(), last))
			.await;
	}
	expect(&mut target.peer.output, |o| match o {
		PeerOutput::AvatarRejected { hash, .. } => Some(*hash),
		PeerOutput::AvatarReady { .. } => panic!("lying hash accepted"),
		_ => None,
	})
	.await;
}

#[tokio::test]
async fn an_oversized_announcement_is_never_requested() {
	let (mut target, mut raw) = trusted_target().await;
	announce(&mut raw, [5u8; 32], u32::MAX).await;
	let seen = expect(&mut target.peer.output, |o| match o {
		PeerOutput::Profile { avatar, .. } => Some(*avatar),
		_ => None,
	})
	.await;
	assert_eq!(seen, None);
	assert!(raw.next_body(Duration::from_millis(300)).await.is_none());
}

#[tokio::test]
async fn an_untrusted_authenticated_peer_gets_no_avatar_request_and_no_profile_event() {
	let mut target = with_cache("target", None, None).await;
	let mut raw = Raw::connect(target.peer.handle.local_addr(), "mallory").await;
	announce(&mut raw, [3u8; 32], 100).await;
	assert!(raw.next_body(Duration::from_millis(400)).await.is_none());
	while let Ok(item) = target.peer.output.try_recv() {
		assert!(!matches!(item, PeerOutput::Profile { .. }));
	}
}

#[tokio::test]
async fn unsolicited_chunks_and_foreign_requests_are_ignored() {
	let (mut target, mut raw) = trusted_target().await;
	raw.send(chunk([1u8; 32], 0, vec![1; 10], true)).await;
	raw.send(Body::AvatarRequest(relay_net::proto::AvatarRequest {
		avatar_sha256: vec![2u8; 32],
	}))
	.await;
	assert!(raw.next_body(Duration::from_millis(300)).await.is_none());
	while let Ok(item) = target.peer.output.try_recv() {
		assert!(!matches!(
			item,
			PeerOutput::AvatarReady { .. } | PeerOutput::AvatarRejected { .. }
		));
	}
}
