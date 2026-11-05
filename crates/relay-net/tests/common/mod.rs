#![allow(dead_code)]

use std::sync::Arc;

use relay_net::handshake::{initiate, respond};
use relay_net::identity::Identity;
use relay_net::session::SecureSession;
use tokio::net::{TcpListener, TcpStream};

pub struct Link {
	pub initiator: Arc<SecureSession>,
	pub responder: Arc<SecureSession>,
	pub initiator_stream: TcpStream,
	pub responder_stream: TcpStream,
	pub initiator_identity: Identity,
	pub responder_identity: Identity,
}

/// Runs a real handshake over a loopback TCP connection.
pub async fn link() -> Link {
	let initiator_identity = Identity::generate().unwrap();
	let responder_identity = Identity::generate().unwrap();
	let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
	let addr = listener.local_addr().unwrap();

	let server = async {
		let (mut stream, _) = listener.accept().await.unwrap();
		let session = respond(&mut stream, &responder_identity).await.unwrap();
		(stream, session)
	};
	let client = async {
		let mut stream = TcpStream::connect(addr).await.unwrap();
		let session = initiate(&mut stream, &initiator_identity, None)
			.await
			.unwrap();
		(stream, session)
	};
	let ((responder_stream, responder), (initiator_stream, initiator)) =
		tokio::join!(server, client);
	Link {
		initiator: Arc::new(initiator),
		responder: Arc::new(responder),
		initiator_stream,
		responder_stream,
		initiator_identity,
		responder_identity,
	}
}

/// Two independent handshakes; handy for epoch and cross-session tests.
pub async fn sessions() -> (Arc<SecureSession>, Arc<SecureSession>) {
	let l = link().await;
	(l.initiator, l.responder)
}
