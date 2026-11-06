//! The public face of the peer manager: configuration, commands, outputs.

use relay_io::LockExt;
use std::net::SocketAddr;
use std::sync::Arc;

use arc_swap::ArcSwap;
use relay_core::{PeerEvent, PeerId, PeerMessage};
use relay_net::AddressError;
use relay_net::AddressPolicy;
use relay_net::channel::SessionBinding;
use relay_net::discovery::InterfacePolicy;
use relay_net::identity::{Fingerprint, PeerTrust, TrustState};
use relay_net::proto::ChannelMessage;
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

/// Where the local advertisement is published. The real implementation is
/// mDNS; tests plug in a recorder to see exactly when the device is visible.
pub trait AdvertSink: Send + Sync {
	/// Replace what is published; `None` withdraws it.
	fn publish(&self, advert: Option<&relay_net::discovery::Advertisement>);
}

/// A shareable [`AdvertSink`].
#[derive(Clone)]
pub struct SinkHandle(pub Arc<dyn AdvertSink>);

impl std::fmt::Debug for SinkHandle {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("SinkHandle")
	}
}

/// How the engine takes part in LAN discovery.
#[derive(Debug, Clone, Default)]
pub struct DiscoverySetup {
	/// Which interfaces may be used.
	pub interfaces: InterfacePolicy,
	/// Publish a local advertisement as well as browsing.
	pub advertise: bool,
	/// Publish through this sink instead of mDNS (no browsing happens then).
	pub sink: Option<SinkHandle>,
	/// Peers pushed by the shell's platform browser, merged with mDNS results.
	pub hints: Option<super::HintSource>,
}

/// What the local advertisement says beyond the display name: set by the
/// channel driver, published by discovery.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdvertSpec {
	/// The mDNS instance id (rotating and pseudonymous in hidden mode).
	pub instance_id: [u8; 16],
	/// Publish neither name nor fingerprint hint.
	pub hidden: bool,
	/// Extra TXT entries (blinded channel tags, listed channel names).
	pub extra_txt: Vec<(String, String)>,
	/// A client is attached. Without one nothing is advertised at all: a
	/// daemon nobody watches stays off the LAN.
	pub present: bool,
}

impl Default for AdvertSpec {
	fn default() -> Self {
		Self {
			instance_id: relay_io::entropy::random_bytes(),
			hidden: false,
			extra_txt: Vec::new(),
			present: false,
		}
	}
}

/// Peer manager configuration.
#[derive(Debug, Clone)]
pub struct PeerConfig {
	/// TCP listen address for inbound control links.
	pub listen: SocketAddr,
	/// Advertised display name.
	pub display_name: String,
	/// Which addresses may be discovered, offered by a peer, dialled or accepted.
	pub policy: AddressPolicy,
	/// Whether addresses the user types or pastes may be in overlay space
	/// (100.64.0.0/10). Discovery and peer-offered candidates never are.
	pub allow_overlay_manual: bool,
	/// Link timers.
	pub timing: LinkTiming,
	/// Reconnect backoff.
	pub backoff: BackoffConfig,
	/// mDNS setup; `None` disables discovery (manual addresses only).
	pub discovery: Option<DiscoverySetup>,
	/// Local avatar cache used to serve and store avatars; `None` disables
	/// avatar exchange (names are still exchanged).
	pub avatars: Option<Arc<relay_config::avatar::AvatarCache>>,
	/// Initial local avatar hash (must be in the cache).
	pub avatar: Option<[u8; 32]>,
	/// Source of the advertisement's channel and privacy parts; `None` keeps a
	/// plain open advertisement.
	pub advert: Option<tokio::sync::watch::Receiver<AdvertSpec>>,
}

impl PeerConfig {
	/// The policy for user-typed addresses and the links that follow from them.
	pub fn manual_policy(&self) -> AddressPolicy {
		self.policy.with_overlay_manual(self.allow_overlay_manual)
	}

	/// LAN-only defaults on an ephemeral port, discovery on.
	pub fn new(display_name: &str) -> Self {
		Self {
			listen: SocketAddr::from(([0, 0, 0, 0], 0)),
			display_name: display_name.to_owned(),
			policy: AddressPolicy::lan_only(),
			allow_overlay_manual: false,
			timing: LinkTiming::default(),
			backoff: BackoffConfig::default(),
			discovery: Some(DiscoverySetup::default()),
			avatars: None,
			avatar: None,
			advert: None,
		}
	}
}

