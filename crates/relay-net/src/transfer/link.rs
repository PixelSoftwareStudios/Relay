//! An established data link: encrypted frames between the two state machines of one transfer.
//!
//! Each half validates direction, ordering and bounds so a hostile peer cannot push more than
//! the declared size, go backwards, or exceed the frame limit. Every error is terminal.
//! Nothing is buffered beyond one frame: the kernel socket buffer and the receiver's reading
//! pace provide backpressure, and the pacer shapes the sender.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use relay_messages::transfer::TransferMsg;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadHalf, WriteHalf, split};
use tokio::sync::{Notify, OwnedSemaphorePermit};
use tokio::time::timeout;

use super::DataError;
use super::frame::{decode, encode, may_send};
use super::limits::{DATA_CHUNK_MAX_BYTES, DATA_IDLE_TIMEOUT, DATA_RECORD_MAX_BYTES};
use super::pacer::Pacer;
use crate::handshake::gate::{SourceKey, source_key};
use crate::identity::Fingerprint;
use crate::record::{Lane, RecordHeader, RecordKind, sealed_len};
use crate::session::SecureSession;

/// Which end of the file this link serves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferRole {
	/// Reads the file: sends chunks and `Done`; receives credit and the result.
	Sender,
	/// Writes the file: receives chunks and `Done`; sends credit and the result.
	Receiver,
}

/// Fixed properties of one link.
#[derive(Debug, Clone)]
pub struct LinkParams {
	pub role: TransferRole,
	/// Declared file size; chunk offsets and credit are validated against it.
	pub size: u64,
	pub offer_id: [u8; 16],
	pub session_id: [u8; 16],
	pub idle_timeout: Duration,
}

impl LinkParams {
	pub fn new(role: TransferRole, size: u64, offer_id: [u8; 16], session_id: [u8; 16]) -> Self {
		Self {
			role,
			size,
			offer_id,
			session_id,
			idle_timeout: DATA_IDLE_TIMEOUT,
		}
	}
}

/// Established links per source address.
pub(crate) type SourceCounts = Arc<Mutex<std::collections::HashMap<SourceKey, usize>>>;

/// Releases the hub's global and per-source slots when the last half is dropped.
pub(crate) struct LinkGuard {
	_permit: OwnedSemaphorePermit,
	source: Option<(SourceKey, SourceCounts)>,
}

impl LinkGuard {
	pub(crate) fn new(
		permit: OwnedSemaphorePermit,
		source: Option<(std::net::IpAddr, SourceCounts)>,
	) -> Self {
		Self {
			_permit: permit,
			source: source.map(|(ip, map)| (source_key(ip), map)),
		}
	}
}

impl Drop for LinkGuard {
	fn drop(&mut self) {
		if let Some((key, map)) = self.source.take() {
			let mut map = map.lock().unwrap_or_else(|p| p.into_inner());
			if let Some(n) = map.get_mut(&key) {
				*n = n.saturating_sub(1);
				if *n == 0 {
					map.remove(&key);
				}
			}
		}
	}
}

#[derive(Default)]
struct CloseState {
	closed: AtomicBool,
	notify: Notify,
}

/// Closes both halves promptly, including a send blocked on backpressure or the pacer.
#[derive(Clone)]
pub struct CloseHandle(Arc<CloseState>);

impl CloseHandle {
	pub fn close(&self) {
		self.0.closed.store(true, Ordering::Release);
		self.0.notify.notify_waiters();
	}

	pub fn is_closed(&self) -> bool {
		self.0.closed.load(Ordering::Acquire)
	}

	async fn closed(&self) {
		loop {
			let notified = self.0.notify.notified();
			if self.is_closed() {
				return;
			}
			notified.await;
		}
	}
}

impl std::fmt::Debug for CloseHandle {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.write_str("CloseHandle")
	}
}

/// Both halves of an established data link.
pub struct DataLink<S> {
	pub tx: DataTx<WriteHalf<S>>,
	pub rx: DataRx<ReadHalf<S>>,
	close: CloseHandle,
	peer: Fingerprint,
	params: LinkParams,
}

