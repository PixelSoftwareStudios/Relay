//! The engine process: loads configuration and identity, wires the peer and
//! media adapters, serves the local API and shuts down gracefully.

mod assemble;
mod boot;
mod channel_store;
mod env;
mod error;
mod favorites;
mod glue;
mod notice;
mod options;
mod settings;
mod transport;

use std::sync::Arc;
use std::time::Duration;

use relay_api::pb::engine_server::EngineServer;
use relay_api::{ClientValidator, Endpoint, EngineSlot, EngineSwitch, LateValidator};
use relay_config::Notice;
use relay_config::identity::{IdentityStore, StoredIdentity};
use relay_core::DeviceKeyId;
use tokio::sync::oneshot;
use tokio::task::JoinHandle;

pub use channel_store::SqliteChannelStore;
pub use env::Environment;
pub use error::DaemonError;
pub use favorites::FavoriteBook;
pub use options::{
	AdapterFactory, AudioSetup, BackendFactory, NetworkOptions, RunOptions, Transport,
};
pub use settings::FileSettings;

use crate::service::{
	ControlHandle, EngineService, GateReader, Service, ServiceBuilder, ServiceConfig,
};

/// How long open streams get to finish after the service stopped.
const SERVER_DRAIN: Duration = Duration::from_secs(2);
/// Time for the final hangup messages to leave before links close.
const SIGNAL_FLUSH: Duration = Duration::from_millis(100);

/// The endpoint being served and what must go with it.
struct Serving {
	leftover: std::path::PathBuf,
	shutdown: oneshot::Sender<()>,
	server: JoinHandle<Result<(), relay_api::ApiError>>,
}

impl Serving {
	/// Stops the server and removes the endpoint files.
	async fn stop(self) -> Result<(), DaemonError> {
		let Self {
			leftover,
			shutdown,
			mut server,
		} = self;
		let _ = shutdown.send(());
		let result = match tokio::time::timeout(SERVER_DRAIN, &mut server).await {
			Ok(Ok(served)) => served.map_err(DaemonError::from),
			Ok(Err(_)) => Ok(()),
			Err(_) => {
				server.abort();
				Ok(())
			}
		};
		let _ = std::fs::remove_file(&leftover);
		result
	}
}

/// A running daemon, for tests and embedding.
pub struct DaemonHandle {
	endpoint: Endpoint,
	serving: Serving,
	service: Service,
	adapters: Option<assemble::Adapters>,
	control: ControlHandle,
	gates: GateReader,
	notices: Vec<Notice>,
	local: DeviceKeyId,
	listen: Option<std::net::SocketAddr>,
	requested: tokio::sync::watch::Receiver<bool>,
	stopped: tokio::sync::watch::Receiver<bool>,
}

/// A daemon that serves its endpoint but has not opened its identity yet. [`Booting::ready`]
/// finishes the start.
pub struct Booting {
	serving: Serving,
	endpoint: Endpoint,
	options: RunOptions,
	opened: env::Opened,
	store: Arc<dyn IdentityStore>,
	/// Whether `store` was supplied by the caller rather than chosen by the settings.
	supplied: bool,
	boot: Arc<boot::Boot>,
	choices: tokio::sync::mpsc::UnboundedReceiver<boot::Choice>,
	slot: EngineSlot<EngineServer<EngineService>>,
	validator: Arc<LateValidator>,
}

impl DaemonHandle {
	/// Loads state, starts the adapters, binds the endpoint and serves. A failure to open the
	/// identity is an error here; see [`DaemonHandle::begin`] for a daemon that waits instead.
	pub async fn start(options: RunOptions) -> Result<Self, DaemonError> {
		Self::begin(options).await?.load(false).await
	}

