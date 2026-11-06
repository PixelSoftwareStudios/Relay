mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, Instant};

use relay_net::AddressPolicy;
use relay_net::media::{
	MAX_PAYLOAD_BYTES, MediaEndpoint, MediaError, MediaReceiver, PathValidator, Received,
	RejectReason, ResponseLimiter, seal_audio,
};
use relay_net::record::{Lane, RecordHeader, RecordKind};
use relay_net::session::{LinkEpoch, SecureSession};

const CALL: [u8; 16] = [3; 16];

fn receiver_for(session: &Arc<SecureSession>) -> MediaReceiver {
	let mut rx = MediaReceiver::new(Arc::clone(session));
	assert!(rx.bind_stream(CALL, 1));
	rx
}

fn audio(session: &SecureSession, payload: &[u8]) -> Vec<u8> {
	seal_audio(session, &CALL, 1, payload).unwrap()
}

#[tokio::test]
async fn accepts_authentic_audio_and_returns_opaque_payload() {
	let (tx, rx_session) = common::sessions().await;
	let mut rx = receiver_for(&rx_session);
	let datagram = audio(&tx, b"opaque rtp bytes");
	match rx.receive(&datagram, Instant::now()).unwrap() {
		Received::Audio {
			call_id,
			generation,
			payload,
			epoch,
		} => {
			assert_eq!(
				(call_id, generation, payload.as_slice()),
				(CALL, 1, &b"opaque rtp bytes"[..])
			);
			assert_eq!(&epoch, tx.epoch());
		}
		other => panic!("unexpected {other:?}"),
	}
}

#[tokio::test]
async fn ciphertext_does_not_contain_the_plaintext_payload() {
	let (tx, _) = common::sessions().await;
	let payload = [0xA5u8; 64];
	let datagram = audio(&tx, &payload);
	assert!(!datagram.windows(16).any(|w| w == [0xA5u8; 16]));
}

#[tokio::test]
async fn nonces_are_unique_per_lane_and_lanes_are_disjoint() {
	let (tx, _) = common::sessions().await;
	let mut media = Vec::new();
	for _ in 0..500 {
		let header = RecordHeader::parse(&audio(&tx, b"x")).unwrap();
		assert_eq!(header.lane(), Lane::Media);
		media.push(header.nonce());
	}
	assert!(
		media.windows(2).all(|w| w[1] == w[0] + 1),
		"monotonic from zero"
	);
	assert_eq!(media[0], 0);

	let control = RecordHeader::parse(&tx.seal(RecordKind::Envelope, b"y").unwrap()).unwrap();
	assert_eq!(control.lane(), Lane::Control);
	assert_eq!(
		control.sequence(),
		0,
		"control counter is independent of media"
	);
	assert_ne!(control.nonce(), media[0]);
	assert_eq!(control.nonce() >> 63, 1);
}

#[tokio::test]
async fn payload_nonce_is_independent_of_rtp_sequence() {
	// Two datagrams with identical payload bytes must still use different nonces.
	let (tx, _) = common::sessions().await;
	let a = RecordHeader::parse(&audio(&tx, b"same")).unwrap();
	let b = RecordHeader::parse(&audio(&tx, b"same")).unwrap();
	assert_ne!(a.nonce(), b.nonce());
}

#[tokio::test]
async fn duplicate_is_rejected_and_reordering_within_window_is_accepted() {
	let (tx, rx_session) = common::sessions().await;
	let mut rx = receiver_for(&rx_session);
	let packets: Vec<_> = (0..10).map(|i| audio(&tx, &[i])).collect();
	let now = Instant::now();
	for i in [0usize, 5, 3, 9, 1] {
		assert!(rx.receive(&packets[i], now).is_ok(), "packet {i}");
	}
	assert_eq!(rx.receive(&packets[5], now), Err(RejectReason::Replay));
	assert_eq!(rx.receive(&packets[9], now), Err(RejectReason::Replay));
}