impl<S: AsyncRead + AsyncWrite> DataLink<S> {
	/// Wraps a handshaken stream. The pacer applies to chunks sent by this side.
	pub fn new(stream: S, session: Arc<SecureSession>, params: LinkParams, pacer: Pacer) -> Self {
		Self::with_guard(stream, session, params, pacer, None)
	}

	pub(crate) fn with_guard(
		stream: S,
		session: Arc<SecureSession>,
		params: LinkParams,
		pacer: Pacer,
		guard: Option<LinkGuard>,
	) -> Self {
		let (read, write) = split(stream);
		let close = CloseHandle(Arc::default());
		let guard = guard.map(Arc::new);
		Self {
			peer: session.remote_fingerprint(),
			tx: DataTx {
				writer: write,
				session: Arc::clone(&session),
				params: params.clone(),
				pacer,
				close: close.clone(),
				sent: 0,
				failed: false,
				_guard: guard.clone(),
			},
			rx: DataRx {
				reader: read,
				session,
				params: params.clone(),
				close: close.clone(),
				last_sequence: None,
				expected: 0,
				credit: 0,
				done: false,
				failed: false,
				_guard: guard,
			},
			close,
			params,
		}
	}

	pub fn close_handle(&self) -> CloseHandle {
		self.close.clone()
	}

	/// The authenticated peer key (equals the one the token was bound to).
	pub fn peer(&self) -> &Fingerprint {
		&self.peer
	}

	pub fn params(&self) -> &LinkParams {
		&self.params
	}
}

/// Sending half.
pub struct DataTx<W> {
	writer: W,
	session: Arc<SecureSession>,
	params: LinkParams,
	pacer: Pacer,
	close: CloseHandle,
	sent: u64,
	failed: bool,
	_guard: Option<Arc<LinkGuard>>,
}

impl<W: AsyncWrite + Unpin> DataTx<W> {
	/// The pacer of this link, for changing its rate while the transfer runs.
	pub fn pacer(&self) -> &Pacer {
		&self.pacer
	}

	/// Sends one message. Chunks larger than 32 KiB are split; each piece is paced. A failure,
	/// including a stalled peer, is terminal.
	pub async fn send(&mut self, msg: &TransferMsg) -> Result<(), DataError> {
		if self.failed {
			return Err(DataError::Closed);
		}
		if !may_send(self.params.role, msg) {
			return Err(DataError::BadFrame("message not allowed in this direction"));
		}
		let close = self.close.clone();
		let result = tokio::select! {
			r = self.send_inner(msg) => r,
			() = close.closed() => Err(DataError::Closed),
		};
		self.failed = result.is_err();
		result
	}

	async fn send_inner(&mut self, msg: &TransferMsg) -> Result<(), DataError> {
		match msg {
			TransferMsg::Chunk { offset, data } => {
				let end = offset.saturating_add(data.len() as u64);
				if *offset != self.sent || end > self.params.size || data.is_empty() {
					return Err(DataError::BadFrame("chunk is out of order or past the end"));
				}
				for (i, piece) in data.chunks(DATA_CHUNK_MAX_BYTES).enumerate() {
					self.pacer.acquire(piece.len()).await;
					let piece_offset = offset + (i * DATA_CHUNK_MAX_BYTES) as u64;
					self.write(&TransferMsg::Chunk {
						offset: piece_offset,
						data: piece.to_vec(),
					})
					.await?;
					self.sent = piece_offset + piece.len() as u64;
					// Bulk bytes are polite: let voice and signaling tasks run between frames.
					tokio::task::yield_now().await;
				}
				Ok(())
			}
			TransferMsg::Done if self.sent != self.params.size => {
				Err(DataError::BadFrame("done before every byte was sent"))
			}
			other => self.write(other).await,
		}
	}

	async fn write(&mut self, msg: &TransferMsg) -> Result<(), DataError> {
		let record = self.session.seal(RecordKind::Data, &encode(msg)?)?;
		let mut frame = Vec::with_capacity(4 + record.len());
		frame.extend_from_slice(&(record.len() as u32).to_be_bytes());
		frame.extend_from_slice(&record);
		let idle = self.params.idle_timeout;
		timeout(idle, async {
			self.writer.write_all(&frame).await?;
			self.writer.flush().await
		})
		.await
		.map_err(|_| DataError::Idle)??;
		Ok(())
	}

