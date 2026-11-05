mod common;
mod transfer_support;

use std::time::Duration;

use relay_messages::transfer::{TransferMsg, Verdict};
use relay_net::identity::Identity;
use relay_net::transfer::{DataError, TransferRole, TransferToken};
use transfer_support::*;

#[tokio::test]
async fn files_of_every_size_transfer_intact_and_verify() {
	for len in [0usize, 1, 32 * 1024, 32 * 1024 + 1, 3 * 1024 * 1024 + 17] {
		let (a, b) = (peer(config(None)).await, peer(config(None)).await);
		let (rx_link, tx_link) = connect(&a, &b, len as u64).await;
		assert_eq!(rx_link.peer(), &fp(&b));
		assert_eq!(tx_link.peer(), &fp(&a));
		let data = pattern(len);
		let (got, verdict) = within(
			Duration::from_secs(60),
			pump(
				data.clone(),
				(tx_link.tx, tx_link.rx),
				(rx_link.tx, rx_link.rx),
			),
		)
		.await;
		assert_eq!(blake3::hash(&got), blake3::hash(&data), "len {len}");
		assert_eq!(verdict, Verdict::Ok);
	}
}

#[tokio::test]
async fn link_slots_are_released_when_the_link_ends() {
	let (a, b) = (peer(config(None)).await, peer(config(None)).await);
	let (rx_link, tx_link) = connect(&a, &b, 1).await;
	assert_eq!(a.hub.open_links(), 1);
	assert_eq!(b.hub.open_links(), 1);
	drop((rx_link, tx_link));
	assert_eq!((a.hub.open_links(), b.hub.open_links()), (0, 0));
	assert_eq!(a.hub.pending_tokens(), 0);
}

/// Dials with the given ticket and reports whether the listener accepted (a refusal is a
/// plain close, observed as `Closed` on the first receive).
async fn rejected(a: &Peer, dialer: &Peer, token: TransferToken, expected_ok: bool) {
	let link = dialer
		.hub
		.dial(addr(a), &fp(a), &ticket(token), TransferRole::Sender, 10)
		.await
		.unwrap();
	let (_tx, mut rx) = (link.tx, link.rx);
	let r = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await;
	if expected_ok {
		assert!(r.is_err(), "link was closed: {r:?}");
	} else {
		assert!(matches!(r, Ok(Err(DataError::Closed))), "{r:?}");
	}
}

#[tokio::test]
async fn wrong_token_is_refused_and_the_real_one_still_works() {
	let (a, b) = (peer(config(None)).await, peer(config(None)).await);
	let (token, pending) = a
		.hub
		.expect(expectation(&fp(&b), TransferRole::Receiver, 10), None)
		.unwrap();
	rejected(&a, &b, TransferToken::random(), false).await;
	rejected(&a, &b, token, true).await;
	assert!(pending.wait().await.is_ok());
}

#[tokio::test]
async fn token_is_revoked_after_repeated_wrong_attempts() {
	let (a, b) = (peer(config(None)).await, peer(config(None)).await);
	let (token, pending) = a
		.hub
		.expect(expectation(&fp(&b), TransferRole::Receiver, 10), None)
		.unwrap();
	for _ in 0..3 {
		rejected(&a, &b, TransferToken::random(), false).await;
	}
	rejected(&a, &b, token, false).await;
	assert!(matches!(pending.wait().await, Err(DataError::Expired)));
}

#[tokio::test]
async fn reused_token_is_refused() {
	let (a, b) = (peer(config(None)).await, peer(config(None)).await);
	let (token, pending) = a
		.hub
		.expect(expectation(&fp(&b), TransferRole::Receiver, 10), None)
		.unwrap();
	let again = token.clone();
	rejected(&a, &b, token, true).await;
	let _first = pending.wait().await.unwrap();
	rejected(&a, &b, again, false).await;
}

#[tokio::test]
async fn expired_token_is_refused() {
	let (a, b) = (peer(config(None)).await, peer(config(None)).await);
	let (token, pending) = a
		.hub
		.expect(
			expectation(&fp(&b), TransferRole::Receiver, 10),
			Some(Duration::from_millis(100)),
		)
		.unwrap();
	tokio::time::sleep(Duration::from_millis(250)).await;
	rejected(&a, &b, token, false).await;
	assert!(matches!(pending.wait().await, Err(DataError::Expired)));
}

#[tokio::test]
async fn a_different_peer_key_cannot_use_the_token_and_does_not_burn_it() {
	let (a, b) = (peer(config(None)).await, peer(config(None)).await);
	let intruder = peer(config(None)).await;
	let (token, pending) = a
		.hub
		.expect(expectation(&fp(&b), TransferRole::Receiver, 10), None)
		.unwrap();
	for _ in 0..5 {
		rejected(&a, &intruder, token.clone(), false).await;
	}
	rejected(&a, &b, token, true).await;
	assert!(pending.wait().await.is_ok());
}

