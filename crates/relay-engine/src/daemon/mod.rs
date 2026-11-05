//! The engine process: loads configuration and identity, serves the local
//! API on an owner-only socket and shuts down gracefully.

mod env;
mod error;
mod settings;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use relay_api::pb::Problem;
use relay_api::{
	ApiLimits, ClientValidator, UdsListener, current_uid, default_socket_path, serve_unix,
};
use relay_config::identity::IdentityStore;
use relay_config::{Notice, Paths};
use relay_core::Limits;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub use env::Environment;
pub use error::DaemonError;
pub use settings::FileSettings;

use crate::service::{
	ControlHandle, Entropy, GateReader, Ports, Service, ServiceBuilder, ServiceConfig,
};

/// How long open streams get to finish after the service stopped.
const SERVER_DRAIN: Duration = Duration::from_secs(2);

/// Builds the adapters once the environment (and the control handle they
/// report to) exists.
pub type AdapterFactory = Box<dyn FnOnce(&Environment) -> Ports + Send>;

/// Everything the daemon needs to start.
pub struct RunOptions {
	/// Where configuration and data live.
	pub paths: Paths,
	/// Socket to bind; `None` uses the per-user default.
	pub socket: Option<PathBuf>,
	/// Identity storage override; `None` follows the preferences.
	pub identity_store: Option<Box<dyn IdentityStore>>,
	/// Adapter factory; `None` serves settings only and reports every other
	/// capability as unsupported.
	pub adapters: Option<AdapterFactory>,
	/// Domain limits (may only tighten the design ceilings).
	pub limits: Limits,
	/// Transport limits.
	pub api: ApiLimits,
	/// Quiet time after which a client without streams is detached.
	pub idle_client_ttl: Duration,
	/// Identifier randomness; `None` uses the OS.
	pub entropy: Option<Arc<dyn Entropy>>,
}

impl RunOptions {
	/// Defaults for the given directories.
	pub fn new(paths: Paths) -> Self {
		Self {
			paths,
			socket: None,
			identity_store: None,
			adapters: None,
			limits: Limits::default(),
			api: ApiLimits::DEFAULT,
			idle_client_ttl: Duration::from_secs(300),
			entropy: None,
		}
	}
}

fn notice_problem(notice: &Notice) -> Problem {
	let (code, message) = match notice {
		Notice::PreferencesRecoveredFromBackup => (
			"PREFERENCES_RECOVERED",
			"Preferences were damaged; the backup was used and the microphone selection cleared."
				.to_owned(),
		),
		Notice::PreferencesReset { .. } => (
			"PREFERENCES_RESET",
			"Preferences were unreadable and were reset; no microphone is selected.".to_owned(),
		),
		Notice::PreferencesMigrated { from, to } => (
			"PREFERENCES_MIGRATED",
			format!("Preferences were migrated from version {from} to {to}."),
		),
		Notice::TrustRecoveredFromBackup => (
			"TRUST_RECOVERED",
			"The trust store was damaged; recent entries may need re-verification.".to_owned(),
		),
		Notice::TrustStoreLost { .. } => (
			"TRUST_LOST",
			"The trust store was lost; every peer must be verified again.".to_owned(),
		),
	};
	Problem {
		code: code.to_owned(),
		message,
		retryable: false,
		recovery_action: "review".to_owned(),
	}
}

/// A running daemon, for tests and embedding.
pub struct DaemonHandle {
	socket: PathBuf,
	service: Service,
	shutdown: oneshot::Sender<()>,
	server: JoinHandle<Result<(), relay_api::ApiError>>,
	control: ControlHandle,
	gates: GateReader,
	notices: Vec<Notice>,
}

