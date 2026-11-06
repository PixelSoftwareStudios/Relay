//! Start-up before the engine exists: the endpoint is served, the system's secure storage is
//! opened in the background, and clients are told which of the two they are waiting for.
//!
//! Opening the identity can block for minutes (a keychain password prompt) or fail (the user
//! said no). Neither may stop the daemon from answering: it reports `Starting` or `Failed` and
//! waits for the user's choice (retry, the file-backed store, or a reset of the identity).
//! Nothing that touches the network (peers, discovery, advertising) starts before the identity
//! is loaded.

use std::sync::{Arc, Mutex};

use relay_api::pb::{EngineInfo, Startup, StartupReason, StartupState};
use relay_api::{API_REVISION, BUILD_REVISION, StartupControl};
use relay_config::ConfigError;
use relay_config::identity::IdentityStore;
use tokio::sync::{mpsc, watch};
use tonic::Status;

/// What a client asked while the identity was not loaded.
pub(super) enum Choice {
	Retry,
	UseFileStore,
	Reset,
}

/// Where the identity is, as far as clients are concerned.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
	/// A read of the secure storage is in flight (possibly at a prompt).
	Reading,
	/// The read failed; the daemon waits for a choice.
	Failed,
	/// The engine is installed.
	Ready,
}

struct Inner {
	phase: Phase,
	startup: Startup,
	/// The store in use, for a reset after start-up.
	store: Option<Arc<dyn IdentityStore>>,
}

/// Shared between the boot task and the RPC handlers.
pub(super) struct Boot {
	inner: Mutex<Inner>,
	choices: mpsc::UnboundedSender<Choice>,
	stop: watch::Sender<bool>,
}

fn reason_of(error: &ConfigError) -> StartupReason {
	match error {
		ConfigError::IdentityMalformed(_)
		| ConfigError::BundleUnreadable(_)
		| ConfigError::UnsupportedVersion { .. } => StartupReason::IdentityUnreadable,
		_ => StartupReason::KeychainUnavailable,
	}
}

impl Boot {
	pub(super) fn new() -> (Arc<Self>, mpsc::UnboundedReceiver<Choice>) {
		let (choices, receiver) = mpsc::unbounded_channel();
		let (stop, _) = watch::channel(false);
		let boot = Arc::new(Self {
			inner: Mutex::new(Inner {
				phase: Phase::Reading,
				startup: Startup {
					state: StartupState::Starting as i32,
					reason: StartupReason::WaitingForKeychain as i32,
					detail: String::new(),
				},
				store: None,
			}),
			choices,
			stop,
		});
		(boot, receiver)
	}

	fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
		self.inner
			.lock()
			.unwrap_or_else(std::sync::PoisonError::into_inner)
	}

	pub(super) fn startup(&self) -> Startup {
		self.lock().startup.clone()
	}

	pub(super) fn reading(&self, store: &Arc<dyn IdentityStore>) {
		let mut inner = self.lock();
		inner.phase = Phase::Reading;
		inner.store = Some(Arc::clone(store));
		inner.startup = Startup {
			state: StartupState::Starting as i32,
			reason: StartupReason::WaitingForKeychain as i32,
			detail: String::new(),
		};
	}

	pub(super) fn failed(&self, error: &ConfigError) {
		let mut inner = self.lock();
		inner.phase = Phase::Failed;
		inner.startup = Startup {
			state: StartupState::Failed as i32,
			reason: reason_of(error) as i32,
			detail: error.to_string(),
		};
	}

	pub(super) fn ready(&self) {
		let mut inner = self.lock();
		inner.phase = Phase::Ready;
		inner.startup = Startup::default();
	}

	/// Resolves when a reset after start-up asked the daemon to stop.
	pub(super) fn stop_requested(&self) -> watch::Receiver<bool> {
		self.stop.subscribe()
	}

	fn info(&self) -> EngineInfo {
		EngineInfo {
			application_version: env!("CARGO_PKG_VERSION").to_owned(),
			api_major: 1,
			build_revision: BUILD_REVISION.to_owned(),
			api_revision: API_REVISION,
			startup: Some(self.startup()),
			..EngineInfo::default()
		}
	}

	/// Passes a choice to the boot task if the daemon is waiting for one.
	fn choose(&self, choice: Choice) -> Result<EngineInfo, Status> {
		let inner = self.lock();
		match inner.phase {
			Phase::Failed => {
				self.choices
					.send(choice)
					.map_err(|_| Status::unavailable("the service is stopping"))?;
				drop(inner);
				Ok(self.info())
			}
			Phase::Reading => Err(Status::failed_precondition("WAITING_FOR_KEYCHAIN")),
			Phase::Ready => Err(Status::failed_precondition("ALREADY_RUNNING")),
		}
	}
}

#[tonic::async_trait]
impl StartupControl for Boot {
	fn info(&self) -> EngineInfo {
		Boot::info(self)
	}

	async fn retry(&self) -> Result<EngineInfo, Status> {
		if self.lock().phase == Phase::Ready {
			return Ok(self.info());
		}
		self.choose(Choice::Retry)
	}

	async fn use_file_store(&self) -> Result<EngineInfo, Status> {
		self.choose(Choice::UseFileStore)
	}

	async fn reset_identity(&self) -> Result<EngineInfo, Status> {
		let phase = self.lock().phase;
		match phase {
			Phase::Failed => self.choose(Choice::Reset),
			Phase::Reading => Err(Status::failed_precondition("WAITING_FOR_KEYCHAIN")),
			Phase::Ready => {
				let store = self
					.lock()
					.store
					.clone()
					.ok_or_else(|| Status::failed_precondition("NO_IDENTITY_STORE"))?;
				tokio::task::spawn_blocking(move || store.delete())
					.await
					.map_err(|e| Status::internal(e.to_string()))?
					.map_err(|e| Status::internal(e.to_string()))?;
				tracing::warn!("the identity was deleted on request; the service stops");
				self.stop.send_replace(true);
				Ok(self.info())
			}
		}
	}
}
