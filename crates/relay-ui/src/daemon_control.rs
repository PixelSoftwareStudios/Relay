//! Starting and stopping the standalone daemon process from the UI, in words a
//! person can act on. The desktop shell calls these; they never need the
//! `relay` command line, and they never run unless the user asked.

use std::time::Duration;

use crate::tr;
use relay_api::Endpoint;
use relay_client::daemon::{self, Launch, StartOptions, Started, Stopped};
use relay_client::refresh::{self, Outcome, Policy, RefreshError};

use crate::actions::Shared;
use crate::autostart::AutoStart;
use crate::hub::Hub;
use crate::model::Link;

const CLIENT_NAME: &str = "relay-desktop";
const STOP_WAIT: Duration = Duration::from_secs(10);

/// The text for a finished start attempt: a note to show, or why it failed.
pub fn start_message(outcome: Started) -> Result<String, String> {
	match outcome {
		// Starting on the user's behalf is silent: only failures are shown.
		Started::Launched { .. } | Started::AlreadyRunning => Ok(String::new()),
		Started::FailedToStart { reason, log_tail } => {
			let last = log_tail.lines().last().unwrap_or("").trim();
			if last.is_empty() {
				Err(tr!("daemon-start-failed", reason = reason))
			} else {
				Err(tr!("daemon-start-failed-log", reason = reason, last = last))
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
		Err(error) => Err(tr!("daemon-stop-failed", error = error.to_string())),
	}
}

/// The text for a finished reset: nothing on success, or what to do about the failure.
pub fn reset_message(
	result: Result<Vec<std::path::PathBuf>, relay_client::reset::ResetError>,
) -> Result<(), String> {
	use relay_client::reset::ResetError;
	match result {
		Ok(_) => Ok(()),
		Err(ResetError::DaemonRunning) => Err(tr!("reset-err-running").to_owned()),
		Err(ResetError::Unavailable(error)) => Err(tr!("reset-err-unavailable", error = error)),
		Err(ResetError::Incomplete { failed, .. }) => {
			let files = failed
				.iter()
				.map(|(path, why)| {
					let name = path
						.file_name()
						.map_or_else(String::new, |n| n.to_string_lossy().into_owned());
					format!("{name} ({why})")
				})
				.collect::<Vec<_>>()
				.join(", ");
			Err(tr!("reset-err-incomplete", files = files))
		}
	}
}

/// Deletes the database and avatar cache of this user (not the identity), off the UI thread.
pub async fn reset_local_data() -> Result<(), String> {
	let result = tokio::task::spawn_blocking(relay_client::reset::reset_platform)
		.await
		.map_err(|e| e.to_string())?;
	reset_message(result)
}

/// Asks the running background service to read the secure storage again.
pub async fn retry_secure_storage(endpoint: &Endpoint) -> Result<(), String> {
	let mut client = relay_api::RelayClient::connect(endpoint)
		.await
		.map_err(|e| e.to_string())?;
	client.retry_keychain().await.map_err(|e| e.to_string())?;
	Ok(())
}

/// Tells the running background service to use the file-backed store, then retry.
pub async fn use_file_storage(endpoint: &Endpoint) -> Result<(), String> {
	let mut client = relay_api::RelayClient::connect(endpoint)
		.await
		.map_err(|e| e.to_string())?;
	client.use_file_store().await.map_err(|e| e.to_string())?;
	Ok(())
}

/// What the launch-time refresh of the daemon led to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchRefresh {
	/// Automatic starts are off (setting, `RELAY_NO_AUTOSTART` or a quit): nothing was touched.
	Skipped,
	/// No daemon answered, another client was starting one, or it could not be inspected.
	Nothing,
	/// An idle daemon was stopped and a fresh one started from this app's build.
	Restarted,
	/// A call is in progress: the daemon was left alone (a stale one shows the banner).
	KeptBusy,
	/// The daemon is still opening its secure storage or waits for the user to choose: it was
	/// left alone and the app shows what it waits for.
	KeptStarting,
	/// The old daemon would not stop or the new one did not start; the text says why.
	Failed(String),
}

/// Turns what the refresh found into [`LaunchRefresh`].
pub fn launch_result(result: Result<Outcome, RefreshError>) -> LaunchRefresh {
	match result {
		Ok(Outcome::Restarted { .. }) => LaunchRefresh::Restarted,
		Ok(Outcome::KeptBusy { .. }) => LaunchRefresh::KeptBusy,
		Ok(Outcome::Starting) => LaunchRefresh::KeptStarting,
		Ok(Outcome::NotRunning | Outcome::Current | Outcome::Contended) => LaunchRefresh::Nothing,
		Err(RefreshError::Probe(why)) => {
			tracing::debug!(%why, "the running daemon was not inspected at launch");
			LaunchRefresh::Nothing
		}
		Err(RefreshError::StopTimedOut(why)) => {
			LaunchRefresh::Failed(tr!("daemon-stop-failed", error = why))
		}
		Err(RefreshError::Start(reason)) => {
			LaunchRefresh::Failed(tr!("daemon-start-failed", reason = reason))
		}
	}
}

/// At app launch, before the first connect: an already running daemon that is idle is
/// stopped and replaced by one from this app's own build, so a binary left over from an
/// earlier build never lingers. A call in progress keeps the daemon. Other attached clients
/// do not matter: they reconnect through their own supervisors. Runs under the daemon start
/// lock, so a CLI autostarting at the same moment cannot race it. A service that is still
/// starting or waits for the user (keychain) is never touched, and the app replaces the service
/// at most once per launch.
pub async fn restart_idle_at_launch(
	endpoint: &Endpoint,
	autostart: &AutoStart,
	hub: &Hub,
	shared: &Shared,
) -> LaunchRefresh {
	if let Some(stored) = stored_autostart(endpoint).await {
		autostart.set_setting(stored);
	}
	if !autostart.enabled() || autostart.paused() || autostart.launch_restarted() {
		return LaunchRefresh::Skipped;
	}
	hub.update(|s| s.set_link_down(Link::Starting, None));
	let result = refresh::refresh_endpoint(
		endpoint,
		&StartOptions::default(),
		CLIENT_NAME,
		Policy::AnyIdle,
		STOP_WAIT,
	)
	.await;
	let outcome = launch_result(result);
	if outcome == LaunchRefresh::Restarted {
		autostart.set_launch_restarted();
	}
	if let LaunchRefresh::Failed(message) = &outcome {
		shared.set_error(message.clone());
	}
	outcome
}

/// How often [`restart_when_idle`] looks at the calls.
const IDLE_POLL: Duration = Duration::from_secs(2);

/// The deferred case of [`restart_idle_at_launch`]: a call was active, so the daemon was kept.
/// When the app is connected and no call is in progress any more, the daemon is replaced
/// without a click (the app owns the daemon's life cycle). Returns after one attempt that
/// found the daemon idle; the stale-service card clears when the new daemon reports itself.
pub async fn restart_when_idle(
	endpoint: &Endpoint,
	autostart: &AutoStart,
	hub: &Hub,
	shared: &Shared,
) -> LaunchRefresh {
	loop {
		tokio::time::sleep(IDLE_POLL).await;
		let idle = hub.read(|s| {
			s.link == Link::Connected && s.calls.iter().all(|call| call.phase.is_terminal())
		});
		if !idle {
			continue;
		}
		match restart_idle_at_launch(endpoint, autostart, hub, shared).await {
			LaunchRefresh::KeptBusy => {}
			other => return other,
		}
	}
}

/// The stored "Start daemon automatically" choice of a running daemon, if one answers.
async fn stored_autostart(endpoint: &Endpoint) -> Option<bool> {
	if !daemon::reachable(endpoint).await {
		return None;
	}
	let mut client = relay_api::RelayClient::connect(endpoint).await.ok()?;
	client
		.register(CLIENT_NAME, env!("CARGO_PKG_VERSION"), &["observe"])
		.await
		.ok()?;
	let snapshot = client.snapshot().await.ok()?;
	snapshot.settings.map(|s| s.autostart_daemon)
}
