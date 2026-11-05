//! Shared test harness: an in-process fake engine served over a real UDS.

#![allow(dead_code)]

mod fake;

use std::path::PathBuf;
use std::sync::Arc;

use relay_api::{ApiLimits, ClientRegistry, Endpoint, RelayClient, UdsListener, serve_unix};
use tempfile::TempDir;
use tokio::sync::oneshot;

pub use fake::FakeEngine;

pub struct Harness {
	pub dir: TempDir,
	pub socket: PathBuf,
	pub engine: FakeEngine,
	pub registry: Arc<ClientRegistry>,
	shutdown: Option<oneshot::Sender<()>>,
}

impl Harness {
	pub async fn start() -> Self {
		Self::start_with(ApiLimits::DEFAULT).await
	}

	pub async fn start_with(limits: ApiLimits) -> Self {
		let dir = tempfile::Builder::new()
			.prefix("rly")
			.tempdir()
			.expect("tempdir");
		let socket = dir.path().join("run").join("engine.sock");
		let registry = Arc::new(ClientRegistry::new(limits.max_clients));
		let engine = FakeEngine::new(Arc::clone(&registry), limits);
		let listener = UdsListener::bind(&socket).await.expect("bind");
		let (tx, rx) = oneshot::channel();
		let served = engine.clone();
		let validator = Arc::clone(&registry);
		tokio::spawn(async move {
			let _ = serve_unix(served, listener, validator, limits, async {
				let _ = rx.await;
			})
			.await;
		});
		Self {
			dir,
			socket,
			engine,
			registry,
			shutdown: Some(tx),
		}
	}

	pub fn endpoint(&self) -> Endpoint {
		Endpoint::Unix(self.socket.clone())
	}

	pub async fn client(&self) -> RelayClient {
		let mut client = RelayClient::connect(&self.endpoint())
			.await
			.expect("connect");
		client
			.register("test", "0.0.0", &["observe"])
			.await
			.expect("register");
		client
	}
}

impl Drop for Harness {
	fn drop(&mut self) {
		if let Some(tx) = self.shutdown.take() {
			let _ = tx.send(());
		}
	}
}
