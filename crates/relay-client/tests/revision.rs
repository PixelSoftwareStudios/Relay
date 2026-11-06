#![cfg(unix)]

#[allow(dead_code)]
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
			api: relay_api::API_REVISION
		}
	);
	// Staleness follows the API revision alone: another build text is not stale.
	assert!(revision.is_current(), "same API revision as this program");
	assert!(Revision::local().is_current());
	let older = Revision {
		build: "test".into(),
		api: relay_api::API_REVISION - 1,
	};
	assert!(!older.is_current(), "an older API revision is stale");
}
