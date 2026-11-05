#![no_main]

use std::sync::{Arc, OnceLock};
use std::time::Instant;

use libfuzzer_sys::fuzz_target;
use relay_net::handshake::{initiate, respond};
use relay_net::identity::Identity;
use relay_net::media::MediaReceiver;
use relay_net::session::SecureSession;
use tokio::net::{TcpListener, TcpStream};

/// One real loopback handshake, shared by every input.
fn receiver_session() -> &'static Arc<SecureSession> {
	static SESSION: OnceLock<Arc<SecureSession>> = OnceLock::new();
	SESSION.get_or_init(|| {
		let runtime = tokio::runtime::Builder::new_current_thread()
			.enable_all()
			.build()
			.unwrap();
		runtime.block_on(async {
			let initiator = Identity::generate().unwrap();
			let responder = Identity::generate().unwrap();
			let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
			let addr = listener.local_addr().unwrap();
			let server = async {
				let (mut stream, _) = listener.accept().await.unwrap();
				respond(&mut stream, &responder).await.unwrap()
			};
			let client = async {
				let mut stream = TcpStream::connect(addr).await.unwrap();
				initiate(&mut stream, &initiator, None).await.unwrap()
			};
			let (session, _) = tokio::join!(server, client);
			Arc::new(session)
		})
	})
}

fuzz_target!(|data: &[u8]| {
	// Arbitrary bytes were never sealed by the peer, so they must always be refused.
	let mut receiver = MediaReceiver::new(Arc::clone(receiver_session()));
	assert!(receiver.receive(data, Instant::now()).is_err());
});
