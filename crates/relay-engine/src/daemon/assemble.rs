//! Building the real adapters: peer manager, media system and their glue.

use std::sync::Arc;
use std::time::Duration;

use relay_audio::device::DeviceSelection;
use relay_net::discovery::sanitize_display_name;
use tokio::runtime::Handle;
use tokio::task::JoinHandle;

use super::error::DaemonError;
use super::options::{AudioSetup, NetworkOptions};
use super::{Environment, glue};
use crate::media::{MediaConfig, MediaSystem, Selection};
use crate::peers::{PeerAdapter, PeerConfig, PeerHandle, spawn};
use crate::service::Ports;
use crate::service::ports::{DeviceControl, SettingsStore};

/// How long to wait for the first device listing before giving up on
/// restoring the saved selection.
const LISTING_WAIT: Duration = Duration::from_secs(2);

/// What keeps the real adapters alive.
pub(super) struct Adapters {
	peers: PeerHandle,
	tasks: Vec<JoinHandle<()>>,
}

impl Adapters {
	pub(super) fn listen_addr(&self) -> std::net::SocketAddr {
		self.peers.local_addr()
	}

	pub(super) fn shutdown(self) {
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
	config.avatars = Some(Arc::clone(&env.avatars));
	config.avatar = stored.avatar;
	let manual_policy = config.manual_policy();
	let (peers, output) = spawn(
		config,
		Arc::clone(&env.transport_identity),
		env.trust.clone(),
		env.incarnation,
	)
	.await?;

	let runtime = Handle::current();
	let (sink, events) = glue::media_events(env.handle.clone());
	let mut media_config = MediaConfig::new(Selection {
		input: None,
		output: DeviceSelection::FollowDefault,
	});
	media_config.bind_ip = network.media_bind;
	media_config.policy = network.policy;
	media_config.gates = Some(env.gates.clone());
	let media = MediaSystem::start(
		media_config,
		audio.factory(),
		None,
		peers.clone(),
		sink,
		runtime.clone(),
	)
	.map_err(DaemonError::Media)?;

	let adapter = Arc::new(PeerAdapter::new(peers.clone(), runtime, manual_policy));
	let tasks = vec![
		events,
		glue::peer_output(output, env.handle.clone(), media.clone(), adapter.clone()),
		glue::device_watch(media.clone(), env.handle.clone()),
	];
	restore_devices(&media, &stored.input_device, &stored.output_device, env).await;
	let ports = Ports {
		media: Arc::new(media.clone()),
		peers: adapter,
		devices: Arc::new(media),
		settings: env.settings.clone(),
	};
	Ok((ports, Adapters { peers, tasks }))
}

/// Re-applies the saved device choice once the device thread has listed
/// hardware. A missing device is reported, never replaced by another.
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
		message: format!("the saved audio device could not be restored: {error}"),
		retryable: false,
		recovery_action: "select_devices".to_owned(),
	}
}