	/// Orderly end of this half (TCP FIN after buffered data).
	pub async fn shutdown(&mut self) {
		let _ = self.writer.shutdown().await;
	}
}

/// Receiving half.
pub struct DataRx<R> {
	reader: R,
	session: Arc<SecureSession>,
	params: LinkParams,
	close: CloseHandle,
	last_sequence: Option<u64>,
	/// Receiver role: next chunk offset. Sender role: unused.
	expected: u64,
	/// Sender role: highest credit seen.
	credit: u64,
	done: bool,
	failed: bool,
	_guard: Option<Arc<LinkGuard>>,
}

impl<R: AsyncRead + Unpin> DataRx<R> {
	/// Next valid message from the peer. `Err(Closed)` also covers a clean EOF.
	pub async fn recv(&mut self) -> Result<TransferMsg, DataError> {
		if self.failed {
			return Err(DataError::Closed);
		}
		let close = self.close.clone();
		let idle = self.params.idle_timeout;
		let result = tokio::select! {
			r = timeout(idle, self.recv_inner()) => r.unwrap_or(Err(DataError::Idle)),
			() = close.closed() => Err(DataError::Closed),
		};
		self.failed = result.is_err();
		result
	}

	async fn recv_inner(&mut self) -> Result<TransferMsg, DataError> {
		let mut prefix = [0u8; 4];
		match self.reader.read_exact(&mut prefix).await {
			Ok(_) => {}
			Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
				return Err(DataError::Closed);
			}
			Err(e) => return Err(e.into()),
		}
		let len = u32::from_be_bytes(prefix) as usize;
		if len < sealed_len(1) || len > DATA_RECORD_MAX_BYTES {
			return Err(DataError::BadFrame("record length out of bounds"));
		}
		let mut record = vec![0u8; len];
		self.reader.read_exact(&mut record).await?;
		let header = RecordHeader::parse(&record).map_err(|_| DataError::BadFrame("bad header"))?;
		if header.lane() != Lane::Control
			|| self
				.last_sequence
				.is_some_and(|last| header.sequence() <= last)
		{
			return Err(DataError::BadFrame("record out of sequence"));
		}
		let opened = self.session.open(&header, &record)?;
		if opened.kind != RecordKind::Data {
			return Err(DataError::BadFrame("not a data frame"));
		}
		let msg = decode(&opened.body)?;
		self.check(&msg)?;
		self.last_sequence = Some(header.sequence());
		Ok(msg)
	}

	/// Direction, ordering and size rules; mutates progress only for accepted frames.
	fn check(&mut self, msg: &TransferMsg) -> Result<(), DataError> {
		let bad = DataError::BadFrame;
		if matches!(msg, TransferMsg::Cancel) {
			return Ok(());
		}
		if self.done {
			return Err(bad("frame after the final marker"));
		}
		match (self.params.role, msg) {
			(TransferRole::Receiver, TransferMsg::Chunk { offset, data }) => {
				let end = offset.saturating_add(data.len() as u64);
				if *offset != self.expected {
					return Err(bad("chunk offset is not the next expected offset"));
				}
				if end > self.params.size {
					return Err(bad("chunk runs past the declared size"));
				}
				self.expected = end;
			}
			(TransferRole::Receiver, TransferMsg::Done) => {
				if self.expected != self.params.size {
					return Err(bad("final marker before every byte arrived"));
				}
				self.done = true;
			}
			(TransferRole::Sender, TransferMsg::Credit(n)) => {
				if *n < self.credit {
					return Err(bad("credit went backwards"));
				}
				if *n > self.params.size {
					return Err(bad("credit exceeds the declared size"));
				}
				self.credit = *n;
			}
			(TransferRole::Sender, TransferMsg::Result(_)) => self.done = true,
			_ => return Err(bad("message not allowed in this direction")),
		}
		Ok(())
	}
}
