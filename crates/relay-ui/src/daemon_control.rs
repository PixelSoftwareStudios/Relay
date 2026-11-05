//! Starting and stopping the standalone `relayd` from the UI, in words a
//! person can act on. The desktop shell calls these; they never need the
//! `relay` command line, and they never run unless the user asked.

use std::time::Duration;

use relay_api::Endpoint;
use relay_client::daemon::{self, Launch, Started, Stopped};

const CLIENT_NAME: &str = "relay-desktop";
const STOP_WAIT: Duration = Duration::from_secs(10);

/// The text for a finished start attempt: a note to show, or why it failed.
pub fn start_message(outcome: Started) -> Result<String, String> {
	match outcome {
		Started::Launched { .. } => Ok("The Relay daemon started.".to_owned()),
		Started::AlreadyRunning => Ok("The Relay daemon is already running.".to_owned()),
		Started::BinaryNotFound { .. } => Err(format!(
			"Cannot find the Relay daemon ({}). Install it next to this app or on your PATH, \
			 or set {} to its location.",
			daemon::binary_name(),
			daemon::BINARY_ENV
		)),
		Started::FailedToStart { reason, log_tail } => {
			let last = log_tail.lines().last().unwrap_or("").trim();
			if last.is_empty() {
				Err(format!("The Relay daemon did not start: {reason}."))
			} else {
				Err(format!(
					"The Relay daemon did not start: {reason}. Last log line: {last}"
				))
			}
		}
	}
}

/// Launches the daemon for `endpoint`, detached from this app.
pub async fn start(endpoint: &Endpoint) -> Result<String, String> {
	start_message(daemon::start(&Launch::new(endpoint.clone())).await)
}

/// Asks the daemon to stop (an explicit `admin` request, separate from the
/// app's own session) and waits until its endpoint is gone.
pub async fn stop(endpoint: &Endpoint) -> Result<(), String> {
	match daemon::stop(endpoint, CLIENT_NAME, STOP_WAIT).await {
		Ok(Stopped::Stopped | Stopped::NotRunning) => Ok(()),
		Err(error) => Err(format!("The Relay daemon did not stop: {error}")),
	}
}