	/// Opens the settings, binds the endpoint and serves it at once. The identity is opened
	/// by [`Booting::ready`], so clients can reach the daemon while the system's secure storage
	/// asks the user for permission.
	pub async fn begin(mut options: RunOptions) -> Result<Booting, DaemonError> {
		let opened = env::open(options.paths.clone())?;
		let (supplied, store) = match options.identity_store.take() {
			Some(store) => (true, Arc::from(store)),
			None => (false, opened.identity_store()?),
		};
		let (boot, choices) = boot::Boot::new();
		let (switch, slot) = EngineSwitch::new(boot.clone());
		let validator = Arc::new(LateValidator::default());
		// Claim the endpoint first: a second engine fails before it touches the network, the
		// audio devices or the secure storage.
		let bound = transport::bind(&options.transport).await?;
		let endpoint = bound.endpoint();
		let leftover = bound.leftover();
		let (shutdown, signal) = oneshot::channel();
		let server = bound.serve(switch, validator.clone(), options.api, async {
			let _ = signal.await;
		});
		Ok(Booting {
			serving: Serving {
				leftover,
				shutdown,
				server,
			},
			endpoint,
			options,
			opened,
			store,
			supplied,
			boot,
			choices,
			slot,
			validator,
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

	/// The system now allows (or may allow) using the microphone, for example a
	/// permission was granted while the app ran: a capture that failed to open
	/// is tried again without a restart.
	pub async fn input_access_changed(&self) {
		let _ = self
			.control
			.system(relay_core::SystemEvent::InputAccessChanged)
			.await;
	}

	/// The published transmit gates.
	pub fn gates(&self) -> &GateReader {
		&self.gates
	}

	/// Resolves once an admin client asked the engine to stop (`Shutdown`).
	pub async fn shutdown_requested(&self) {
		let mut requested = self.requested.clone();
		let mut stopped = self.stopped.clone();
		// A closed channel means the service is gone, which is as good as a stop.
		tokio::select! {
			_ = requested.wait_for(|asked| *asked) => {}
			_ = stopped.wait_for(|asked| *asked) => {}
		}
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
			serving,
			service,
			adapters,
			..
		} = self;
		service.stop().await;
		if let Some(adapters) = adapters {
			tokio::time::sleep(SIGNAL_FLUSH).await;
			adapters.shutdown().await;
		}
		serving.stop().await
	}
}

impl Booting {
	/// Where clients connect (already served).
	pub fn endpoint(&self) -> &Endpoint {
		&self.endpoint
	}

	/// How far the start-up is, as clients see it.
	pub fn startup(&self) -> relay_api::pb::Startup {
		self.boot.startup()
	}

	/// Opens the identity and finishes the start. The system may ask the user for permission;
	/// that wait does not block the runtime. If opening fails the daemon stays up as `Failed`
	/// and waits for a retry, the file-backed store or an identity reset.
	pub async fn ready(self) -> Result<DaemonHandle, DaemonError> {
		self.load(true).await
	}

	/// Stops serving and removes the endpoint files.
	pub async fn abandon(self) -> Result<(), DaemonError> {
		self.serving.stop().await
	}

	async fn load(mut self, stay_up: bool) -> Result<DaemonHandle, DaemonError> {
		let identity = loop {
			self.boot.reading(&self.store);
			let store = Arc::clone(&self.store);
			let read = tokio::task::spawn_blocking(move || env::load_identity(&*store)).await;
			let error = match read {
				Ok(Ok(identity)) => break identity,
				Ok(Err(error)) => error,
				Err(joined) => relay_config::ConfigError::CredentialStore(joined.to_string()),
			};
			tracing::warn!(%error, "the secure storage could not be opened");
			if !stay_up {
				let _ = self.serving.stop().await;
				return Err(error.into());
			}
			self.boot.failed(&error);
			if !self.wait_for_choice().await? {
				return Err(error.into());
			}
		};
		match self.finish(identity).await {
			Ok(handle) => Ok(handle),
			Err((error, serving)) => {
				let _ = serving.stop().await;
				Err(error)
			}
		}
	}

	/// Waits while `Failed` for the user's choice and applies it. `false` when the daemon was
	/// asked to stop instead.
	async fn wait_for_choice(&mut self) -> Result<bool, DaemonError> {
		loop {
			let Some(choice) = self.choices.recv().await else {
				return Ok(false);
			};
			match choice {
				boot::Choice::Retry => return Ok(true),
				boot::Choice::UseFileStore => {
					tracing::warn!("the user chose the file-backed store for identity and secrets");
					self.opened.choose_file_store()?;
					self.store = self.opened.identity_store()?;
					self.supplied = false;
					return Ok(true);
				}
				boot::Choice::Reset => {
					tracing::warn!("the stored identity is deleted on request");
					let store = Arc::clone(&self.store);
					match tokio::task::spawn_blocking(move || store.delete()).await {
						Ok(Ok(())) => return Ok(true),
						Ok(Err(error)) => self.boot.failed(&error),
						Err(joined) => {
							self.boot
								.failed(&relay_config::ConfigError::CredentialStore(
									joined.to_string(),
								))
						}
					}
				}
			}
		}
	}

	async fn finish(
		self,
		identity: StoredIdentity,
	) -> Result<DaemonHandle, (DaemonError, Serving)> {
		let Self {
			serving,
			endpoint,
			mut options,
			opened,
			store,
			boot,
			slot,
			validator,
			..
		} = self;
		match Self::assemble(&mut options, opened, &*store, identity, &slot, &validator).await {
			Ok(parts) => {
				boot.ready();
				// A reset after start-up asks the daemon to stop; the store is kept for it.
				let stop = boot.stop_requested();
				Ok(DaemonHandle {
					endpoint,
					serving,
					service: parts.service,
					adapters: parts.adapters,
					control: parts.control,
					gates: parts.gates,
					notices: parts.notices,
					local: parts.local,
					listen: parts.listen,
					requested: parts.requested,
					stopped: stop,
				})
			}
			Err(error) => Err((error, serving)),
		}
	}
}

struct Assembled {
	service: Service,
	adapters: Option<assemble::Adapters>,
	control: ControlHandle,
	gates: GateReader,
	notices: Vec<Notice>,
	local: DeviceKeyId,
	listen: Option<std::net::SocketAddr>,
	requested: tokio::sync::watch::Receiver<bool>,
}

impl Booting {
	async fn assemble(
		options: &mut RunOptions,
		opened: env::Opened,
		store: &dyn IdentityStore,
		identity: StoredIdentity,
		slot: &EngineSlot<EngineServer<EngineService>>,
		validator: &Arc<LateValidator>,
	) -> Result<Assembled, DaemonError> {
		let loaded = env::finish(
			opened,
			store,
			identity,
			options.channel_vault.take(),
			options.default_display_name.take(),
		)?;
		let mut config = ServiceConfig::new(loaded.local);
		config.limits = options.limits.clone();
		config.api = options.api;
		config.idle_client_ttl = options.idle_client_ttl;
		config.presence_debounce = options.presence_debounce;
		config.idle_exit = options.idle_exit;
		config.warnings = loaded.notices.iter().map(notice::problem).collect();
		if let Some(entropy) = options.entropy.take() {
			config.entropy = entropy;
		}
		let builder = ServiceBuilder::new(config)?;
		let control = builder.handle();
		let gates = builder.gates();
		let registry = builder.registry();
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
		let (ports, adapters, listen) = match options.adapters.take() {
			Some(factory) => (factory(&environment), None, None),
			None => {
				let clock = options
					.channel_clock
					.take()
					.unwrap_or_else(crate::channels::Clock::system);
				let (ports, adapters) = assemble::assemble(
					&environment,
					options.network.clone(),
					std::mem::replace(&mut options.audio, AudioSetup::Cpal),
					options.mic_test_max,
					clock,
					options.messaging.clone(),
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
		let validator_registry: Arc<dyn ClientValidator> = registry;
		validator.install(validator_registry);
		slot.install(relay_api::engine_server(service.api.clone(), &options.api));
		Ok(Assembled {
			service,
			adapters,
			control,
			gates,
			notices,
			local,
			listen,
			requested,
		})
	}
}

/// Runs the engine until SIGINT or SIGTERM. The endpoint is served from the start; while the
/// system's secure storage is opened (or waits for the user) the daemon reports that state
/// instead of exiting.
pub async fn run(options: RunOptions) -> Result<(), DaemonError> {
	let booting = DaemonHandle::begin(options).await?;
	tracing::info!(
		endpoint = %relay_client_describe(booting.endpoint()),
		"engine listening, opening the identity"
	);
	let daemon = tokio::select! {
		ready = booting.ready() => ready?,
		signal = wait_for_signal() => {
			signal?;
			tracing::info!("stopped before the identity was opened");
			return Ok(());
		}
	};
	tracing::info!("engine ready");
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