#[tokio::test]
async fn records_older_than_the_window_are_rejected() {
	let (tx, rx_session) = common::sessions().await;
	let mut rx = receiver_for(&rx_session);
	let old = audio(&tx, b"old");
	for _ in 0..1100 {
		let _ = audio(&tx, b"skip");
	}
	let newer = audio(&tx, b"new");
	let now = Instant::now();
	assert!(rx.receive(&newer, now).is_ok());
	assert_eq!(rx.receive(&old, now), Err(RejectReason::Replay));
}

#[tokio::test]
async fn forged_huge_counter_does_not_poison_the_replay_window() {
	let (tx, rx_session) = common::sessions().await;
	let mut rx = receiver_for(&rx_session);
	let now = Instant::now();

	// Attacker forges a header with a near-maximal counter and random ciphertext.
	let forged_header = RecordHeader::new(Lane::Media, *tx.epoch(), (1 << 62) + 7).encode();
	let mut forged = forged_header.to_vec();
	forged.extend_from_slice(&[0x42; 80]);
	assert_eq!(rx.receive(&forged, now), Err(RejectReason::Authentication));

	// Also a real record whose counter was altered in transit.
	let mut altered = audio(&tx, b"genuine");
	altered[24] = 0x3F;
	assert_eq!(rx.receive(&altered, now), Err(RejectReason::Authentication));

	// Legitimate traffic still flows and ordering state is untouched.
	for i in 0..5u8 {
		assert!(rx.receive(&audio(&tx, &[i]), now).is_ok());
	}
}

#[tokio::test]
async fn every_single_byte_flip_is_rejected() {
	let (tx, rx_session) = common::sessions().await;
	let datagram = audio(&tx, b"integrity covers every byte");
	for i in 0..datagram.len() {
		let mut rx = receiver_for(&rx_session);
		let mut flipped = datagram.clone();
		flipped[i] ^= 0x01;
		assert!(
			rx.receive(&flipped, Instant::now()).is_err(),
			"byte {i} flip accepted"
		);
		// The pristine datagram must still be accepted afterwards: failures left no trace.
		assert!(
			rx.receive(&datagram, Instant::now()).is_ok(),
			"state mutated by flip at {i}"
		);
	}
}

#[tokio::test]
async fn truncated_and_oversize_datagrams_are_rejected() {
	let (tx, rx_session) = common::sessions().await;
	let mut rx = receiver_for(&rx_session);
	let datagram = audio(&tx, b"payload");
	let now = Instant::now();
	for len in [0, 1, 31, 32, datagram.len() - 1] {
		assert!(rx.receive(&datagram[..len], now).is_err(), "len {len}");
	}
	assert_eq!(rx.receive(&[0u8; 1201], now), Err(RejectReason::Length));
	assert!(matches!(
		seal_audio(&tx, &CALL, 1, &vec![0; MAX_PAYLOAD_BYTES + 1]),
		Err(MediaError::Body(_))
	));
	let max = seal_audio(&tx, &CALL, 1, &vec![0; MAX_PAYLOAD_BYTES]).unwrap();
	assert_eq!(max.len(), 1200);
	assert!(rx.receive(&max, now).is_ok());
}

#[tokio::test]
async fn unbound_stream_or_generation_never_reaches_the_decoder() {
	let (tx, rx_session) = common::sessions().await;
	let mut rx = receiver_for(&rx_session);
	let now = Instant::now();
	let other_call = seal_audio(&tx, &[9; 16], 1, b"x").unwrap();
	let other_generation = seal_audio(&tx, &CALL, 2, b"x").unwrap();
	assert_eq!(
		rx.receive(&other_call, now),
		Err(RejectReason::UnboundStream)
	);
	assert_eq!(
		rx.receive(&other_generation, now),
		Err(RejectReason::UnboundStream)
	);
	rx.unbind_call(&CALL);
	assert_eq!(
		rx.receive(&audio(&tx, b"x"), now),
		Err(RejectReason::UnboundStream)
	);
}

