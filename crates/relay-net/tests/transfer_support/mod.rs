#![allow(dead_code)]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use relay_messages::transfer::{TransferMsg, Verdict};
use relay_net::AddressPolicy;
use relay_net::identity::{Fingerprint, Identity};
use relay_net::transfer::{
	DataLink, DataRx, DataTx, Expectation, HubConfig, Ticket, TransferHub, TransferRole,
};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::net::TcpStream;

pub const OFFER: [u8; 16] = [1; 16];
pub const SESSION: [u8; 16] = [2; 16];

pub fn config(rate: Option<u64>) -> HubConfig {
	HubConfig {
		rate_per_link: rate,
		address_policy: AddressPolicy::lan_and_loopback(),
		..HubConfig::default()
	}
}

pub struct Peer {
	pub identity: Arc<Identity>,
	pub hub: TransferHub,
}

pub async fn peer(config: HubConfig) -> Peer {
	let identity = Arc::new(Identity::generate().unwrap());
	let hub = TransferHub::bind(
		"127.0.0.1:0".parse().unwrap(),
		Arc::clone(&identity),
		config,
	)
	.await
	.unwrap();
	Peer { identity, hub }
}

pub fn fp(p: &Peer) -> Fingerprint {
	p.identity.fingerprint()
}

pub fn expectation(peer: &Fingerprint, role: TransferRole, size: u64) -> Expectation {
	Expectation {
		offer_id: OFFER,
		session_id: SESSION,
		peer: *peer,
		role,
		size,
	}
}

pub fn addr(p: &Peer) -> SocketAddr {
	p.hub.local_addr()
}

pub fn ticket(token: relay_net::transfer::TransferToken) -> Ticket {
	Ticket {
		session_id: SESSION,
		offer_id: OFFER,
		token,
	}
}

/// Pumps a whole file from sender to receiver over established links; returns the received
/// bytes and the verdict the sender saw.
pub async fn pump<A, B, C, D>(
	data: Vec<u8>,
	sender: (DataTx<A>, DataRx<B>),
	receiver: (DataTx<C>, DataRx<D>),
) -> (Vec<u8>, Verdict)
where
	A: AsyncWrite + Unpin + Send + 'static,
	B: AsyncRead + Unpin + Send + 'static,
	C: AsyncWrite + Unpin + Send + 'static,
	D: AsyncRead + Unpin + Send + 'static,
{
	let size = data.len() as u64;
	let (mut stx, mut srx) = sender;
	let (mut rtx, mut rrx) = receiver;
	let send = tokio::spawn(async move {
		let mut credit = 0u64;
		let mut offset = 0usize;
		while offset < data.len() {
			while (offset as u64) >= credit {
				match srx.recv().await.unwrap() {
					TransferMsg::Credit(n) => credit = n,
					other => panic!("unexpected {other:?}"),
				}
			}
			let end = (offset + 64 * 1024).min(data.len());
			stx.send(&TransferMsg::Chunk {
				offset: offset as u64,
				data: data[offset..end].to_vec(),
			})
			.await
			.unwrap();
			offset = end;
		}
		stx.send(&TransferMsg::Done).await.unwrap();
		loop {
			match srx.recv().await.unwrap() {
				TransferMsg::Result(v) => return v,
				TransferMsg::Credit(_) => {}
				other => panic!("unexpected {other:?}"),
			}
		}
	});
	rtx.send(&TransferMsg::Credit(size)).await.unwrap();
	let mut got = Vec::new();
	loop {
		match rrx.recv().await.unwrap() {
			TransferMsg::Chunk { offset, data } => {
				assert_eq!(offset, got.len() as u64);
				got.extend_from_slice(&data);
			}
			TransferMsg::Done => break,
			other => panic!("unexpected {other:?}"),
		}
	}
	rtx.send(&TransferMsg::Result(Verdict::Ok)).await.unwrap();
	(got, send.await.unwrap())
}

/// Receiver listens, sender dials; returns both links.
pub async fn connect(
	rx_peer: &Peer,
	tx_peer: &Peer,
	size: u64,
) -> (DataLink<TcpStream>, DataLink<TcpStream>) {
	let (token, pending) = rx_peer
		.hub
		.expect(
			expectation(&fp(tx_peer), TransferRole::Receiver, size),
			None,
		)
		.unwrap();
	let (expected, t) = (fp(rx_peer), ticket(token));
	let dial = tx_peer
		.hub
		.dial(addr(rx_peer), &expected, &t, TransferRole::Sender, size);
	let (link_rx, link_tx) = tokio::join!(pending.wait(), dial);
	(link_rx.unwrap(), link_tx.unwrap())
}

pub fn pattern(len: usize) -> Vec<u8> {
	(0..len)
		.map(|i| (i.wrapping_mul(31) ^ (i >> 7)) as u8)
		.collect()
}

pub async fn within<T>(d: Duration, f: impl std::future::Future<Output = T>) -> T {
	tokio::time::timeout(d, f).await.expect("timed out")
}
