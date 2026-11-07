//! A second, tiny engine client that carries the "this shell alerts calls itself"
//! flag (`SetClientQuiet`).
//!
//! The flag belongs to a client that holds a state watch and ends with it, and the
//! UI's own session lives inside relay-ui, out of reach. So the shell registers its
//! own client, opens a watch, sends the flag and keeps the watch open while the
//! flag is wanted, reconnecting (and re-sending) whenever the connection drops.
//! The engine rings if it is alone or any attached client is not quiet: with only
//! this client attached the engine stays silent and Android's notification alerts;
//! with the app open the UI's client is not quiet, so the engine rings as before.

use std::time::Duration;

use relay_api::Endpoint;
use relay_client::wire::connect_registered;

const NAME: &str = "relay-android-alert";
const CAPABILITIES: &[&str] = &["observe", "call-control"];
const WATCH_QUEUE: usize = 16;
const RETRY: Duration = Duration::from_secs(2);

/// Holds a quiet client until the task is aborted (dropping it closes the watch,
/// which ends the flag).
pub async fn hold(endpoint: Endpoint) {
	loop {
		if let Err(error) = hold_once(&endpoint).await {
			tracing::info!(%error, "quiet client lost; retrying");
		}
		tokio::time::sleep(RETRY).await;
	}
}

async fn hold_once(endpoint: &Endpoint) -> Result<(), relay_api::ApiError> {
	let mut client =
		connect_registered(endpoint, NAME, env!("CARGO_PKG_VERSION"), CAPABILITIES).await?;
	let (_snapshot, watch) = client.snapshot_and_watch().await?;
	let mut watch = watch.into_bounded(WATCH_QUEUE);
	client.set_client_quiet(true).await?;
	tracing::info!("quiet client attached: the shell alerts incoming calls");
	while let Some(item) = watch.recv().await {
		item?;
	}
	Ok(())
}
