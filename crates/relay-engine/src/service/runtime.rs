//! Assembly: channel, gates, actor task and the API implementation.

use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::time::Duration;

use relay_api::{ApiLimits, ClientRegistry, StreamLimiter};
use relay_core::{ClientId, Engine, Limits, PeerId, State};

const MAX_RELEASE_DELAY_MS: u32 = 2000;
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;

use super::actor::{Actor, Attendance};
use super::clients::ClientBook;
use super::directory::{Aux, Directory};
use super::error::ServiceError;
use super::feed::Feed;
use super::gates::{GatePublisher, GateReader};
use super::handle::ControlHandle;
use super::ids::{Entropy, Ids, OsEntropy};
use super::ledger::Ledger;
use super::ports::Ports;
use super::presence::{Presence, PresenceLink};
use super::rpc::{EngineService, Shared};
use crate::channels::ChannelHub;
use crate::messaging::MessagingHandle;

/// Everything configurable about the service.
pub struct ServiceConfig {
	/// This device's identity digest.
	pub local: PeerId,
	/// Domain limits (may only tighten the design ceilings).
	pub limits: Limits,
	/// Transport and stream limits.
	pub api: ApiLimits,
	/// How long a client with no open stream may stay quiet before it is
	/// detached and everything it held is released.
	pub idle_client_ttl: Duration,
	/// Source of randomness for identifiers.
	pub entropy: Arc<dyn Entropy>,
	/// Capacity of the actor's inbox.
	pub inbox: usize,
	/// Start-up notices that clients must show (recovered files and so on).
	pub warnings: Vec<relay_api::pb::Problem>,
	/// How long the advertisement stays up after the last client detached,
	/// so a reconnecting window does not flicker.
	pub presence_debounce: Duration,
	/// Stop the engine after this long with no client attached. `None` keeps
	/// it running (service units, manual starts).
	pub idle_exit: Option<Duration>,
	/// Wall clock in unix milliseconds, used to date call history entries.
	pub wall_clock: super::recorder::WallClock,
}

impl ServiceConfig {
	/// Defaults for a device identity.
	pub fn new(local: PeerId) -> Self {
		Self {
			local,
			limits: Limits::default(),
			api: ApiLimits::DEFAULT,
			idle_client_ttl: Duration::from_secs(300),
			entropy: Arc::new(OsEntropy),
			inbox: 1024,
			warnings: Vec::new(),
			presence_debounce: Duration::from_secs(2),
			idle_exit: None,
			wall_clock: super::recorder::system_clock(),
		}
	}
}

/// A service whose channel exists but whose actor has not started yet, so
/// adapters can be given a [`ControlHandle`] before they are built.
pub struct ServiceBuilder {
	shutdown: Arc<watch::Sender<bool>>,
	presence: Arc<PresenceLink>,
	config: ServiceConfig,
	handle: ControlHandle,
	inbox: mpsc::Receiver<super::handle::Msg>,
	gates: GatePublisher,
	registry: Arc<ClientRegistry>,
	channels: Option<ChannelHub>,
	messaging: Option<MessagingHandle>,
}

impl ServiceBuilder {
	/// Validates the configuration and creates the channel and gate board.
	pub fn new(config: ServiceConfig) -> Result<Self, ServiceError> {
		config
			.limits
			.validate()
			.map_err(|e| ServiceError::Config(e.to_string()))?;
		if config.inbox == 0 {
			return Err(ServiceError::Config("inbox must not be empty".to_owned()));
		}
		let (tx, inbox) = mpsc::channel(config.inbox);
		let gates = GatePublisher::default();
		let handle = ControlHandle::new(tx, gates.reader());
		let registry = Arc::new(ClientRegistry::new(config.limits.attached_clients));
		Ok(Self {
			shutdown: Arc::new(watch::channel(false).0),
			presence: PresenceLink::new(),
			config,
			handle,
			inbox,
			gates,
			registry,
			channels: None,
			messaging: None,
		})
	}

	/// Serves the channel RPCs from this driver (without one they are unimplemented).
	pub fn with_channels(mut self, channels: ChannelHub) -> Self {
		self.channels = Some(channels);
		self
	}

	/// Serves the session messaging RPCs from this actor and feeds it the calls the control
	/// actor knows (without one they are unimplemented).
	pub fn with_messaging(mut self, messaging: MessagingHandle) -> Self {
		self.messaging = Some(messaging);
		self
	}

	/// The handle adapters use to report events.
	pub fn handle(&self) -> ControlHandle {
		self.handle.clone()
	}

	/// The reader the media thread polls.
	pub fn gates(&self) -> GateReader {
		self.gates.reader()
	}

