//! UDP side of media: one endpoint per peer link, a receive task feeding the
//! render worker through a bounded queue, and the production packet sink.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use relay_net::AddressPolicy;
use relay_net::media::{MAX_PAYLOAD_BYTES, MediaEndpoint};
use relay_net::session::SecureSession;
use rtrb::{Consumer, Producer, RingBuffer};
use tokio::task::AbortHandle;

use super::MediaError;
use super::events::{EventSink, MediaEvent};
use super::packet::Ingress;
use super::recv::SourceId;
use super::send::PacketSink;
use super::supervise::{FailureKind, WorkerFailure, WorkerId};

/// Packets buffered between the network task and the render worker. Deep
/// enough for a long scheduling hiccup, shallow enough that after a stall the
/// queue never holds seconds of stale audio.
const INGRESS_DEPTH: usize = 32;
/// Receive errors in a row after which the task concludes the socket is dead.
const RECV_ERRORS_MAX: u32 = 100;

/// Counters of one peer's receive task.
#[derive(Debug, Default)]
pub struct NetStats {
	/// Packets handed to the render worker.
	pub forwarded: AtomicU64,
	/// Packets dropped because the queue to the render worker was full.
	pub overflow: AtomicU64,
	/// Receive errors that were not ICMP-unreachable echoes.
	pub errors: AtomicU64,
}

/// A bound endpoint and its receive task.
pub struct PeerSocket {
	/// The endpoint shared by the receive task, senders and path validation.
	pub endpoint: Arc<MediaEndpoint>,
	/// Identifies this task's packets to the render worker.
	pub source: SourceId,
	/// Counters.
	pub stats: Arc<NetStats>,
	task: AbortHandle,
}

impl Drop for PeerSocket {
	fn drop(&mut self) {
		self.task.abort();
	}
}

impl PeerSocket {
	/// Binds `local` for `session` and starts the receive task. Returns the
	/// socket and the queue the render worker must drain.
	pub async fn open(
		local: SocketAddr,
		session: Arc<SecureSession>,
		policy: AddressPolicy,
		source: SourceId,
		origin: Instant,
		events: EventSink,
	) -> Result<(Self, Consumer<Ingress>), MediaError> {
		let endpoint = Arc::new(MediaEndpoint::bind(local, session, policy).await?);
		let (producer, consumer) = RingBuffer::new(INGRESS_DEPTH);
		let stats = Arc::new(NetStats::default());
		let task = tokio::spawn(receive_loop(
			Arc::clone(&endpoint),
			producer,
			origin,
			Arc::clone(&stats),
			events,
		));
		Ok((
			Self {
				endpoint,
				source,
				stats,
				task: task.abort_handle(),
			},
			consumer,
		))
	}
}

async fn receive_loop(
	endpoint: Arc<MediaEndpoint>,
	mut queue: Producer<Ingress>,
	origin: Instant,
	stats: Arc<NetStats>,
	events: EventSink,
) {
	let mut errors = 0;
	let mut payload = [0u8; MAX_PAYLOAD_BYTES];
	loop {
		match endpoint.recv_into(&mut payload).await {
			Ok(meta) => {
				errors = 0;
				// relay-net already authenticated and replay-checked the
				// datagram; the call id and generation are its sealed binding.
				let arrival = u64::try_from(origin.elapsed().as_micros()).unwrap_or(u64::MAX);
				let Some(ingress) =
					Ingress::new(meta.call_id, meta.generation, arrival, &payload[..meta.len])
				else {
					continue;
				};
				let counter = if queue.push(ingress).is_ok() {
					&stats.forwarded
				} else {
					&stats.overflow
				};
				counter.fetch_add(1, Ordering::Relaxed);
			}
			Err(error) => {
				// Connection-reset echoes never reach here (relay-net counts and
				// skips them); this is a failing socket.
				errors += 1;
				stats.errors.fetch_add(1, Ordering::Relaxed);
				tracing::debug!(%error, "media receive error");
				if errors >= RECV_ERRORS_MAX {
					tracing::error!(%error, "media socket is dead; receive task stopped");
					events(MediaEvent::WorkerFailed(WorkerFailure {
						worker: WorkerId::Network,
						kind: FailureKind::Socket,
						permanent: true,
						call: None,
					}));
					return;
				}
				tokio::time::sleep(Duration::from_millis(10)).await;
			}
		}
	}
}

/// Seals and sends packets on a UDP endpoint from a worker thread.
///
/// The send is non-blocking: if the socket buffer is full the packet is
/// dropped and counted, never waited for. Real-time audio drops rather than
/// parks the capture thread behind a congested link (RT-04). Sealing uses a
/// stack buffer; nothing is allocated per packet.
pub struct EndpointSink {
	endpoint: Arc<MediaEndpoint>,
}

impl EndpointSink {
	/// Wraps an endpoint.
	pub fn new(endpoint: Arc<MediaEndpoint>) -> Self {
		Self { endpoint }
	}
}

impl PacketSink for EndpointSink {
	fn send(&mut self, call: [u8; 16], generation: u32, rtp: &[u8]) -> Result<(), MediaError> {
		match self.endpoint.try_send_audio(&call, generation, rtp) {
			Ok(()) => Ok(()),
			Err(relay_net::media::MediaError::Io(error))
				if error.kind() == std::io::ErrorKind::WouldBlock =>
			{
				Err(MediaError::WouldBlock)
			}
			Err(error) => Err(error.into()),
		}
	}
}
