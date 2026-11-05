//! One secure control link: handshake, `Hello`, and the reader and writer
//! tasks that carry envelopes until the link dies.

use std::sync::Arc;
use std::time::Duration;

use relay_net::control::{ControlError, control_channel, new_envelope, new_hello};
use relay_net::handshake::{HandshakeError, initiate, respond};
use relay_net::identity::{Fingerprint, Identity, PublicKey};
use relay_net::proto::{Envelope, Keepalive, envelope::Body};
use relay_net::session::{LinkEpoch, Role, SecureSession};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio::sync::mpsc;
use tokio::task::AbortHandle;
use tokio::time::{MissedTickBehavior, interval, timeout};

/// Identifies one link inside the manager.
pub type LinkId = u64;

const OUTBOX_DEPTH: usize = 64;
/// Bulk (avatar) frames in flight per link. Small on purpose: signaling waits
/// behind at most one bulk frame.
const BULK_DEPTH: usize = 2;

/// Link timers. Keepalives are control traffic; a muted call stays alive.
#[derive(Debug, Clone, Copy)]
pub struct LinkTiming {
	/// Keepalive send interval.
	pub keepalive: Duration,
	/// Inbound silence after which the link is declared dead.
	pub idle: Duration,
	/// Deadline for the peer's `Hello` after the handshake.
	pub hello: Duration,
}

impl Default for LinkTiming {
	fn default() -> Self {
		Self {
			keepalive: Duration::from_secs(5),
			idle: Duration::from_secs(15),
			hello: Duration::from_secs(5),
		}
	}
}

/// What a link needs to know about the local engine.
pub struct LinkParams {
	/// Device identity.
	pub identity: Arc<Identity>,
	/// Advertised display name.
	pub display_name: arc_swap::ArcSwap<String>,
	/// Random per-process value that tells a restart from a reconnect.
	pub incarnation: [u8; 16],
	/// Timers.
	pub timing: LinkTiming,
}