	/// The registry that validates client headers; share it with the
	/// transport guard.
	pub fn registry(&self) -> Arc<ClientRegistry> {
		Arc::clone(&self.registry)
	}

	/// Starts the actor and the idle-client sweeper.
	pub fn start(self, ports: Ports) -> Result<Service, ServiceError> {
		let Self {
			shutdown,
			presence,
			config,
			handle,
			inbox,
			gates,
			registry,
			channels,
			messaging,
		} = self;
		let incarnation = registry.incarnation();
		// The stored push-to-talk release delay seeds the domain; later changes
		// arrive through `SetGlobalControls`.
		let stored = ports.settings.load();
		let mut limits = config.limits.clone();
		limits.ptt_release_delay = std::time::Duration::from_millis(u64::from(
			stored.ptt_release_delay_ms.min(MAX_RELEASE_DELAY_MS),
		));
		let state = State::new(
			config.local,
			relay_core::Incarnation::from_u128(u128::from_be_bytes(incarnation)),
			limits,
		)
		.map_err(|e| ServiceError::Config(e.to_string()))?;
		let ids = Ids::new(Arc::clone(&config.entropy));
		let directory = Directory::new(
			Aux::from(ports.settings.load()),
			ports.peers.peers(),
			ports.devices.devices(),
			config.warnings,
		);
		directory.set_favorites(ports.settings.favorites());
		ports.peers.profile_changed(&directory.aux.profile());
		let ledger = Ledger::new(
			config.limits.operation_cache_per_client,
			config.limits.operation_cache_global,
			config.limits.operation_retention,
		);
		let feed = Feed::new(
			config.limits.lifecycle_replay_events,
			config.api.subscriber_queue,
		);
		let book = Arc::new(ClientBook::default());
		let actor = Actor::new(
			Engine::new(state),
			ports.clone(),
			gates,
			feed,
			directory,
			ledger,
			incarnation,
			inbox,
			Attendance {
				link: Arc::clone(&presence),
				presence: Presence::new(
					tokio::time::Instant::now(),
					config.presence_debounce,
					config.idle_exit,
				),
				shutdown: Arc::clone(&shutdown),
			},
			messaging.clone(),
			config.wall_clock,
		);
		let api = EngineService::new(Shared {
			handle: handle.clone(),
			shutdown: Arc::clone(&shutdown),
			presence: Arc::clone(&presence),
			registry: Arc::clone(&registry),
			book: Arc::clone(&book),
			limiter: StreamLimiter::new(config.api.max_streams),
			ids,
			peers: ports.peers,
			media: ports.media,
			settings: ports.settings,
			started: tokio::time::Instant::now(),
			api: config.api,
			limits: config.limits,
			next_source: AtomicU64::new(1),
			channels,
			messaging,
		});
		let sweeper = tokio::spawn(sweep(
			handle.clone(),
			Arc::clone(&registry),
			book,
			config.idle_client_ttl,
		));
		Ok(Service {
			api,
			handle,
			registry,
			actor: tokio::spawn(actor.run()),
			sweeper,
			shutdown,
		})
	}
}

/// A running control service.
pub struct Service {
	/// The `relay.v1.Engine` implementation to serve.
	pub api: EngineService,
	/// Handle for adapters and tests.
	pub handle: ControlHandle,
	/// Validator to give the transport guard.
	pub registry: Arc<ClientRegistry>,
	actor: JoinHandle<()>,
	sweeper: JoinHandle<()>,
	shutdown: Arc<watch::Sender<bool>>,
}

impl Service {
	/// Turns `true` once an admin client asked the engine to stop.
	pub fn shutdown_requested(&self) -> watch::Receiver<bool> {
		self.shutdown.subscribe()
	}

	/// Stops the actor (releasing every client and call) and waits for it.
	pub async fn stop(self) {
		self.sweeper.abort();
		self.handle.shutdown().await;
		let _ = self.actor.await;
	}
}

async fn sweep(
	handle: ControlHandle,
	registry: Arc<ClientRegistry>,
	book: Arc<ClientBook>,
	ttl: Duration,
) {
	let period = (ttl / 4).max(Duration::from_millis(10));
	let mut ticker = tokio::time::interval(period);
	loop {
		ticker.tick().await;
		for client in book.idle(ttl) {
			release_client(&handle, &registry, &book, client).await;
		}
	}
}

async fn release_client(
	handle: &ControlHandle,
	registry: &ClientRegistry,
	book: &ClientBook,
	client: ClientId,
) {
	tracing::info!(%client, "detaching idle client");
	book.remove(client);
	registry.remove(&client.to_bytes());
	let _ = handle.disconnected(client).await;
}