#[tokio::test]
async fn records_from_another_link_are_not_accepted() {
	let (_, rx_session) = common::sessions().await;
	let (other_tx, _) = common::sessions().await;
	let mut rx = receiver_for(&rx_session);
	assert_eq!(
		rx.receive(&audio(&other_tx, b"x"), Instant::now()),
		Err(RejectReason::Epoch)
	);
}

#[tokio::test]
async fn control_lane_record_is_not_accepted_as_media() {
	let (tx, rx_session) = common::sessions().await;
	let mut rx = receiver_for(&rx_session);
	let control = tx.seal(RecordKind::Envelope, &[0u8; 40]).unwrap();
	assert_eq!(
		rx.receive(&control, Instant::now()),
		Err(RejectReason::Header)
	);
}

#[tokio::test]
async fn epoch_overlap_admits_at_most_two_epochs_for_a_bounded_time() {
	let (old_tx, old_rx) = common::sessions().await;
	let (new_tx, new_rx) = common::sessions().await;
	// Both receivers' sessions are independent handshakes; model the peer holding both.
	let mut rx = MediaReceiver::with_overlap(Arc::clone(&old_rx), Duration::from_millis(500));
	rx.bind_stream(CALL, 1);
	let start = Instant::now();
	rx.install_epoch(Arc::clone(&new_rx), start);
	assert_eq!(rx.epoch_count(), 2);

	assert!(
		rx.receive(&audio(&old_tx, b"a"), start + Duration::from_millis(100))
			.is_ok()
	);
	assert!(
		rx.receive(&audio(&new_tx, b"b"), start + Duration::from_millis(100))
			.is_ok()
	);
	// After the overlap the old epoch is gone; the new one keeps working.
	let late = start + Duration::from_millis(600);
	assert_eq!(
		rx.receive(&audio(&old_tx, b"c"), late),
		Err(RejectReason::Epoch)
	);
	assert!(rx.receive(&audio(&new_tx, b"d"), late).is_ok());
	assert_eq!(rx.epoch_count(), 1);
}

#[tokio::test]
async fn a_third_epoch_evicts_the_oldest() {
	let (_, a) = common::sessions().await;
	let (_, b) = common::sessions().await;
	let (_, c) = common::sessions().await;
	let mut rx = MediaReceiver::new(Arc::clone(&a));
	let now = Instant::now();
	rx.install_epoch(Arc::clone(&b), now);
	rx.install_epoch(Arc::clone(&c), now);
	assert_eq!(rx.epoch_count(), 2);
	assert!(rx.session_for(a.epoch()).is_none());
	assert!(rx.session_for(c.epoch()).is_some());
}

#[test]
fn path_validator_requires_matching_challenge_endpoint_epoch_and_freshness() {
	let addr: SocketAddr = "192.168.1.10:5000".parse().unwrap();
	let other: SocketAddr = "192.168.1.11:5000".parse().unwrap();
	let epoch = LinkEpoch::random();
	let now = Instant::now();
	let mut path = PathValidator::new();
	let challenge = path.begin(addr, epoch, now);

	assert!(
		path.on_response(&[0; 16], addr, &epoch, now).is_none(),
		"unknown challenge"
	);
	assert!(
		path.on_response(&challenge, other, &epoch, now).is_none(),
		"wrong source"
	);
	assert!(
		path.on_response(&challenge, addr, &LinkEpoch::random(), now)
			.is_none(),
		"wrong epoch"
	);
	assert!(
		path.on_response(&challenge, addr, &epoch, now + Duration::from_secs(4))
			.is_none(),
		"expired"
	);
	assert_eq!(path.validated(), None);
	assert_eq!(
		path.on_response(&challenge, addr, &epoch, now + Duration::from_secs(1)),
		Some(addr)
	);
	assert!(
		path.on_response(&challenge, addr, &epoch, now).is_none(),
		"challenge is single use"
	);
	path.invalidate();
	assert_eq!(path.validated(), None);
}

