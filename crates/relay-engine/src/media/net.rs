//! UDP side of media: one endpoint per peer link, a receive task feeding the
//! render worker through a bounded queue, and the production packet sink.

use std::net::SocketAddr;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use relay_net::AddressPolicy;
use relay_net::media::MediaEndpoint;
use relay_net::session::SecureSession;
use rtrb::{Consumer, Producer, RingBuffer};
use tokio::runtime::Handle;
use tokio::task::AbortHandle;

use super::MediaError;
use super::packet::Ingress;
use super::recv::SourceId;
use super::send::PacketSink;

/// Packets buffered between the network task and the render worker.
const INGRESS_DEPTH: usize = 256;
/// Receive errors in a row after which the task gives up.
const RECV_ERRORS_MAX: u32 = 100;

/// Counters of one peer's receive task.
#[derive(Debug, Default)]
pub struct NetStats {
	/// Packets handed to the render worker.
	pub forwarded: AtomicU64,
	/// Packets dropped because the queue to the render worker was full.
	pub overflow: AtomicU64,
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
	) -> Result<(Self, Consumer<Ingress>), MediaError> {
		let endpoint = Arc::new(MediaEndpoint::bind(local, session, policy).await?);
		let (producer, consumer) = RingBuffer::new(INGRESS_DEPTH);
		let stats = Arc::new(NetStats::default());
		let task = tokio::spawn(receive_loop(
			Arc::clone(&endpoint),
			producer,
			origin,
			Arc::clone(&stats),
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
) {
	let mut errors = 0;
	loop {
		match endpoint.recv().await {
			Ok(packet) => {
				errors = 0;
				// relay-net already authenticated and replay-checked the
				// datagram; the call id and generation are its sealed binding.
				let arrival = u64::try_from(origin.elapsed().as_micros()).unwrap_or(u64::MAX);
				let Some(ingress) =
					Ingress::new(packet.call_id, packet.generation, arrival, &packet.payload)
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
				errors += 1;
				tracing::debug!(%error, "media receive error");
				if errors >= RECV_ERRORS_MAX {
					return;
				}
				tokio::time::sleep(Duration::from_millis(10)).await;
			}
		}
	}
}

/// Seals and sends packets on a UDP endpoint from a worker thread.
///
/// The send is a single non-blocking `send_to` driven to completion on the
/// control runtime; the worker never waits on anything else. The sealed
/// datagram buffer is allocated inside relay-net per packet; that allocation
/// is a known gap of this sink.
pub struct EndpointSink {
	endpoint: Arc<MediaEndpoint>,
	runtime: Handle,
}

impl EndpointSink {
	/// Wraps an endpoint.
	pub fn new(endpoint: Arc<MediaEndpoint>, runtime: Handle) -> Self {
		Self { endpoint, runtime }
	}
}

impl PacketSink for EndpointSink {
	fn send(&mut self, call: [u8; 16], generation: u32, rtp: &[u8]) -> Result<(), MediaError> {
		Ok(self
			.runtime
			.block_on(self.endpoint.send_audio(&call, generation, rtp))?)
	}
}
