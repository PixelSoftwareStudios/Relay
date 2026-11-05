//! The listener, the dialer, and the caps that keep file transfers from starving a call.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use relay_messages::limits::BULK_RATE_BYTES_PER_SEC;
use tokio::net::{TcpListener, TcpSocket, TcpStream};
use tokio::sync::{Semaphore, oneshot};
use tokio::task::JoinHandle;
use tokio::time::{Instant, timeout, timeout_at};

use super::DataError;
use super::handshake::{self, Ticket};
use super::limits::{
	DATA_CONNECT_TIMEOUT, DATA_IDLE_TIMEOUT, DATA_LINKS_MAX, DATA_LINKS_PER_SOURCE, DSCP_CS1_TOS,
	TOKEN_TTL_DEFAULT,
};
use super::link::{DataLink, LinkGuard, LinkParams, SourceCounts, TransferRole};
use super::pacer::{Pacer, SharedLimit};
use super::token::{Expectation, Registry, TransferToken};
use crate::address::AddressPolicy;
use crate::handshake::gate::{HandshakeGate, source_key};
use crate::handshake::{AcceptBackoff, HandshakeError};
use crate::identity::{Fingerprint, Identity};

/// Network priority of file bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
	/// Default traffic class.
	Normal,
	/// DSCP CS1 ("lower effort"): the TOS/traffic-class byte is set on the data sockets where
	/// the OS allows it (Linux, macOS, BSD; Windows ignores the option without QoS policy).
	/// Honouring it is up to the routers and Wi-Fi access point (WMM maps CS1 to background).
	/// The marking is best effort; the pacer is what actually protects the call.
	Background,
}

#[derive(Debug, Clone)]
pub struct HubConfig {
	/// Established data links at once, both directions together.
	pub max_links: usize,
	/// Established links from one source address.
	pub per_source: usize,
	/// Default cap per link in bytes per second; `None` disables it.
	pub rate_per_link: Option<u64>,
	pub priority: Priority,
	pub address_policy: AddressPolicy,
	pub idle_timeout: Duration,
}

impl Default for HubConfig {
	fn default() -> Self {
		Self {
			max_links: DATA_LINKS_MAX,
			per_source: DATA_LINKS_PER_SOURCE,
			rate_per_link: Some(BULK_RATE_BYTES_PER_SEC),
			priority: Priority::Background,
			address_policy: AddressPolicy::lan_only(),
			idle_timeout: DATA_IDLE_TIMEOUT,
		}
	}
}

struct Inner {
	identity: Arc<Identity>,
	config: HubConfig,
	registry: Registry,
	gate: HandshakeGate,
	links: Arc<Semaphore>,
	sources: SourceCounts,
	global: SharedLimit,
}

impl Inner {
	fn guard(&self, source: Option<std::net::IpAddr>) -> Result<LinkGuard, DataError> {
		let permit = Arc::clone(&self.links)
			.try_acquire_owned()
			.map_err(|_| DataError::TooManyPending)?;
		if let Some(ip) = source {
			let mut map = self.sources.lock().unwrap_or_else(|p| p.into_inner());
			let held = map.entry(source_key(ip)).or_default();
			if *held >= self.config.per_source {
				return Err(DataError::TooManyPending);
			}
			*held += 1;
		}
		Ok(LinkGuard::new(
			permit,
			source.map(|ip| (ip, Arc::clone(&self.sources))),
		))
	}

	fn pacer(&self) -> Pacer {
		Pacer::new(self.config.rate_per_link, Some(self.global.clone()))
	}

	fn params(
		&self,
		role: TransferRole,
		size: u64,
		offer: [u8; 16],
		session: [u8; 16],
	) -> LinkParams {
		let mut p = LinkParams::new(role, size, offer, session);
		p.idle_timeout = self.config.idle_timeout;
		p
	}

	fn mark(&self, socket: &TcpSocket, v6: bool) {
		if self.config.priority == Priority::Background {
			let r = if v6 {
				socket.set_tclass_v6(DSCP_CS1_TOS)
			} else {
				socket.set_tos_v4(DSCP_CS1_TOS)
			};
			if let Err(error) = r {
				tracing::debug!(%error, "could not mark data socket as background");
			}
		}
	}

	/// Handles one inbound connection. Every failure is a silent close.
	async fn serve(self: Arc<Self>, mut stream: TcpStream, peer: SocketAddr) {
		if self.config.address_policy.check_ip(peer.ip()).is_err() {
			return;
		}
		let Ok(permit) = self.gate.try_enter(peer.ip()) else {
			return;
		};
		let admit = |fp: &Fingerprint, ticket: &Ticket| {
			// Caps first, so a refused link does not burn the token.
			let guard = self.guard(Some(peer.ip()))?;
			let claimed =
				self.registry
					.claim(fp, &ticket.session_id, &ticket.offer_id, &ticket.token)?;
			Ok((claimed, guard))
		};
		let result = tokio::select! {
			r = handshake::respond(&mut stream, &self.identity, admit) => r,
			() = permit.evicted() => Err(DataError::Handshake(HandshakeError::Overloaded)),
		};
		drop(permit);
		let Ok((session, (claimed, guard))) = result else {
			return;
		};
		let e = claimed.expectation;
		let link = DataLink::with_guard(
			stream,
			Arc::new(session),
			self.params(e.role, e.size, e.offer_id, e.session_id),
			self.pacer(),
			Some(guard),
		);
		let _ = claimed.deliver.send(link);
	}
}

