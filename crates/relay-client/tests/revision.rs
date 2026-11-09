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
			api: relay_api::API_REVISION,
			version: relay_api::VERSION.into(),
		}
	);
	// Staleness follows the API revision and the release version: another build text is not stale.
	assert!(revision.is_current(), "same API revision as this program");
	assert!(Revision::local().is_current());
	let older = Revision {
		build: "test".into(),
		api: relay_api::API_REVISION - 1,
		version: relay_api::VERSION.into(),
	};
	assert!(!older.is_current(), "an older API revision is stale");
	// Same API revision, another release (an update replaced the app): stale as well.
	let other_release = Revision {
		version: "0.0.1-other".into(),
		..Revision::local()
	};
	assert!(!other_release.is_current(), "another release is stale");
}