impl DaemonHandle {
	/// Loads state, binds the socket and starts serving.
	pub async fn start(options: RunOptions) -> Result<Self, DaemonError> {
		let RunOptions {
			paths,
			socket,
			identity_store,
			adapters,
			limits,
			api,
			idle_client_ttl,
			entropy,
		} = options;
		let loaded = env::load(paths, identity_store)?;
		let mut config = ServiceConfig::new(loaded.local);
		config.limits = limits;
		config.api = api;
		config.idle_client_ttl = idle_client_ttl;
		config.warnings = loaded.notices.iter().map(notice_problem).collect();
		if let Some(entropy) = entropy {
			config.entropy = entropy;
		}
		let builder = ServiceBuilder::new(config)?;
		let control = builder.handle();
		let gates = builder.gates();
		let registry = builder.registry();

		let notices = loaded.notices.clone();
		let environment = Environment {
			paths: loaded.paths,
			lock: loaded.lock,
			preferences: loaded.preferences,
			notices: loaded.notices,
			local: loaded.local,
			identity: loaded.identity,
			identity_storage: loaded.identity_storage,
			trust: loaded.trust,
			settings: loaded.settings,
			avatars: loaded.avatars,
			handle: control.clone(),
			gates: gates.clone(),
		};
		let ports = match adapters {
			Some(factory) => factory(&environment),
			None => {
				let mut ports = Ports::unwired();
				ports.settings = environment.settings.clone();
				ports
			}
		};

		let socket = match socket {
			Some(path) => path,
			None => default_socket_path(
				std::env::var_os("XDG_RUNTIME_DIR"),
				&std::env::temp_dir(),
				current_uid()?,
			),
		};
		let listener = UdsListener::bind(&socket).await?;
		let service = builder.start(ports)?;
		let (shutdown, signal) = oneshot::channel();
		let validator: Arc<dyn ClientValidator> = registry;
		let server = tokio::spawn(serve_unix(
			service.api.clone(),
			listener,
			validator,
			api,
			async {
				let _ = signal.await;
			},
		));
		Ok(Self {
			socket,
			service,
			shutdown,
			server,
			control,
			gates,
			notices,
		})
	}

	/// The socket clients connect to.
	pub fn socket_path(&self) -> &std::path::Path {
		&self.socket
	}

	/// Handle for injecting adapter events (tests, embedding).
	pub fn control(&self) -> &ControlHandle {
		&self.control
	}

	/// The published transmit gates.
	pub fn gates(&self) -> &GateReader {
		&self.gates
	}

	/// Recovery notices raised while loading.
	pub fn notices(&self) -> &[Notice] {
		&self.notices
	}

	/// Stops the control actor (closing every gate and releasing every
	/// client), then the server, and removes the socket.
	pub async fn shutdown(self) -> Result<(), DaemonError> {
		let Self {
			socket,
			service,
			shutdown,
			mut server,
			..
		} = self;
		service.stop().await;
		let _ = shutdown.send(());
		match tokio::time::timeout(SERVER_DRAIN, &mut server).await {
			Ok(Ok(served)) => served?,
			Ok(Err(_)) => {}
			Err(_) => server.abort(),
		}
		let _ = std::fs::remove_file(&socket);
		Ok(())
	}
}

/// Runs the engine until SIGINT or SIGTERM.
pub async fn run(options: RunOptions) -> Result<(), DaemonError> {
	let daemon = DaemonHandle::start(options).await?;
	tracing::info!(socket = %daemon.socket_path().display(), "engine listening");
	wait_for_signal().await?;
	tracing::info!("shutting down");
	daemon.shutdown().await
}

#[cfg(unix)]
async fn wait_for_signal() -> Result<(), DaemonError> {
	use tokio::signal::unix::{SignalKind, signal};
	let mut terminate = signal(SignalKind::terminate())?;
	tokio::select! {
		result = tokio::signal::ctrl_c() => result?,
		_ = terminate.recv() => {}
	}
	Ok(())
}

#[cfg(not(unix))]
async fn wait_for_signal() -> Result<(), DaemonError> {
	tokio::signal::ctrl_c().await?;
	Ok(())
}
