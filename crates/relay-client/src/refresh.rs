//! Replacing a stale or merely idle daemon with the running program's own build.
//!
//! A daemon that outlives a rebuild confuses a newer client ("unknown setting", missing
//! limits). A client therefore compares the daemon's revision with its own and, when the
//! daemon is idle, restarts it. What counts as idle: no call is preparing, ringing,
//! connecting, connected or interrupted. Other attached clients do not matter; they
//! reconnect through their own supervisors.
//!
//! The decision is [`refresh`], written against the small [`Daemon`] trait so it is tested
//! with fakes. [`refresh_endpoint`] is the real thing: it takes the per-endpoint start lock
//! (the one [`crate::daemon::ensure_running`] uses), so a client that autostarts at the same
//! moment never races the stop and the start.

use std::time::Duration;

use relay_api::pb::{Call, CallState};
use relay_api::{Endpoint, RelayClient, Revision};
use relay_io::IoError;
use relay_io::lock::ProcessLock;
use tokio::time::{Instant, sleep};

use crate::daemon::{self, StartOptions, Started, Stopped};

/// When to replace a running daemon.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Policy {
	/// Only a daemon whose build or API revision differs from this program's (the CLI).
	StaleOnly,
	/// Any idle daemon, current or not (the desktop app at launch).
	AnyIdle,
}

/// What a running daemon looks like to [`refresh`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Probe {
	/// Its build and API revision.
	pub revision: Revision,
	/// A call is in progress.
	pub busy: bool,
}

/// What [`refresh`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
	/// Nothing answers at the endpoint; nothing was done.
	NotRunning,
	/// The daemon was left as it is: it is current (or, with [`Policy::StaleOnly`], not stale).
	Current,
	/// The daemon should be replaced but a call is in progress, so it was left alone.
	KeptBusy {
		/// Its build differs from this program's.
		stale: bool,
		/// Its revision.
		revision: Revision,
	},
	/// The daemon was stopped and a new one started from this program's build.
	Restarted {
		/// The revision of the daemon that was replaced.
		from: Revision,
	},
	/// Another client held the start lock for the whole wait; nothing was done.
	Contended,
}

/// Why a replacement failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum RefreshError {
	/// The running daemon could not be inspected.
	#[error("cannot inspect the running daemon: {0}")]
	Probe(String),
	/// The daemon did not stop in time and is still running.
	#[error("the running daemon did not stop: {0}")]
	StopTimedOut(String),
	/// The new daemon did not start.
	#[error("the new daemon did not start: {0}")]
	Start(String),
}

/// The daemon as [`refresh`] sees it.
#[allow(async_fn_in_trait)]
pub trait Daemon {
	/// `None` when nothing answers.
	async fn probe(&mut self) -> Result<Option<Probe>, String>;
	/// Asks it to stop and waits until it is gone.
	async fn stop(&mut self) -> Result<(), String>;
	/// Starts a daemon from this program's build and waits until it answers.
	async fn start(&mut self) -> Result<(), String>;
}

/// Replaces the daemon according to `policy`.
pub async fn refresh<D: Daemon>(daemon: &mut D, policy: Policy) -> Result<Outcome, RefreshError> {
	let Some(probe) = daemon.probe().await.map_err(RefreshError::Probe)? else {
		return Ok(Outcome::NotRunning);
	};
	let stale = !probe.revision.is_current();
	if !(stale || policy == Policy::AnyIdle) {
		return Ok(Outcome::Current);
	}
	if probe.busy {
		return Ok(Outcome::KeptBusy {
			stale,
			revision: probe.revision,
		});
	}
	daemon.stop().await.map_err(RefreshError::StopTimedOut)?;
	daemon.start().await.map_err(RefreshError::Start)?;
	Ok(Outcome::Restarted {
		from: probe.revision,
	})
}

/// Whether any call is still in progress (anything that is not over or being torn down).
pub fn any_call_in_progress(calls: &[Call]) -> bool {
	calls.iter().any(|call| {
		matches!(
			CallState::try_from(call.state),
			Ok(CallState::Preparing
				| CallState::OutgoingRinging
				| CallState::IncomingRinging
				| CallState::Negotiating
				| CallState::Connected
				| CallState::Interrupted)
		)
	})
}

/// The real daemon behind an endpoint.
struct Live<'a> {
	endpoint: &'a Endpoint,
	options: &'a StartOptions,
	name: &'a str,
	stop_wait: Duration,
}

impl Daemon for Live<'_> {
	async fn probe(&mut self) -> Result<Option<Probe>, String> {
		if !daemon::reachable(self.endpoint).await {
			return Ok(None);
		}
		let mut client = RelayClient::connect(self.endpoint)
			.await
			.map_err(|e| e.to_string())?;
		let revision = client.daemon_revision();
		client
			.register(self.name, env!("CARGO_PKG_VERSION"), &["observe"])
			.await
			.map_err(|e| e.to_string())?;
		let snapshot = client.snapshot().await.map_err(|e| e.to_string())?;
		Ok(Some(Probe {
			revision,
			busy: any_call_in_progress(&snapshot.calls),
		}))
	}

	async fn stop(&mut self) -> Result<(), String> {
		match daemon::stop(self.endpoint, self.name, self.stop_wait).await {
			Ok(Stopped::Stopped | Stopped::NotRunning) => Ok(()),
			Err(error) => Err(error.to_string()),
		}
	}

	async fn start(&mut self) -> Result<(), String> {
		match daemon::start_replacement(&self.options.launch(self.endpoint)).await {
			Started::Launched { .. } | Started::AlreadyRunning => Ok(()),
			Started::BinaryNotFound { searched } => Err(format!(
				"cannot find {} (looked in {} places)",
				daemon::binary_name(),
				searched.len()
			)),
			Started::FailedToStart { reason, log_tail } => {
				Err(format!("{reason}\n{log_tail}").trim().to_owned())
			}
		}
	}
}

/// [`refresh`] on the daemon at `endpoint`, under the start lock. `name` is the client name
/// shown to the daemon; `stop_wait` bounds how long the old daemon may take to go away.
pub async fn refresh_endpoint(
	endpoint: &Endpoint,
	options: &StartOptions,
	name: &str,
	policy: Policy,
	stop_wait: Duration,
) -> Result<Outcome, RefreshError> {
	let lock_file = daemon::lock_path(endpoint);
	let deadline = Instant::now() + options.lock_timeout;
	let lock = loop {
		match ProcessLock::acquire_beside(&lock_file) {
			Ok(lock) => break lock,
			Err(IoError::Locked(_)) if Instant::now() < deadline => {
				sleep(Duration::from_millis(50)).await;
			}
			Err(IoError::Locked(_)) => return Ok(Outcome::Contended),
			Err(error) => return Err(RefreshError::Probe(format!("start lock: {error}"))),
		}
	};
	let mut live = Live {
		endpoint,
		options,
		name,
		stop_wait,
	};
	let outcome = refresh(&mut live, policy).await;
	drop(lock);
	outcome
}