/// Why a link could not be established.
#[derive(Debug, Error)]
pub enum LinkError {
	#[error("tcp: {0}")]
	Io(#[from] std::io::Error),
	#[error("handshake: {0}")]
	Handshake(#[from] HandshakeError),
	#[error("control: {0}")]
	Control(#[from] ControlError),
	#[error("peer sent no Hello in time")]
	HelloTimeout,
	#[error("first message was not a Hello")]
	NotHello,
	#[error("manager stopped")]
	Stopped,
}

/// A live, authenticated link.
pub struct Established {
	/// Manager-local id.
	pub id: LinkId,
	/// Authenticated peer key digest.
	pub fingerprint: Fingerprint,
	/// Authenticated peer key.
	pub key: PublicKey,
	/// Untrusted display name from the peer's `Hello`.
	pub name: String,
	/// Whether the local engine dialled.
	pub dialled_locally: bool,
	/// Epoch of the secure session.
	pub epoch: [u8; 16],
	/// Local address of the control connection.
	pub local_ip: std::net::IpAddr,
	/// The session, shared with the media endpoint.
	pub session: Arc<SecureSession>,
	/// Outbound control queue.
	pub outbox: mpsc::Sender<Envelope>,
	/// Low-priority queue (avatar chunks); drained only when `outbox` is empty.
	pub bulk: mpsc::Sender<Envelope>,
	tasks: [AbortHandle; 2],
}

impl Drop for Established {
	fn drop(&mut self) {
		self.tasks.iter().for_each(AbortHandle::abort);
	}
}

/// Link notifications for the manager.
pub enum LinkEvent {
	/// A link finished its handshake and `Hello` exchange.
	Up(Box<Established>),
	/// An accepted envelope body.
	Message { link: LinkId, body: Body },
	/// The link ended.
	Down { link: LinkId },
	/// A dial or accept failed before the link was usable.
	Failed { link: LinkId, error: String },
}

/// Runs the handshake over `stream` and starts the link tasks.
pub async fn establish(
	mut stream: TcpStream,
	role: Role,
	expected: Option<Fingerprint>,
	params: &LinkParams,
	id: LinkId,
	events: mpsc::Sender<LinkEvent>,
) -> Result<(), LinkError> {
	stream.set_nodelay(true)?;
	let local_ip = stream.local_addr()?.ip();
	let session = Arc::new(match role {
		Role::Initiator => initiate(&mut stream, &params.identity, expected.as_ref()).await?,
		Role::Responder => respond(&mut stream, &params.identity).await?,
	});
	let (mut sender, mut receiver) = control_channel(stream, Arc::clone(&session));
	let hello = new_hello(
		&params.identity.fingerprint(),
		&params.display_name.load(),
		params.incarnation,
	);
	sender.send(&new_envelope(Body::Hello(hello))).await?;
	let first = timeout(params.timing.hello, receiver.recv())
		.await
		.map_err(|_| LinkError::HelloTimeout)??;
	let Some(Body::Hello(peer_hello)) = first.body else {
		return Err(LinkError::NotHello);
	};

	let (outbox, queue) = mpsc::channel::<Envelope>(OUTBOX_DEPTH);
	let (bulk, bulk_queue) = mpsc::channel::<Envelope>(BULK_DEPTH);
	let timing = params.timing;
	let writer = tokio::spawn({
		let events = events.clone();
		async move {
			write_loop(sender, queue, bulk_queue, timing.keepalive).await;
			let _ = events.send(LinkEvent::Down { link: id }).await;
		}
	});
	let reader = tokio::spawn({
		let (events, outbox) = (events.clone(), outbox.clone());
		async move {
			read_loop(receiver, outbox, &events, id, timing.idle).await;
			let _ = events.send(LinkEvent::Down { link: id }).await;
		}
	});
	let established = Established {
		id,
		fingerprint: session.remote_fingerprint(),
		key: *session.remote_key(),
		name: relay_net::discovery::sanitize_display_name(&peer_hello.display_name),
		dialled_locally: role == Role::Initiator,
		epoch: epoch_bytes(session.epoch()),
		local_ip,
		session,
		outbox,
		bulk,
		tasks: [writer.abort_handle(), reader.abort_handle()],
	};
	events
		.send(LinkEvent::Up(Box::new(established)))
		.await
		.map_err(|_| LinkError::Stopped)
}

/// The raw bytes of a link epoch.
pub fn epoch_bytes(epoch: &LinkEpoch) -> [u8; 16] {
	*epoch.as_bytes()
}

async fn write_loop<W: tokio::io::AsyncWrite + Unpin>(
	mut sender: relay_net::control::ControlSender<W>,
	mut queue: mpsc::Receiver<Envelope>,
	mut bulk: mpsc::Receiver<Envelope>,
	keepalive: Duration,
) {
	let mut tick = interval(keepalive);
	tick.set_missed_tick_behavior(MissedTickBehavior::Delay);
	tick.tick().await;
	let mut nonce = 0u64;
	loop {
		// Biased: signaling and keepalives always go before bulk frames.
		let envelope = tokio::select! {
			biased;
			queued = queue.recv() => match queued {
				Some(envelope) => envelope,
				None => break,
			},
			_ = tick.tick() => {
				nonce += 1;
				new_envelope(Body::Keepalive(Keepalive { nonce, reply: false }))
			}
			Some(envelope) = bulk.recv() => envelope,
		};
		if sender.send(&envelope).await.is_err() {
			break;
		}
	}
	let _ = sender.shutdown().await;
}

async fn read_loop<R: tokio::io::AsyncRead + Unpin>(
	mut receiver: relay_net::control::ControlReceiver<R>,
	outbox: mpsc::Sender<Envelope>,
	events: &mpsc::Sender<LinkEvent>,
	id: LinkId,
	idle: Duration,
) {
	loop {
		let envelope = match timeout(idle, receiver.recv()).await {
			Ok(Ok(envelope)) => envelope,
			Ok(Err(error)) => {
				tracing::debug!(%error, link = id, "control link closed");
				return;
			}
			Err(_) => {
				tracing::debug!(link = id, "control link idle timeout");
				return;
			}
		};
		match envelope.body {
			Some(Body::Keepalive(ping)) if !ping.reply => {
				let reply = Keepalive {
					nonce: ping.nonce,
					reply: true,
				};
				if outbox
					.try_send(new_envelope(Body::Keepalive(reply)))
					.is_err()
				{
					return;
				}
			}
			Some(Body::Keepalive(_) | Body::Hello(_)) | None => {}
			Some(body) => {
				if events
					.send(LinkEvent::Message { link: id, body })
					.await
					.is_err()
				{
					return;
				}
			}
		}
	}
}
