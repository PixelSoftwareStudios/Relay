mod common;

use std::sync::Arc;
use std::time::Duration;

use proptest::prelude::*;
use relay_messages::transfer::TransferMsg;
use relay_net::record::RecordKind;
use relay_net::session::SecureSession;
use relay_net::transfer::{DataError, DataLink, LinkParams, Pacer, TransferRole};
use tokio::io::{AsyncWriteExt, DuplexStream, duplex};

const SIZE: u64 = 100_000;

fn frame(session: &SecureSession, kind: RecordKind, body: &[u8]) -> Vec<u8> {
	let record = session.seal(kind, body).unwrap();
	let mut out = (record.len() as u32).to_be_bytes().to_vec();
	out.extend_from_slice(&record);
	out
}

fn chunk(offset: u64, len: usize) -> Vec<u8> {
	let mut b = vec![1];
	b.extend_from_slice(&offset.to_be_bytes());
	b.extend(std::iter::repeat_n(7u8, len));
	b
}

fn credit(n: u64) -> Vec<u8> {
	let mut b = vec![3];
	b.extend_from_slice(&n.to_be_bytes());
	b
}

/// A link of `role` whose peer is a raw stream we control.
async fn victim(role: TransferRole) -> (Arc<SecureSession>, DuplexStream, DataLink<DuplexStream>) {
	let (init, resp) = common::sessions().await;
	let (raw, near) = duplex(1 << 22);
	let mut params = LinkParams::new(role, SIZE, [1; 16], [2; 16]);
	params.idle_timeout = Duration::from_millis(300);
	(
		init,
		raw,
		DataLink::new(near, resp, params, Pacer::new(None, None)),
	)
}

#[tokio::test]
async fn hostile_frames_to_a_receiver_are_refused() {
	// Fresh sessions per case: each case needs its own matching pair.
	for case in 0..9 {
		let (s, mut raw, mut link) = victim(TransferRole::Receiver).await;
		let bytes = match case {
			0 => frame(&s, RecordKind::Data, &chunk(5, 10)), // gap at start
			1 => frame(&s, RecordKind::Data, &chunk(0, 32 * 1024 + 1)), // oversize
			2 => frame(&s, RecordKind::Data, &chunk(0, 0)),  // empty chunk
			3 => frame(&s, RecordKind::Data, &chunk(SIZE - 5, 10)), // past the end
			4 => frame(&s, RecordKind::Data, &[2]),          // done too early
			5 => frame(&s, RecordKind::Data, &credit(10)),   // wrong direction
			6 => frame(&s, RecordKind::Data, &[99, 1, 2]),   // unknown tag
			7 => frame(&s, RecordKind::Envelope, &chunk(0, 10)), // wrong kind
			_ => {
				let mut b = frame(&s, RecordKind::Data, &chunk(0, 10));
				let n = b.len();
				b[n - 1] ^= 1; // tampered
				b
			}
		};
		raw.write_all(&bytes).await.unwrap();
		let r = link.rx.recv().await;
		assert!(r.is_err(), "case {case}: {r:?}");
		assert!(matches!(link.rx.recv().await, Err(DataError::Closed)));
	}
}

#[tokio::test]
async fn out_of_order_and_repeated_chunks_are_refused() {
	let (s, mut raw, mut link) = victim(TransferRole::Receiver).await;
	raw.write_all(&frame(&s, RecordKind::Data, &chunk(0, 100)))
		.await
		.unwrap();
	assert!(matches!(
		link.rx.recv().await,
		Ok(TransferMsg::Chunk { .. })
	));
	// Same offset again: "negative progress".
	raw.write_all(&frame(&s, RecordKind::Data, &chunk(0, 100)))
		.await
		.unwrap();
	assert!(matches!(link.rx.recv().await, Err(DataError::BadFrame(_))));
}

#[tokio::test]
async fn replayed_record_is_refused() {
	let (s, mut raw, mut link) = victim(TransferRole::Receiver).await;
	let f = frame(&s, RecordKind::Data, &chunk(0, 100));
	raw.write_all(&f).await.unwrap();
	raw.write_all(&f).await.unwrap();
	assert!(link.rx.recv().await.is_ok());
	assert!(matches!(link.rx.recv().await, Err(DataError::BadFrame(_))));
}

#[tokio::test]
async fn hostile_frames_to_a_sender_are_refused() {
	for case in 0..4 {
		let (s, mut raw, mut link) = victim(TransferRole::Sender).await;
		let mut bytes = Vec::new();
		match case {
			0 => bytes = frame(&s, RecordKind::Data, &credit(SIZE + 1)),
			1 => {
				bytes.extend(frame(&s, RecordKind::Data, &credit(500)));
				bytes.extend(frame(&s, RecordKind::Data, &credit(400))); // backwards
			}
			2 => bytes = frame(&s, RecordKind::Data, &chunk(0, 10)),
			_ => bytes = frame(&s, RecordKind::Data, &[2]),
		}
		raw.write_all(&bytes).await.unwrap();
		let mut result = link.rx.recv().await;
		if case == 1 {
			assert!(result.is_ok());
			result = link.rx.recv().await;
		}
		assert!(result.is_err(), "case {case}: {result:?}");
	}
}

#[tokio::test]
async fn absurd_length_prefix_is_refused_without_allocating() {
	for len in [0u32, 1, 40, u32::MAX, 1 << 20] {
		let (_, mut raw, mut link) = victim(TransferRole::Receiver).await;
		raw.write_all(&len.to_be_bytes()).await.unwrap();
		assert!(link.rx.recv().await.is_err(), "len {len}");
	}
}

#[tokio::test]
async fn silent_peer_hits_the_idle_timeout() {
	let (_, _raw, mut link) = victim(TransferRole::Receiver).await;
	assert!(matches!(link.rx.recv().await, Err(DataError::Idle)));
}

#[tokio::test]
async fn sender_side_refuses_misuse_locally() {
	let (_, _raw, mut link) = victim(TransferRole::Sender).await;
	let bad = [
		TransferMsg::Credit(1),
		TransferMsg::Accept,
		TransferMsg::Done, // before every byte was sent
	];
	for m in bad {
		let (_, _raw, mut l) = victim(TransferRole::Sender).await;
		assert!(matches!(l.tx.send(&m).await, Err(DataError::BadFrame(_))));
	}
	let out_of_order = TransferMsg::Chunk {
		offset: 3,
		data: vec![1],
	};
	assert!(link.tx.send(&out_of_order).await.is_err());
	let (_, _raw, mut l) = victim(TransferRole::Sender).await;
	let past = TransferMsg::Chunk {
		offset: 0,
		data: vec![1; SIZE as usize + 1],
	};
	assert!(l.tx.send(&past).await.is_err());
}

proptest! {
	#![proptest_config(ProptestConfig::with_cases(48))]

	#[test]
	fn arbitrary_bodies_never_panic_and_stay_bounded(
		body in proptest::collection::vec(any::<u8>(), 0..2000),
		receiver in any::<bool>(),
	) {
		let rt = tokio::runtime::Builder::new_current_thread().enable_all().build().unwrap();
		rt.block_on(async {
			let role = if receiver { TransferRole::Receiver } else { TransferRole::Sender };
			let (s, mut raw, mut link) = victim(role).await;
			raw.write_all(&frame(&s, RecordKind::Data, &body)).await.unwrap();
			if let Ok(TransferMsg::Chunk { data, .. }) = link.rx.recv().await {
				assert!(data.len() <= 32 * 1024);
			}
		});
	}
}
