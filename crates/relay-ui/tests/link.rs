use relay_api::Endpoint;
use relay_ui::engine::{Engine, EngineError};
use relay_ui::relay_link::RelayLink;

#[tokio::test]
async fn a_missing_socket_reads_as_daemon_unavailable() {
	let dir = std::env::temp_dir().join(format!("relay-desktop-test-{}", std::process::id()));
	let link = RelayLink::new(move || Ok(Endpoint::Unix(dir.join("absent.sock"))));
	let Err(error) = link.connect().await else {
		panic!("connect must fail without a daemon");
	};
	assert!(matches!(error, EngineError::Unavailable(_)), "{error}");
}
