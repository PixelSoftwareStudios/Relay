//! What a caller can configure when starting the engine.

use std::net::{IpAddr, SocketAddr};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use relay_api::ApiLimits;
use relay_audio::device::{AudioBackend, CpalBackend};
use relay_config::Paths;
use relay_config::identity::IdentityStore;
use relay_core::Limits;
use relay_net::AddressPolicy;

use super::Environment;
use crate::peers::DiscoverySetup;
use crate::service::{Entropy, Ports};

/// Builds the adapters themselves; replaces the real peers and media wiring.
/// For tests of the service layer that want fakes.
pub type AdapterFactory = Box<dyn FnOnce(&Environment) -> Ports + Send>;

/// Builds the audio backend on the device thread.
pub type BackendFactory = Box<dyn FnOnce() -> Box<dyn AudioBackend> + Send>;

/// How clients reach the engine.
#[derive(Debug, Clone)]
pub enum Transport {
	/// An owner-only Unix socket; `None` uses the per-user default path.
	Unix(Option<PathBuf>),
	/// Loopback TCP with a token profile file; `None` places the profile next
	/// to the default socket.
	LoopbackTcp(Option<PathBuf>),
}

/// Where the engine listens and who it talks to.
#[derive(Debug, Clone)]
pub struct NetworkOptions {
	/// Peer-link listen address; `None` uses `0.0.0.0` and the preferred port.
	pub listen: Option<SocketAddr>,
	/// Which addresses may be dialled or accepted.
	pub policy: AddressPolicy,
	/// Address the media sockets bind to.
	pub media_bind: IpAddr,
	/// LAN discovery; `None` limits the engine to manual addresses.
	pub discovery: Option<DiscoverySetup>,
}

impl Default for NetworkOptions {
	fn default() -> Self {
		Self {
			listen: None,
			policy: AddressPolicy::lan_only(),
			media_bind: IpAddr::from([0, 0, 0, 0]),
			discovery: Some(DiscoverySetup {
				advertise: true,
				..DiscoverySetup::default()
			}),
		}
	}
}

impl NetworkOptions {
	/// Loopback only, ephemeral port, no discovery: several engines on one
	/// machine (tests, demos).
	pub fn loopback() -> Self {
		Self {
			listen: Some(SocketAddr::from(([127, 0, 0, 1], 0))),
			policy: AddressPolicy::lan_and_loopback(),
			media_bind: IpAddr::from([127, 0, 0, 1]),
			discovery: None,
		}
	}
}

/// Where audio comes from and goes to.
pub enum AudioSetup {
	/// The operating system's devices through cpal.
	Cpal,
	/// A caller-supplied backend (fake hardware in tests).
	Backend(BackendFactory),
}

impl AudioSetup {
	pub(super) fn factory(self) -> BackendFactory {
		match self {
			Self::Cpal => Box::new(|| Box::new(CpalBackend::new())),
			Self::Backend(factory) => factory,
		}
	}
}

/// Everything the daemon needs to start.
pub struct RunOptions {
	/// Where configuration and data live.
	pub paths: Paths,
	/// How clients connect.
	pub transport: Transport,
	/// Identity storage override; `None` follows the preferences.
	pub identity_store: Option<Box<dyn IdentityStore>>,
	/// Peer-link and discovery settings.
	pub network: NetworkOptions,
	/// Audio hardware.
	pub audio: AudioSetup,
	/// Replaces the real peer and media adapters when set.
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
	/// Production defaults for the given directories.
	pub fn new(paths: Paths) -> Self {
		Self {
			paths,
			transport: Transport::Unix(None),
			identity_store: None,
			network: NetworkOptions::default(),
			audio: AudioSetup::Cpal,
			adapters: None,
			limits: Limits::default(),
			api: ApiLimits::DEFAULT,
			idle_client_ttl: Duration::from_secs(300),
			entropy: None,
		}
	}
}
