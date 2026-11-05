//! RT-10: the media send and receive paths allocate nothing per packet, and the
//! allocation-free API produces the same wire format as the allocating one.

mod common;

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Instant;

use relay_net::AddressPolicy;
use relay_net::media::{
	MAX_PAYLOAD_BYTES, MediaEndpoint, MediaError, MediaReceiver, Received, ReceivedRef, seal_audio,
	seal_audio_into,
};
use relay_net::record::RecordKind;
use relay_net::session::SecureSession;

use relay_testkit::{CountingAlloc, allocations_in};

#[global_allocator]
static ALLOC: CountingAlloc = CountingAlloc;

const CALL: [u8; 16] = [3; 16];

fn receiver_for(session: &Arc<SecureSession>) -> MediaReceiver {
	let mut rx = MediaReceiver::new(Arc::clone(session));
	assert!(rx.bind_stream(CALL, 1));
	rx
}

#[tokio::test]
async fn seal_into_is_allocation_free_and_wire_identical() {
	let (tx, rx_session) = common::sessions().await;
	let payload = [0x5a_u8; 92];
	let mut out = [0u8; 1200];
	let mut n = 0;
	let allocs = allocations_in(|| {
		n = seal_audio_into(&tx, &CALL, 1, &payload, &mut out).unwrap();
	});
	assert_eq!(allocs, 0);
	assert_eq!(n, seal_audio(&tx, &CALL, 1, &payload).unwrap().len());
	// The old allocating receiver accepts the new sealer's datagram.
	let mut rx = receiver_for(&rx_session);
	match rx.receive(&out[..n], Instant::now()).unwrap() {
		Received::Audio { payload: p, .. } => assert_eq!(p, payload),
		other => panic!("unexpected {other:?}"),
	}
}

#[tokio::test]
async fn receive_ref_is_allocation_free() {
	let (tx, rx_session) = common::sessions().await;
	let mut rx = receiver_for(&rx_session);
	let payload = [7_u8; 92];
	let datagrams: Vec<Vec<u8>> = (0..20)
		.map(|_| seal_audio(&tx, &CALL, 1, &payload).unwrap())
		.collect();
	let mut scratch = [0u8; 1201];
	let mut accepted = 0;
	let allocs = allocations_in(|| {
		for d in &datagrams {
			if let Ok(ReceivedRef::Audio { payload: p, .. }) =
				rx.receive_ref(d, Instant::now(), &mut scratch)
			{
				assert_eq!(p.len(), 92);
				accepted += 1;
			}
		}
	});
	assert_eq!((accepted, allocs), (20, 0));
}

#[tokio::test]
async fn oversize_payload_is_refused_without_consuming_a_counter() {
	let (tx, _) = common::sessions().await;
	let big = vec![0u8; MAX_PAYLOAD_BYTES + 1];
	let mut out = [0u8; 1200];
	assert!(matches!(
		seal_audio_into(&tx, &CALL, 1, &big, &mut out),
		Err(MediaError::Body(_))
	));
	let mut tiny = [0u8; 40];
	assert!(seal_audio_into(&tx, &CALL, 1, &[1], &mut tiny).is_err());
	// No sequence was burned by the refusals: the first real record is sequence 0.
	let n = seal_audio_into(&tx, &CALL, 1, &[1], &mut out).unwrap();
	let header = relay_net::record::RecordHeader::parse(&out[..n]).unwrap();
	assert_eq!(header.sequence(), 0);
	let _ = RecordKind::Audio;
}

#[tokio::test]
async fn try_send_and_recv_into_allocate_nothing_per_packet() {
	let (a_session, b_session) = common::sessions().await;
	let policy = AddressPolicy::lan_and_loopback();
	let loopback: SocketAddr = "127.0.0.1:0".parse().unwrap();
	let a = Arc::new(
		MediaEndpoint::bind(loopback, a_session, policy)
			.await
			.unwrap(),
	);
	let b = Arc::new(
		MediaEndpoint::bind(loopback, b_session, policy)
			.await
			.unwrap(),
	);
	a.bind_stream(CALL, 1);
	b.bind_stream(CALL, 1);
	let b_addr = b.local_addr().unwrap();
	// Both sides must be receiving to answer and to see the path response.
	let pumps: Vec<_> = [&a, &b]
		.into_iter()
		.map(|e| {
			let e = Arc::clone(e);
			tokio::spawn(async move {
				let mut buf = [0u8; MAX_PAYLOAD_BYTES];
				let _ = e.recv_into(&mut buf).await;
			})
		})
		.collect();
	a.validate_path(b_addr).await.unwrap();
	for pump in pumps {
		pump.abort();
	}

	// Not validated in the other direction: refusal, not a hang.
	assert!(matches!(
		b.try_send_audio(&CALL, 1, b"x"),
		Err(MediaError::PathNotValidated)
	));

	let payload = [9_u8; 92];
	let mut sent = 0;
	let send_allocs = allocations_in(|| {
		for _ in 0..50 {
			if a.try_send_audio(&CALL, 1, &payload).is_ok() {
				sent += 1;
			}
		}
	});
	assert_eq!((sent, send_allocs), (50, 0));

	let mut buf = [0u8; MAX_PAYLOAD_BYTES];
	let meta = b.recv_into(&mut buf).await.unwrap();
	assert_eq!((meta.call_id, meta.generation, meta.len), (CALL, 1, 92));
	assert_eq!(&buf[..92], &payload);
}