#[test]
fn response_limiter_caps_replies_per_second() {
	let now = Instant::now();
	let mut limiter = ResponseLimiter::new(3, now);
	assert!((0..5).filter(|_| limiter.allow(now)).count() == 3);
	assert!(limiter.allow(now + Duration::from_millis(1100)));
}

async fn endpoint_pair() -> (
	Arc<MediaEndpoint>,
	Arc<MediaEndpoint>,
	Arc<SecureSession>,
	Arc<SecureSession>,
) {
	let (a_session, b_session) = common::sessions().await;
	let policy = AddressPolicy::lan_and_loopback();
	let loopback: SocketAddr = "127.0.0.1:0".parse().unwrap();
	let a = Arc::new(
		MediaEndpoint::bind(loopback, Arc::clone(&a_session), policy)
			.await
			.unwrap(),
	);
	let b = Arc::new(
		MediaEndpoint::bind(loopback, Arc::clone(&b_session), policy)
			.await
			.unwrap(),
	);
	(a, b, a_session, b_session)
}

fn spawn_recv(endpoint: &Arc<MediaEndpoint>) -> tokio::sync::mpsc::UnboundedReceiver<Vec<u8>> {
	let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
	let endpoint = Arc::clone(endpoint);
	tokio::spawn(async move {
		while let Ok(packet) = endpoint.recv().await {
			let _ = tx.send(packet.payload);
		}
	});
	rx
}

#[tokio::test]
async fn udp_loopback_path_validation_then_audio_both_directions() {
	let (a, b, ..) = endpoint_pair().await;
	b.bind_stream(CALL, 1);
	a.bind_stream(CALL, 1);
	let (mut a_rx, mut b_rx) = (spawn_recv(&a), spawn_recv(&b));

	assert!(matches!(
		a.send_audio(&CALL, 1, b"early").await,
		Err(MediaError::PathNotValidated)
	));

	let (a_addr, b_addr) = (a.local_addr().unwrap(), b.local_addr().unwrap());
	a.validate_path(b_addr).await.unwrap();
	b.validate_path(a_addr).await.unwrap();
	assert_eq!(a.validated_path(), Some(b_addr));

	a.send_audio(&CALL, 1, b"hello bob").await.unwrap();
	b.send_audio(&CALL, 1, b"hello alice").await.unwrap();
	assert_eq!(b_rx.recv().await.unwrap(), b"hello bob");
	assert_eq!(a_rx.recv().await.unwrap(), b"hello alice");

	a.invalidate_path();
	assert!(matches!(
		a.send_audio(&CALL, 1, b"x").await,
		Err(MediaError::PathNotValidated)
	));
}

#[tokio::test]
async fn path_validation_survives_lost_challenges_by_sending_again() {
	let (a, b, ..) = endpoint_pair().await;
	let (_a_rx, _b_rx) = (spawn_recv(&a), spawn_recv(&b));
	let b_addr = b.local_addr().unwrap();
	// A relay that loses the first two datagrams towards B, like a Wi-Fi link that is still
	// resolving the neighbour, and passes everything else in both directions.
	let relay = Arc::new(tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap());
	let relay_addr = relay.local_addr().unwrap();
	let task = {
		let relay = Arc::clone(&relay);
		tokio::spawn(async move {
			let (mut buf, mut dropped, mut from_a) = ([0u8; 2048], 0, None);
			loop {
				let Ok((n, from)) = relay.recv_from(&mut buf).await else {
					return;
				};
				if from == b_addr {
					if let Some(a_addr) = from_a {
						let _ = relay.send_to(&buf[..n], a_addr).await;
					}
				} else {
					from_a = Some(from);
					if dropped < 2 {
						dropped += 1;
					} else {
						let _ = relay.send_to(&buf[..n], b_addr).await;
					}
				}
			}
		})
	};
	let started = Instant::now();
	a.validate_path(relay_addr).await.unwrap();
	assert!(
		started.elapsed() >= Duration::from_millis(400),
		"two sends were lost"
	);
	assert_eq!(a.validated_path(), Some(relay_addr));
	task.abort();
}