/// Listener plus dialer for data links. Dropping it stops accepting.
pub struct TransferHub {
	inner: Arc<Inner>,
	addr: SocketAddr,
	accept: JoinHandle<()>,
}

impl TransferHub {
	/// Binds the listener (use port 0 for an ephemeral port) and starts accepting.
	pub async fn bind(
		addr: SocketAddr,
		identity: Arc<Identity>,
		config: HubConfig,
	) -> Result<Self, DataError> {
		let socket = if addr.is_ipv4() {
			TcpSocket::new_v4()?
		} else {
			TcpSocket::new_v6()?
		};
		let inner = Arc::new(Inner {
			identity,
			links: Arc::new(Semaphore::new(config.max_links)),
			config,
			registry: Registry::default(),
			gate: HandshakeGate::new(),
			sources: Arc::default(),
			global: SharedLimit::unlimited(),
		});
		inner.mark(&socket, addr.is_ipv6());
		socket.bind(addr)?;
		let listener: TcpListener = socket.listen(32)?;
		let addr = listener.local_addr()?;
		let accept = tokio::spawn(accept_loop(listener, Arc::clone(&inner)));
		Ok(Self {
			inner,
			addr,
			accept,
		})
	}

	pub fn local_addr(&self) -> SocketAddr {
		self.addr
	}

	/// Caps the sum of all links' file bytes per second (`None` clears the cap).
	pub fn set_global_rate(&self, rate: Option<u64>) {
		self.inner.global.set_rate(rate);
	}

	/// Data links currently open (either direction).
	pub fn open_links(&self) -> usize {
		self.inner.config.max_links - self.inner.links.available_permits()
	}

	/// Tokens issued and not yet redeemed, expired or dropped.
	pub fn pending_tokens(&self) -> usize {
		self.inner.registry.len()
	}

	/// Mints a token for exactly one inbound data link described by `expectation`. Send the
	/// token and [`Self::local_addr`]'s port to the peer over the control link, then `wait` on
	/// the returned handle. Dropping the handle revokes the token.
	pub fn expect(
		&self,
		expectation: Expectation,
		ttl: Option<Duration>,
	) -> Result<(TransferToken, PendingLink), DataError> {
		let offer_id = expectation.offer_id;
		let (token, expires, receive) = self
			.inner
			.registry
			.issue(expectation, ttl.unwrap_or(TOKEN_TTL_DEFAULT))?;
		Ok((
			token,
			PendingLink {
				receive,
				expires,
				offer_id,
				inner: Arc::clone(&self.inner),
			},
		))
	}

	/// Opens an outbound data link to `addr` (the control link's remote IP and the granted
	/// port). `expected` pins the peer key: a different key aborts before our key or the token
	/// is sent.
	pub async fn dial(
		&self,
		addr: SocketAddr,
		expected: &Fingerprint,
		ticket: &Ticket,
		role: TransferRole,
		size: u64,
	) -> Result<DataLink<TcpStream>, DataError> {
		self.inner.config.address_policy.check_socket(addr)?;
		let guard = self.inner.guard(None)?;
		let socket = if addr.is_ipv4() {
			TcpSocket::new_v4()?
		} else {
			TcpSocket::new_v6()?
		};
		self.inner.mark(&socket, addr.is_ipv6());
		let mut stream = timeout(DATA_CONNECT_TIMEOUT, socket.connect(addr))
			.await
			.map_err(|_| DataError::Handshake(HandshakeError::Timeout))??;
		let session =
			handshake::initiate(&mut stream, &self.inner.identity, expected, ticket).await?;
		Ok(DataLink::with_guard(
			stream,
			Arc::new(session),
			self.inner
				.params(role, size, ticket.offer_id, ticket.session_id),
			self.inner.pacer(),
			Some(guard),
		))
	}
}

impl Drop for TransferHub {
	fn drop(&mut self) {
		self.accept.abort();
	}
}

async fn accept_loop(listener: TcpListener, inner: Arc<Inner>) {
	let mut backoff = AcceptBackoff::new();
	loop {
		match listener.accept().await {
			Ok((stream, peer)) => {
				backoff.reset();
				tokio::spawn(Arc::clone(&inner).serve(stream, peer));
			}
			Err(error) => {
				tracing::debug!(%error, "data link accept failed");
				tokio::time::sleep(backoff.on_error()).await;
			}
		}
	}
}

/// An issued token waiting for its data link.
pub struct PendingLink {
	receive: oneshot::Receiver<DataLink<TcpStream>>,
	expires: Instant,
	offer_id: [u8; 16],
	inner: Arc<Inner>,
}

impl PendingLink {
	/// Resolves with the link, or `Expired` when the token ran out or was revoked.
	pub async fn wait(mut self) -> Result<DataLink<TcpStream>, DataError> {
		match timeout_at(self.expires, &mut self.receive).await {
			Ok(Ok(link)) => Ok(link),
			_ => Err(DataError::Expired),
		}
	}
}

impl Drop for PendingLink {
	fn drop(&mut self) {
		self.inner.registry.revoke(&self.offer_id);
	}
}