#[tokio::test]
async fn dialer_pins_the_listener_key_before_revealing_the_token() {
	let (a, b) = (peer(config(None)).await, peer(config(None)).await);
	let (token, _pending) = a
		.hub
		.expect(expectation(&fp(&b), TransferRole::Receiver, 10), None)
		.unwrap();
	let other = Identity::generate().unwrap().fingerprint();
	let r = b
		.hub
		.dial(addr(&a), &other, &ticket(token), TransferRole::Sender, 10)
		.await;
	assert!(matches!(r, Err(DataError::Handshake(_))));
	assert_eq!(a.hub.pending_tokens(), 1, "token untouched");
}

#[tokio::test]
async fn control_handshake_cannot_be_used_as_a_data_link() {
	let (a, b) = (peer(config(None)).await, peer(config(None)).await);
	let (_token, pending) = a
		.hub
		.expect(expectation(&fp(&b), TransferRole::Receiver, 10), None)
		.unwrap();
	let mut s = tokio::net::TcpStream::connect(addr(&a)).await.unwrap();
	let r = relay_net::handshake::initiate(&mut s, &b.identity, None).await;
	assert!(
		r.is_err()
			|| tokio::io::AsyncReadExt::read(&mut s, &mut [0u8; 1])
				.await
				.is_ok()
	);
	drop(pending);
}

#[tokio::test]
async fn cancel_from_either_side_ends_the_link_promptly() {
	let (a, b) = (peer(config(None)).await, peer(config(None)).await);
	let (mut rx_link, mut tx_link) = connect(&a, &b, 1 << 20).await;
	tx_link.tx.send(&TransferMsg::Cancel).await.unwrap();
	assert_eq!(rx_link.rx.recv().await.unwrap(), TransferMsg::Cancel);
	drop(tx_link);
	assert!(matches!(rx_link.rx.recv().await, Err(DataError::Closed)));

	let (mut rx_link, mut tx_link) = connect(&a, &b, 1 << 20).await;
	rx_link.tx.send(&TransferMsg::Cancel).await.unwrap();
	assert_eq!(tx_link.rx.recv().await.unwrap(), TransferMsg::Cancel);
}

#[tokio::test]
async fn close_handle_unblocks_a_send_stuck_on_the_pacer() {
	// 1 KiB/s with a 64 KiB burst: the second chunk would wait about a minute.
	let (a, b) = (peer(config(None)).await, peer(config(Some(1024))).await);
	let (_rx_link, mut tx_link) = connect(&a, &b, 1 << 20).await;
	let close = tx_link.close_handle();
	let sender = tokio::spawn(async move {
		let mut off = 0u64;
		loop {
			let chunk = TransferMsg::Chunk {
				offset: off,
				data: vec![0; 32 * 1024],
			};
			if let Err(e) = tx_link.tx.send(&chunk).await {
				return e;
			}
			off += 32 * 1024;
		}
	});
	tokio::time::sleep(Duration::from_millis(200)).await;
	let started = std::time::Instant::now();
	close.close();
	let e = within(Duration::from_secs(2), sender).await.unwrap();
	assert!(matches!(e, DataError::Closed));
	assert!(started.elapsed() < Duration::from_secs(1));
}

#[tokio::test]
async fn link_drop_mid_transfer_ends_the_peer_with_closed() {
	let (a, b) = (peer(config(None)).await, peer(config(None)).await);
	let (mut rx_link, mut tx_link) = connect(&a, &b, 1 << 20).await;
	tx_link
		.tx
		.send(&TransferMsg::Chunk {
			offset: 0,
			data: vec![9; 1000],
		})
		.await
		.unwrap();
	drop(tx_link);
	assert!(matches!(
		rx_link.rx.recv().await,
		Ok(TransferMsg::Chunk { .. })
	));
	assert!(matches!(rx_link.rx.recv().await, Err(DataError::Closed)));
	assert!(matches!(rx_link.rx.recv().await, Err(DataError::Closed)));
}

#[tokio::test]
async fn global_link_cap_refuses_extra_links_without_burning_their_tokens() {
	let mut cfg = config(None);
	cfg.max_links = 1;
	let a = peer(cfg).await;
	let b = peer(config(None)).await;
	let (_t1, _l1) = {
		let (token, pending) = a
			.hub
			.expect(expectation(&fp(&b), TransferRole::Receiver, 10), None)
			.unwrap();
		let l = b
			.hub
			.dial(addr(&a), &fp(&a), &ticket(token), TransferRole::Sender, 10)
			.await
			.unwrap();
		((), (pending.wait().await.unwrap(), l))
	};
	let mut e2 = expectation(&fp(&b), TransferRole::Receiver, 10);
	e2.offer_id = [7; 16];
	let (token2, pending2) = a.hub.expect(e2, None).unwrap();
	let mut t = ticket(token2);
	t.offer_id = [7; 16];
	let l2 = b
		.hub
		.dial(addr(&a), &fp(&a), &t, TransferRole::Sender, 10)
		.await
		.unwrap();
	let mut rx2 = l2.rx;
	assert!(matches!(
		tokio::time::timeout(Duration::from_millis(500), rx2.recv()).await,
		Ok(Err(DataError::Closed))
	));
	assert_eq!(a.hub.pending_tokens(), 1);
	drop(pending2);
}