#[tokio::test]
async fn path_validation_times_out_when_nothing_answers() {
	let (a, ..) = endpoint_pair().await;
	let silent = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
	let _recv = spawn_recv(&a);
	let result = a.validate_path(silent.local_addr().unwrap()).await;
	assert!(matches!(result, Err(MediaError::PathTimeout)));
	assert_eq!(a.validated_path(), None);
}

#[tokio::test]
async fn public_destinations_are_refused_by_policy() {
	let (a, ..) = endpoint_pair().await;
	let public: SocketAddr = "8.8.8.8:5000".parse().unwrap();
	assert!(matches!(
		a.validate_path(public).await,
		Err(MediaError::Address(_))
	));
}

#[tokio::test]
async fn injected_garbage_and_replays_do_not_disturb_live_media() {
	let (a, b, a_session, _) = endpoint_pair().await;
	b.bind_stream(CALL, 1);
	let mut b_rx = spawn_recv(&b);
	let _a_rx = spawn_recv(&a);
	let b_addr = b.local_addr().unwrap();
	a.validate_path(b_addr).await.unwrap();

	let attacker = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
	attacker.send_to(&[0xFF; 300], b_addr).await.unwrap();
	let captured = seal_audio(&a_session, &CALL, 1, b"captured").unwrap();
	attacker.send_to(&captured, b_addr).await.unwrap();
	attacker.send_to(&captured, b_addr).await.unwrap();

	a.send_audio(&CALL, 1, b"live").await.unwrap();
	let mut got = vec![b_rx.recv().await.unwrap(), b_rx.recv().await.unwrap()];
	got.sort();
	assert_eq!(
		got,
		vec![b"captured".to_vec(), b"live".to_vec()],
		"replay delivered at most once"
	);
	assert!(
		tokio::time::timeout(Duration::from_millis(100), b_rx.recv())
			.await
			.is_err()
	);
}

#[tokio::test]
async fn source_address_change_does_not_redirect_media() {
	let (a, b, ..) = endpoint_pair().await;
	b.bind_stream(CALL, 1);
	let (_a_rx, _b_rx) = (spawn_recv(&a), spawn_recv(&b));
	let b_addr = b.local_addr().unwrap();
	a.validate_path(b_addr).await.unwrap();
	// Traffic from a new source (here: a spoofed raw socket) never changes a's destination.
	let spoofer = tokio::net::UdpSocket::bind("127.0.0.1:0").await.unwrap();
	a.send_audio(&CALL, 1, b"x").await.unwrap();
	spoofer
		.send_to(&[1; 100], a.local_addr().unwrap())
		.await
		.unwrap();
	assert_eq!(a.validated_path(), Some(b_addr));
}

#[tokio::test]
async fn rekey_switch_validates_new_epoch_before_sending_on_it() {
	let (a, b, ..) = endpoint_pair().await;
	b.bind_stream(CALL, 1);
	let mut b_rx = spawn_recv(&b);
	let _a_rx = spawn_recv(&a);
	let (a_addr, b_addr) = (a.local_addr().unwrap(), b.local_addr().unwrap());
	a.validate_path(b_addr).await.unwrap();
	b.validate_path(a_addr).await.unwrap();

	// A second handshake yields the next epoch's keys for both sides.
	let (next_a, next_b) = common::sessions().await;
	let (ra, rb) = tokio::join!(
		a.switch_epoch(next_a, b_addr),
		b.switch_epoch(next_b, a_addr)
	);
	ra.unwrap();
	rb.unwrap();

	a.send_audio(&CALL, 1, b"after rekey").await.unwrap();
	assert_eq!(b_rx.recv().await.unwrap(), b"after rekey");
}
