//! The engine process: loads configuration and identity, wires the peer and
//! media adapters, serves the local API and shuts down gracefully.

mod assemble;
mod channel_store;
mod env;
mod error;
mod glue;
mod notice;
mod options;
mod settings;
mod transport;

use std::sync::Arc;
use std::time::Duration;

use relay_api::{ClientValidator, Endpoint};
use relay_config::Notice;
use relay_core::DeviceKeyId;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub use channel_store::SqliteChannelStore;
pub use env::Environment;
pub use error::DaemonError;
pub use options::{
	AdapterFactory, AudioSetup, BackendFactory, NetworkOptions, RunOptions, Transport,
};
pub use settings::FileSettings;

use crate::service::{ControlHandle, GateReader, Service, ServiceBuilder, ServiceConfig};

/// How long open streams get to finish after the service stopped.
const SERVER_DRAIN: Duration = Duration::from_secs(2);
/// Time for the final hangup messages to leave before links close.
const SIGNAL_FLUSH: Duration = Duration::from_millis(100);

/// A running daemon, for tests and embedding.
pub struct DaemonHandle {
	endpoint: Endpoint,
	leftover: std::path::PathBuf,
	service: Service,
	adapters: Option<assemble::Adapters>,
	shutdown: oneshot::Sender<()>,
	server: JoinHandle<Result<(), relay_api::ApiError>>,
	control: ControlHandle,
	gates: GateReader,
	notices: Vec<Notice>,
	local: DeviceKeyId,
	listen: Option<std::net::SocketAddr>,
	requested: tokio::sync::watch::Receiver<bool>,
}

impl DaemonHandle {
	/// Loads state, starts the adapters, binds the endpoint and serves.
	pub async fn start(options: RunOptions) -> Result<Self, DaemonError> {
		let RunOptions {
			paths,
			transport,
			identity_store,
			network,
			audio,
			adapters,
			limits,
			api,
			idle_client_ttl,
			entropy,
			mic_test_max,
			presence_debounce,
			idle_exit,
			channel_vault,
			channel_clock,
			messaging,
		} = options;
		let loaded = env::load(paths, identity_store, channel_vault)?;
		let mut config = ServiceConfig::new(loaded.local);
		config.limits = limits;
		config.api = api;
		config.idle_client_ttl = idle_client_ttl;
		config.presence_debounce = presence_debounce;
		config.idle_exit = idle_exit;
		config.warnings = loaded.notices.iter().map(notice::problem).collect();
		if let Some(entropy) = entropy {
			config.entropy = entropy;
		}
		let builder = ServiceBuilder::new(config)?;
		let control = builder.handle();
		let gates = builder.gates();
		let registry = builder.registry();

		// Claim the endpoint first: a second engine fails before it touches
		// the network or the audio devices.
		let bound = transport::bind(&transport).await?;
		let notices = loaded.notices.clone();
		let local = loaded.local;
		let environment = Environment {
			paths: loaded.paths,
			lock: loaded.lock,
			preferences: loaded.preferences,
			notices: loaded.notices,
			local,
			identity: loaded.identity,
			identity_storage: loaded.identity_storage,
			transport_identity: loaded.transport_identity,
			trust: loaded.trust,
			incarnation: registry.incarnation(),
			settings: loaded.settings,
			avatars: loaded.avatars,
			channels: loaded.channels,
			handle: control.clone(),
			gates: gates.clone(),
		};
		let (ports, adapters, listen) = match adapters {
			Some(factory) => (factory(&environment), None, None),
			None => {
				let clock = channel_clock.unwrap_or_else(crate::channels::Clock::system);
				let (ports, adapters) = assemble::assemble(
					&environment,
					network,
					audio,
					mic_test_max,
					clock,
					messaging,
				)
				.await?;
				let listen = Some(adapters.listen_addr());
				(ports, Some(adapters), listen)
			}
		};
		let builder = match adapters.as_ref() {
			Some(adapters) => builder
				.with_channels(adapters.channels())
				.with_messaging(adapters.messaging()),
			None => builder,
		};
		let service = builder.start(ports)?;
		let requested = service.shutdown_requested();
		let endpoint = bound.endpoint();
		let leftover = bound.leftover();
		let (shutdown, signal) = oneshot::channel();
		let validator: Arc<dyn ClientValidator> = registry;
		let server = bound.serve(service.api.clone(), validator, api, async {
			let _ = signal.await;
		});
		Ok(Self {
			endpoint,
			leftover,
			service,
			adapters,
			shutdown,
			server,
			control,
			gates,
			notices,
			local,
			listen,
			requested,
		})
	}

	/// Where clients connect.
	pub fn endpoint(&self) -> &Endpoint {
		&self.endpoint
	}

	/// The peer-link address other engines dial, once the real adapters run.
	pub fn listen_addr(&self) -> Option<std::net::SocketAddr> {
		self.listen
	}

	/// The channel driver, when the real adapters run.
	pub fn channels(&self) -> Option<crate::channels::ChannelHub> {
		self.adapters.as_ref().map(assemble::Adapters::channels)
	}

	/// Handle for injecting adapter events (tests, embedding).
	pub fn control(&self) -> &ControlHandle {
		&self.control
	}

	/// The published transmit gates.
	pub fn gates(&self) -> &GateReader {
		&self.gates
	}

	/// Resolves once an admin client asked the engine to stop (`Shutdown`).
	pub async fn shutdown_requested(&self) {
		let mut requested = self.requested.clone();
		// A closed channel means the service is gone, which is as good as a stop.
		let _ = requested.wait_for(|asked| *asked).await;
	}

	/// This device's identity digest.
	pub fn local_peer(&self) -> DeviceKeyId {
		self.local
	}

	/// Recovery notices raised while loading.
	pub fn notices(&self) -> &[Notice] {
		&self.notices
	}

	/// Stops the control actor (closing every gate, releasing every client
	/// and ending every call), then the adapters and the server, and removes
	/// the endpoint files.
	pub async fn shutdown(self) -> Result<(), DaemonError> {
		let Self {
			leftover,
			service,
			adapters,
			shutdown,
			mut server,
			..
		} = self;
		service.stop().await;
		if let Some(adapters) = adapters {
			tokio::time::sleep(SIGNAL_FLUSH).await;
			adapters.shutdown().await;
		}
		let _ = shutdown.send(());
		match tokio::time::timeout(SERVER_DRAIN, &mut server).await {
			Ok(Ok(served)) => served?,
			Ok(Err(_)) => {}
			Err(_) => server.abort(),
		}
		let _ = std::fs::remove_file(&leftover);
		Ok(())
	}
}

/// Runs the engine until SIGINT or SIGTERM.
pub async fn run(options: RunOptions) -> Result<(), DaemonError> {
	let daemon = DaemonHandle::start(options).await?;
	tracing::info!(
		endpoint = %relay_client_describe(daemon.endpoint()),
		"engine listening"
	);
	tokio::select! {
		signal = wait_for_signal() => signal?,
		() = daemon.shutdown_requested() => tracing::info!("stop requested through the API"),
	}
	tracing::info!("shutting down");
	daemon.shutdown().await
}

fn relay_client_describe(endpoint: &Endpoint) -> String {
	match endpoint {
		#[cfg(unix)]
		Endpoint::Unix(path) => path.display().to_string(),
		Endpoint::LoopbackTcp { profile_path } => format!("tcp via {}", profile_path.display()),
	}
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