/// Channel traffic and discovery facts. These never reach call state: the host
/// hands them to the channel driver only.
#[derive(Debug, Clone, PartialEq)]
pub enum ChannelEvent {
	/// A trusted link is the live one for `peer` (trust was evaluated first).
	LinkUp {
		/// The peer.
		peer: PeerId,
		/// The session transcript channel proofs bind to.
		binding: SessionBinding,
	},
	/// The trusted link to `peer` ended.
	LinkDown {
		/// The peer.
		peer: PeerId,
	},
	/// A channel message from the peer's live trusted link.
	Message {
		/// The sender.
		peer: PeerId,
		/// The session the message arrived on.
		binding: SessionBinding,
		/// The message (structurally validated by the control layer).
		message: Box<ChannelMessage>,
	},
	/// A discovery record (untrusted hints only).
	Advert(Box<relay_net::discovery::DiscoveredPeer>),
}

/// Session messaging traffic and link facts. These never reach call state: the host hands
/// them to the messaging actor only, and only from the live trusted link of `peer`.
#[derive(Clone, PartialEq)]
pub enum SessionEvent {
	/// A trusted link is the live one for `peer`.
	LinkUp {
		/// The peer.
		peer: PeerId,
		/// The control connection's remote address: where a data link is dialled.
		remote_ip: std::net::IpAddr,
	},
	/// The trusted link to `peer` ended.
	LinkDown {
		/// The peer.
		peer: PeerId,
	},
	/// A session message from the peer's live trusted link (structurally validated).
	Message {
		/// The authenticated sender.
		peer: PeerId,
		/// The message.
		message: Box<relay_net::proto::SessionMessage>,
	},
}

/// Message content must never reach a log line, so `Debug` names the kind only.
impl std::fmt::Debug for SessionEvent {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		match self {
			Self::LinkUp { peer, .. } => write!(f, "SessionEvent::LinkUp({peer})"),
			Self::LinkDown { peer } => write!(f, "SessionEvent::LinkDown({peer})"),
			Self::Message { peer, .. } => write!(f, "SessionEvent::Message({peer}, <redacted>)"),
		}
	}
}

