#![cfg(unix)]

mod support;

use relay_api::{RelayClient, Revision};
use support::Harness;

#[tokio::test]
async fn the_daemon_revision_comes_from_the_connection() {
	let h = Harness::start().await;
	let client = RelayClient::connect(&h.endpoint()).await.unwrap();
	let revision = client.daemon_revision();
	assert_eq!(
		revision,
		Revision {
			build: "test".into(),
			api: 0
		}
	);
	assert!(!revision.is_current(), "another build than this program");
	assert!(Revision::local().is_current());
}
