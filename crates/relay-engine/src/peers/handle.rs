//! The public face of the peer manager: configuration, commands, outputs.

use std::net::SocketAddr;
use std::sync::Arc;

use arc_swap::ArcSwap;
use relay_core::{PeerEvent, PeerId, PeerMessage};
use relay_net::AddressError;
use relay_net::AddressPolicy;
use relay_net::discovery::InterfacePolicy;
use relay_net::identity::{Fingerprint, PeerTrust, TrustState};
use relay_net::session::SecureSession;
use thiserror::Error;
use tokio::sync::{mpsc, oneshot};

use super::backoff::BackoffConfig;
use super::link::{LinkError, LinkTiming};
use super::signaling::MediaSignal;

/// Peer manager failures.
#[derive(Debug, Error)]
pub enum PeerError {
	#[error("peer manager stopped")]
	Stopped,
	#[error("command queue is full")]
	Busy,
	#[error("no authenticated key is known for that peer in this process")]
	UnknownKey,
	#[error("trust store refused the decision: {0}")]
	Trust(String),
	#[error("address refused: {0}")]
	Address(#[from] AddressError),
	#[error("link failed: {0}")]
	Link(#[from] LinkError),
	#[error("could not bind the listener: {0}")]
	Bind(std::io::Error),
	#[error("discovery: {0}")]
	Discovery(String),
}

/// How the engine takes part in LAN discovery.
#[derive(Debug, Clone, Default)]
pub struct DiscoverySetup {
	/// Which interfaces may be used.
	pub interfaces: InterfacePolicy,
	/// Publish a local advertisement as well as browsing.
	pub advertise: bool,
}

/// Peer manager configuration.
#[derive(Debug, Clone)]
pub struct PeerConfig {
	/// TCP listen address for inbound control links.
	pub listen: SocketAddr,
	/// Advertised display name.
	pub display_name: String,
	/// Which addresses may be dialled or accepted.
	pub policy: AddressPolicy,
	/// Link timers.
	pub timing: LinkTiming,
	/// Reconnect backoff.
	pub backoff: BackoffConfig,
	/// mDNS setup; `None` disables discovery (manual addresses only).
	pub discovery: Option<DiscoverySetup>,
}

impl PeerConfig {
	/// LAN-only defaults on an ephemeral port, discovery on.
	pub fn new(display_name: &str) -> Self {
		Self {
			listen: SocketAddr::from(([0, 0, 0, 0], 0)),
			display_name: display_name.to_owned(),
			policy: AddressPolicy::lan_only(),
			timing: LinkTiming::default(),
			backoff: BackoffConfig::default(),
			discovery: Some(DiscoverySetup::default()),
		}
	}
}

/// Everything the peer manager reports upward.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PeerOutput {
	/// An event for the domain (link state, call signaling).
	Core(PeerEvent),
	/// Media negotiation for the media module.
	Media {
		/// The sender.
		peer: PeerId,
		/// The signal.
		signal: MediaSignal,
	},
	/// An authenticated but untrusted peer. The link is held without
	/// signaling until the user decides.
	TrustRequired {
		/// The peer.
		peer: PeerId,
		/// Untrusted display name.
		name: String,
		/// The classification (always `New` today).
		trust: PeerTrust,
	},
	/// A key other than the pinned one presented a trusted name. The link
	/// was refused and nothing was replaced.
	KeyChanged {
		/// The offending key digest.
		peer: PeerId,
		/// Untrusted display name.
		name: String,
		/// The previously pinned key digest.
		previous: PeerId,
	},
}

/// A point-in-time view of a known peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerSnapshot {
	/// Key digest.
	pub id: PeerId,
	/// Display name (discovery or `Hello`; untrusted).
	pub name: String,
	/// Known addresses.
	pub addresses: Vec<SocketAddr>,
	/// Whether a trusted control link is up.
	pub linked: bool,
	/// Trust state if pinned.
	pub trust: Option<TrustState>,
}

pub(super) type Reply<T> = oneshot::Sender<Result<T, PeerError>>;

pub(super) enum Command {
	Ensure(Fingerprint),
	Release(Fingerprint),
	Dial {
		addr: SocketAddr,
		expected: Option<Fingerprint>,
		reply: Reply<Fingerprint>,
	},
	Send(Fingerprint, relay_net::proto::Envelope),
	Pin {
		peer: Fingerprint,
		state: TrustState,
		reply: Reply<()>,
	},
	Session {
		peer: Fingerprint,
		reply: oneshot::Sender<Option<Arc<SecureSession>>>,
	},
	AddAddress(Fingerprint, SocketAddr),
	Redial(Fingerprint),
	Discovered(Box<relay_net::discovery::DiscoveredPeer>),
	Removed(String),
	Shutdown,
}

