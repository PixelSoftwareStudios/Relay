mod common;

use relay_net::handshake::{HandshakeError, HandshakeGate, initiate, respond};
use relay_net::identity::Identity;
use relay_net::session::Role;
use tokio::io::{AsyncReadExt, AsyncWriteExt, duplex};
use tokio::net::{TcpListener, TcpStream};

#[tokio::test]
async fn handshake_authenticates_both_static_keys() {
	let link = common::link().await;
	assert_eq!(link.initiator.role(), Role::Initiator);
	assert_eq!(link.responder.role(), Role::Responder);
	assert_eq!(link.initiator.epoch(), link.responder.epoch());
	assert_eq!(
		link.initiator.remote_fingerprint(),
		link.responder_identity.fingerprint()
	);
	assert_eq!(
		link.responder.remote_fingerprint(),
		link.initiator_identity.fingerprint()
	);
}

#[tokio::test]
async fn each_handshake_uses_a_fresh_epoch() {
	let a = common::link().await;
	let b = common::link().await;
	assert_ne!(a.initiator.epoch(), b.initiator.epoch());
}

#[tokio::test]
async fn pinned_peer_mismatch_fails_before_revealing_our_key() {
	let client = Identity::generate().unwrap();
	let server = Identity::generate().unwrap();
	let other = Identity::generate().unwrap();
	let (mut a, mut b) = duplex(8192);
	let (c, s) = tokio::join!(
		async move { initiate(&mut a, &client, Some(&other.fingerprint())).await },
		async { respond(&mut b, &server).await },
	);
	assert!(matches!(c, Err(HandshakeError::UnexpectedPeer)));
	assert!(s.is_err(), "responder must not complete without message 3");
}

#[tokio::test]
async fn bad_preface_magic_is_rejected() {
	let id = Identity::generate().unwrap();
	let (mut a, mut b) = duplex(8192);
	let mut preface = [0u8; 24];
	preface[..4].copy_from_slice(b"XXXX");
	a.write_all(&preface).await.unwrap();
	assert!(matches!(
		respond(&mut b, &id).await,
		Err(HandshakeError::BadPreface)
	));
}

#[tokio::test]
async fn unsupported_major_is_rejected_without_fallback() {
	let id = Identity::generate().unwrap();
	let (mut a, mut b) = duplex(8192);
	let mut preface = [0u8; 24];
	preface[..4].copy_from_slice(b"RLH1");
	preface[4..6].copy_from_slice(&2u16.to_be_bytes());
	a.write_all(&preface).await.unwrap();
	assert!(matches!(
		respond(&mut b, &id).await,
		Err(HandshakeError::UnsupportedMajor(2))
	));
}

#[tokio::test]
async fn nonzero_reserved_preface_flags_are_rejected() {
	let id = Identity::generate().unwrap();
	let (mut a, mut b) = duplex(8192);
	let mut preface = [0u8; 24];
	preface[..4].copy_from_slice(b"RLH1");
	preface[5] = 1;
	preface[7] = 1;
	a.write_all(&preface).await.unwrap();
	assert!(matches!(
		respond(&mut b, &id).await,
		Err(HandshakeError::BadPreface)
	));
}

#[tokio::test]
async fn oversize_and_zero_length_handshake_frames_are_rejected_before_allocation() {
	for len in [0u32, 4097, u32::MAX] {
		let id = Identity::generate().unwrap();
		let (mut a, mut b) = duplex(8192);
		let mut preface = [0u8; 24];
		preface[..4].copy_from_slice(b"RLH1");
		preface[4..6].copy_from_slice(&1u16.to_be_bytes());
		a.write_all(&preface).await.unwrap();
		a.write_all(&len.to_be_bytes()).await.unwrap();
		assert!(
			matches!(
				respond(&mut b, &id).await,
				Err(HandshakeError::BadMessageLength)
			),
			"len {len}"
		);
	}
}

#[tokio::test]
async fn garbage_handshake_message_fails() {
	let id = Identity::generate().unwrap();
	let (mut a, mut b) = duplex(8192);
	let mut preface = [0u8; 24];
	preface[..4].copy_from_slice(b"RLH1");
	preface[4..6].copy_from_slice(&1u16.to_be_bytes());
	a.write_all(&preface).await.unwrap();
	a.write_all(&40u32.to_be_bytes()).await.unwrap();
	a.write_all(&[0xAB; 40]).await.unwrap();
	// Message 1 of XX is an ephemeral key; any 32+ bytes parse, so the failure appears at msg 3.
	drop(a);
	assert!(respond(&mut b, &id).await.is_err());
}

#[tokio::test]
async fn closed_connection_mid_handshake_fails() {
	let id = Identity::generate().unwrap();
	let (a, mut b) = duplex(8192);
	drop(a);
	assert!(matches!(
		respond(&mut b, &id).await,
		Err(HandshakeError::Io(_))
	));
}

#[tokio::test]
async fn tampered_epoch_in_preface_breaks_the_prologue_binding() {
	let server_id = Identity::generate().unwrap();
	let client_id = Identity::generate().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let (server_addr, proxy_addr) = (listener.local_addr().unwrap(), proxy.local_addr().unwrap());

	// On-path attacker that flips one epoch bit and relays everything else untouched.
	tokio::spawn(async move {
		let (mut from_client, _) = proxy.accept().await.unwrap();
		let mut to_server = TcpStream::connect(server_addr).await.unwrap();
		let mut preface = [0u8; 24];
		from_client.read_exact(&mut preface).await.unwrap();
		preface[10] ^= 1;
		to_server.write_all(&preface).await.unwrap();
		let _ = tokio::io::copy_bidirectional(&mut from_client, &mut to_server).await;
	});

	let server = tokio::spawn(async move {
		let (mut s, _) = listener.accept().await.unwrap();
		respond(&mut s, &server_id).await.is_ok()
	});
	let mut stream = TcpStream::connect(proxy_addr).await.unwrap();
	let client = initiate(&mut stream, &client_id, None).await;
	assert!(
		client.is_err(),
		"mismatched prologue must not yield a session"
	);
	server.abort();
}

#[tokio::test]
async fn pending_handshake_gate_is_bounded() {
	let gate = HandshakeGate::with_capacity(2);
	let a = gate.try_enter().unwrap();
	let _b = gate.try_enter().unwrap();
	assert!(matches!(gate.try_enter(), Err(HandshakeError::Overloaded)));
	drop(a);
	assert!(gate.try_enter().is_ok());
}
