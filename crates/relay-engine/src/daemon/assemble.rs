//! Building the real adapters: peer manager, media system and their glue.

use std::sync::Arc;
use std::time::Duration;

use relay_net::discovery::sanitize_display_name;
use relay_net::transfer::{HubConfig, TransferHub};
use tokio::runtime::Handle;
use tokio::sync::watch;
use tokio::task::JoinHandle;

use super::error::DaemonError;
use super::options::{AudioSetup, NetworkOptions};
use super::{Environment, glue};
use crate::channels::{ChannelHub, Clock, HubParts};
use crate::media::{MediaConfig, MediaSystem, Selection};
use crate::messaging::{
	self, HubParts as MessagingParts, MessagingConfig, MessagingHandle, MessagingLimits,
	MessagingOptions,
};
use crate::peers::{AdvertSpec, PeerAdapter, PeerConfig, PeerHandle, spawn};
use crate::service::Ports;
use crate::service::ports::{AdvertMode, DeviceControl, SettingsStore};

/// How long to wait for the first device listing before giving up on
/// restoring the saved selection.
const LISTING_WAIT: Duration = Duration::from_secs(2);

/// What keeps the real adapters alive.
pub(super) struct Adapters {
	peers: PeerHandle,
	channels: ChannelHub,
	messaging: MessagingHandle,
	tasks: Vec<JoinHandle<()>>,
}

impl Adapters {
	pub(super) fn listen_addr(&self) -> std::net::SocketAddr {
		self.peers.local_addr()
	}

	pub(super) fn channels(&self) -> ChannelHub {
		self.channels.clone()
	}

	pub(super) fn messaging(&self) -> MessagingHandle {
		self.messaging.clone()
	}

	pub(super) async fn shutdown(self) {
		self.messaging.stop().await;
		self.peers.shutdown();
		for task in self.tasks {
			task.abort();
		}
	}
}

pub(super) async fn assemble(
	env: &Environment,
	network: NetworkOptions,
	audio: AudioSetup,
	mic_test_max: std::time::Duration,
	clock: Clock,
	messaging: MessagingOptions,
) -> Result<(Ports, Adapters), DaemonError> {
	let stored = env.settings.load();
	let mut config = PeerConfig::new(&sanitize_display_name(&stored.display_name));
	config.listen = network
		.listen
		.unwrap_or_else(|| ([0, 0, 0, 0], env.preferences.network.listen_port).into());
	config.policy = network.policy;
	// Only addresses the user types may enter overlay space.
	config.allow_overlay_manual = env.preferences.network.allow_overlay_manual;
	config.discovery = network.discovery;
	config.blocks = env.blocks.clone();
	config.avatars = Some(Arc::clone(&env.avatars));
	config.avatar = stored.avatar;
	let manual_policy = config.manual_policy();
	let advert = Arc::new(
		watch::channel(AdvertSpec {
			hidden: stored.advert_mode == AdvertMode::Hidden,
			..AdvertSpec::default()
		})
		.0,
	);
	config.advert = Some(advert.subscribe());
	let (peers, output) = spawn(
		config,
		Arc::clone(&env.transport_identity),
		env.trust.clone(),
		env.incarnation,
	)
	.await?;

	let channels = ChannelHub::new(HubParts {
		store: Arc::clone(&env.channels),
		local: peers.fingerprint(),
		peers: peers.clone(),
		control: env.handle.clone(),
		settings: env.settings.clone(),
		clock,
		advert: Arc::clone(&advert),
	})
	.map_err(|e| DaemonError::Channels(e.to_string()))?;
	let messaging = start_messaging(env, &peers, network.policy, messaging).await;
	let runtime = Handle::current();
	let (sink, events) = glue::media_events(env.handle.clone());
	let mut media_config = MediaConfig::new(Selection::default());
	media_config.bind_ip = network.media_bind;
	media_config.policy = network.policy;
	media_config.gates = Some(env.gates.clone());
	media_config.mic_test_max = mic_test_max;
	let media = MediaSystem::start(
		media_config,
		audio.factory(),
		None,
		peers.clone(),
		sink,
		runtime.clone(),
	)
	.map_err(DaemonError::Media)?;

	let mut adapter =
		PeerAdapter::new(peers.clone(), runtime, network.policy, manual_policy).with_advert(advert);
	if let Some(interfaces) = network.interfaces {
		adapter = adapter.with_interfaces(interfaces);
	}
	let adapter = Arc::new(adapter);
	let tasks = vec![
		events,
		glue::peer_output(
			output,
			env.handle.clone(),
			media.clone(),
			adapter.clone(),
			channels.clone(),
			messaging.clone(),
		),
		channels.spawn_maintenance(),
		glue::device_watch(media.clone(), env.handle.clone()),
	];
	restore_devices(&media, &stored.input_device, &stored.output_device, env).await;
	let ports = Ports {
		media: Arc::new(media.clone()),
		peers: adapter,
		devices: Arc::new(media),
		settings: env.settings.clone(),
	};
	Ok((
		ports,
		Adapters {
			peers,
			channels,
			messaging,
			tasks,
		},
	))
}

/// Binds the data-link listener and starts the messaging actor. A listener that cannot be
/// bound only turns file transfers off; text and images keep working.
async fn start_messaging(
	env: &Environment,
	peers: &PeerHandle,
	policy: relay_net::AddressPolicy,
	options: MessagingOptions,
) -> MessagingHandle {
	let config = MessagingConfig::from_prefs(&env.preferences);
	let listen = peers.local_addr();
	let bind = std::net::SocketAddr::new(listen.ip(), env.preferences.network.transfer_port);
	let hub_config = HubConfig {
		rate_per_link: config.rate_bytes_per_sec(),
		address_policy: policy,
		..HubConfig::default()
	};
	let transfers = match TransferHub::bind(bind, Arc::clone(&env.transport_identity), hub_config)
		.await
	{
		Ok(hub) => Some(Arc::new(hub)),
		Err(error) => {
			tracing::warn!(%error, "file transfers are unavailable: the data-link listener could not be bound");
			None
		}
	};
	messaging::spawn(MessagingParts {
		peers: peers.clone(),
		transfers,
		local: peers.fingerprint(),
		config: Arc::new(arc_swap::ArcSwap::from_pointee(config)),
		store: env.settings.clone(),
		sinks: options.sinks,
		limits: options
			.limits
			.unwrap_or_else(|| MessagingLimits::from_prefs(&env.preferences.limits)),
	})
}

/// Re-applies the saved device choice once the device thread has listed
/// hardware. A missing pinned device is reported and the system default is
/// used meanwhile; no other device is ever picked.
async fn restore_devices(media: &MediaSystem, input: &str, output: &str, env: &Environment) {
	if input.is_empty() && output.is_empty() {
		return;
	}
	let waited = tokio::time::timeout(LISTING_WAIT, async {
		while media.device_listing().is_empty() {
			tokio::time::sleep(Duration::from_millis(20)).await;
		}
	})
	.await;
	let result = match waited {
		Ok(()) => media.select(input, output),
		Err(_) => return,
	};
	if let Err(error) = result {
		let _ = env
			.handle
			.attention(None, saved_device_missing(&error))
			.await;
	}
}

fn saved_device_missing(error: &crate::service::PortError) -> relay_api::pb::Problem {
	relay_api::pb::Problem {
		code: "SAVED_DEVICE_UNAVAILABLE".to_owned(),
		message: format!(
			"the saved audio device could not be restored, using the system default: {error}"
		),
		retryable: false,
		recovery_action: "select_devices".to_owned(),
	}
}