/// Cloneable control handle. Sync methods never block; they report `Busy`
/// when the bounded command queue is full.
#[derive(Clone)]
pub struct PeerHandle {
	pub(super) commands: mpsc::Sender<Command>,
	pub(super) local_addr: SocketAddr,
	pub(super) local: Fingerprint,
	pub(super) snapshot: Arc<ArcSwap<Vec<PeerSnapshot>>>,
	pub(super) sessions: Arc<std::sync::Mutex<super::signaling::SessionTable>>,
}

impl PeerHandle {
	fn try_send(&self, command: Command) -> Result<(), PeerError> {
		self.commands
			.try_send(command)
			.map_err(|error| match error {
				mpsc::error::TrySendError::Full(_) => PeerError::Busy,
				mpsc::error::TrySendError::Closed(_) => PeerError::Stopped,
			})
	}

	async fn ask<T>(&self, make: impl FnOnce(Reply<T>) -> Command) -> Result<T, PeerError> {
		let (reply, answer) = oneshot::channel();
		self.commands
			.send(make(reply))
			.await
			.map_err(|_| PeerError::Stopped)?;
		answer.await.map_err(|_| PeerError::Stopped)?
	}

	/// The address the control listener is bound to.
	pub fn local_addr(&self) -> SocketAddr {
		self.local_addr
	}

	/// The local device fingerprint.
	pub fn fingerprint(&self) -> Fingerprint {
		self.local
	}

	/// Latest known peers (discovery plus links).
	pub fn peers(&self) -> Arc<Vec<PeerSnapshot>> {
		self.snapshot.load_full()
	}

	/// Wants a trusted link to `peer`; reconnects with backoff until released.
	pub fn ensure_link(&self, peer: PeerId) -> Result<(), PeerError> {
		self.try_send(Command::Ensure(fingerprint(peer)))
	}

	/// Stops reconnecting to `peer`. An existing link stays until it drops.
	pub fn release(&self, peer: PeerId) -> Result<(), PeerError> {
		self.try_send(Command::Release(fingerprint(peer)))
	}

	/// Sends a call message; queued (bounded) until the link is up.
	pub fn send(&self, peer: PeerId, message: PeerMessage) -> Result<(), PeerError> {
		let envelope = {
			let mut table = self.sessions.lock().unwrap_or_else(|p| p.into_inner());
			super::signaling::encode(&message, &mut table)
		};
		self.try_send(Command::Send(fingerprint(peer), envelope))
	}

	/// Sends media negotiation traffic.
	pub fn send_media(&self, peer: PeerId, signal: &MediaSignal) -> Result<(), PeerError> {
		self.try_send(Command::Send(
			fingerprint(peer),
			super::signaling::encode_media(signal),
		))
	}

	/// Remembers a manual address for a peer and dials it if wanted.
	pub fn add_address(&self, peer: PeerId, addr: SocketAddr) -> Result<(), PeerError> {
		self.try_send(Command::AddAddress(fingerprint(peer), addr))
	}

	/// Dials `addr`, optionally requiring a specific peer key. Returns the
	/// authenticated peer once the link is up (trusted or not).
	pub async fn dial(
		&self,
		addr: SocketAddr,
		expected: Option<PeerId>,
	) -> Result<PeerId, PeerError> {
		let expected = expected.map(fingerprint);
		let found = self
			.ask(|reply| Command::Dial {
				addr,
				expected,
				reply,
			})
			.await?;
		Ok(peer_id(&found))
	}

	/// Persists an explicit user trust decision and promotes a held link.
	pub async fn pin(&self, peer: PeerId, state: TrustState) -> Result<(), PeerError> {
		self.ask(|reply| Command::Pin {
			peer: fingerprint(peer),
			state,
			reply,
		})
		.await
	}

	/// The secure session of the current trusted link, for media binding.
	pub async fn session(&self, peer: PeerId) -> Option<Arc<SecureSession>> {
		let (reply, answer) = oneshot::channel();
		self.commands
			.send(Command::Session {
				peer: fingerprint(peer),
				reply,
			})
			.await
			.ok()?;
		answer.await.ok()?
	}

	/// Stops the manager and closes every link.
	pub fn shutdown(&self) {
		let _ = self.commands.try_send(Command::Shutdown);
	}
}

/// Converts a domain peer id to a transport fingerprint.
pub fn fingerprint(peer: PeerId) -> Fingerprint {
	Fingerprint::from_bytes(*peer.as_bytes())
}

/// Converts a transport fingerprint to a domain peer id.
pub fn peer_id(fingerprint: &Fingerprint) -> PeerId {
	PeerId::from_bytes(*fingerprint.as_bytes())
}