/// Everything the peer manager reports upward.
#[derive(Debug, Clone, PartialEq)]
pub enum PeerOutput {
	/// An event for the domain (link state, call signaling).
	Core(PeerEvent),
	/// Channel protocol traffic; routed to the channel driver, never to the domain.
	Channel(ChannelEvent),
	/// Session messaging traffic; routed to the messaging actor, never to the domain.
	Session(SessionEvent),
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
	/// A trusted peer's display profile (untrusted text; display only).
	Profile {
		/// The peer.
		peer: PeerId,
		/// Sanitized display name.
		name: String,
		/// Announced avatar hash, if a plausible one was announced.
		avatar: Option<[u8; 32]>,
	},
	/// A peer's avatar is in the local cache (fetched, validated, hashed).
	AvatarReady {
		/// The peer.
		peer: PeerId,
		/// Content hash of the cached avatar.
		hash: [u8; 32],
	},
	/// A peer's avatar was refused (bad chunk order, oversize, wrong hash,
	/// not a valid canonical image).
	AvatarRejected {
		/// The peer.
		peer: PeerId,
		/// The hash the peer announced.
		hash: [u8; 32],
		/// Why.
		reason: String,
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
	/// Key digest; `None` for a discovered device whose key is not yet known
	/// (discovery only carries a short hint).
	pub id: Option<PeerId>,
	/// Short discovery hint, when seen through discovery.
	pub hint: Option<relay_net::discovery::FingerprintHint>,
	/// Announced avatar hash, once a profile arrived.
	pub avatar: Option<[u8; 32]>,
	/// Display name (discovery or `Hello`; untrusted).
	pub name: String,
	/// Known addresses.
	pub addresses: Vec<SocketAddr>,
	/// Whether a trusted control link is up.
	pub linked: bool,
	/// Trust state if pinned.
	pub trust: Option<TrustState>,
	/// Live round-trip timer of the link (see `rtt.rs`); read it with [`RttCell::get`].
	pub probe: Option<Arc<super::rtt::RttCell>>,
}

/// What media needs to know about a trusted link.
#[derive(Clone)]
pub struct LinkInfo {
	/// The secure session shared by control and media.
	pub session: Arc<SecureSession>,
	/// The local address of the control connection: the interface address
	/// a peer can reach this engine on.
	pub local_ip: std::net::IpAddr,
	/// The link's authenticated remote IP when it is an opted-in overlay
	/// address; media may use exactly this overlay address.
	pub overlay_peer: Option<std::net::IpAddr>,
}

pub(super) type Reply<T> = oneshot::Sender<Result<T, PeerError>>;

pub(super) enum Command {
	Ensure(Fingerprint),
	Release(Fingerprint),
	/// The stored trust decision was removed; drop the link.
	Forgotten(Fingerprint),
	/// The pinned key `old` was replaced by the verified key `new`: dial the new one.
	Replaced {
		old: Fingerprint,
		new: Fingerprint,
		via: Vec<SocketAddr>,
	},
	Dial {
		addr: SocketAddr,
		expected: Option<Fingerprint>,
		reply: Reply<Fingerprint>,
	},
	Send(Fingerprint, relay_net::proto::Envelope),
	Channel(Fingerprint, Box<ChannelMessage>),
	Session(Fingerprint, Box<relay_net::proto::SessionMessage>),
	Pin {
		peer: Fingerprint,
		state: TrustState,
		reply: Reply<()>,
	},
	Link {
		peer: Fingerprint,
		reply: oneshot::Sender<Option<LinkInfo>>,
	},
	AddAddress(Fingerprint, SocketAddr),
	Redial(Fingerprint),
	SetProfile(super::profile::LocalProfile),
	AvatarValidated {
		peer: Fingerprint,
		expected: [u8; 32],
		stored: Result<[u8; 32], String>,
	},
	AvatarServed(Fingerprint),
	Discovered(Box<relay_net::discovery::DiscoveredPeer>),
	Removed(String),
	/// Whether this device is present and visible changed: probe, or stop probing.
	Visibility,
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
	pub(super) sessions: Arc<std::sync::Mutex<super::sessions::SessionTable>>,
	pub(super) trust: super::trust::SharedTrust,
	pub(super) key_changes: super::key_changes::KeyChanges,
}

impl PeerHandle {
	pub(super) fn try_send(&self, command: Command) -> Result<(), PeerError> {
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
			let mut table = self.sessions.lock_unpoisoned();
			super::signaling::encode(&message, peer, &mut table)
		};
		self.try_send(Command::Send(fingerprint(peer), envelope))
	}

	/// Sends a channel message over the live trusted link; dropped if there is none
	/// (channel exchanges are bound to one session and are never queued).
	pub fn send_channel(&self, peer: PeerId, message: ChannelMessage) -> Result<(), PeerError> {
		self.try_send(Command::Channel(fingerprint(peer), Box::new(message)))
	}

	/// Sends a session message over the live trusted link; dropped if there is none or the
	/// control queue is full (the messaging layer retries unacked messages itself).
	pub fn send_session(
		&self,
		peer: PeerId,
		message: relay_net::proto::SessionMessage,
	) -> Result<(), PeerError> {
		self.try_send(Command::Session(fingerprint(peer), Box::new(message)))
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

	/// Fingerprints of every device the user trusts, linked or not.
	pub fn trusted(&self) -> Vec<Fingerprint> {
		self.trust.lock().fingerprints()
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

	/// The current trusted link, for media binding.
	pub async fn link(&self, peer: PeerId) -> Option<LinkInfo> {
		let (reply, answer) = oneshot::channel();
		self.commands
			.send(Command::Link {
				peer: fingerprint(peer),
				reply,
			})
			.await
			.ok()?;
		answer.await.ok()?
	}

	/// The peer a call was exchanged with, once an invitation was sent or received.
	pub fn peer_of(&self, call: relay_core::CallId) -> Option<PeerId> {
		self.sessions.lock_unpoisoned().peer_of(call)
	}

	/// Changes the local display name and avatar: re-advertised on mDNS and
	/// sent to every trusted link.
	pub fn set_profile(&self, profile: super::profile::LocalProfile) -> Result<(), PeerError> {
		self.try_send(Command::SetProfile(profile))
	}

	/// Feeds a discovery record from another source (tests, manual entry).
	pub fn note_discovered(
		&self,
		peer: relay_net::discovery::DiscoveredPeer,
	) -> Result<(), PeerError> {
		self.try_send(Command::Discovered(Box::new(peer)))
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
