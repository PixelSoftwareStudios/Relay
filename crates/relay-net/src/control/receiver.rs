use std::collections::{HashMap, VecDeque};
use std::sync::Arc;

use prost::Message;
use tokio::io::{AsyncRead, AsyncReadExt};

use super::session_rate::{LinkClock, SessionRateLimit};
use super::validate::check_hello;
use super::{ControlError, validate_envelope};
use crate::limits::CONTROL_RECORD_MAX_BYTES;
use crate::proto::{Envelope, envelope::Body};
use crate::record::{Lane, RecordHeader, RecordKind, sealed_len};
use crate::session::{OpenError, SecureSession};

const MESSAGE_ID_RETENTION: usize = 1024;

pub struct ControlReceiver<R> {
	reader: R,
	session: Arc<SecureSession>,
	last_sequence: Option<u64>,
	hello_seen: bool,
	seen: HashMap<Vec<u8>, blake3::Hash>,
	seen_order: VecDeque<Vec<u8>>,
	failed: bool,
	clock: LinkClock,
	session_limit: SessionRateLimit,
}

impl<R: AsyncRead + Unpin> ControlReceiver<R> {
	pub(super) fn new(reader: R, session: Arc<SecureSession>) -> Self {
		Self {
			reader,
			session,
			last_sequence: None,
			hello_seen: false,
			seen: HashMap::new(),
			seen_order: VecDeque::new(),
			failed: false,
			clock: LinkClock::start(),
			session_limit: SessionRateLimit::new(0),
		}
	}

	/// Replaces the flood guard applied to `SessionMessage` envelopes.
	pub fn set_session_limit(&mut self, limit: SessionRateLimit) {
		self.session_limit = limit;
	}

	/// Receives the next accepted envelope. The first must be a `Hello` whose key digest
	/// equals the authenticated static key. Identical re-sent message ids are dropped.
	pub async fn recv(&mut self) -> Result<Envelope, ControlError> {
		if self.failed {
			return Err(ControlError::Closed);
		}
		loop {
			match self.recv_one().await {
				Ok(Some(envelope)) => return Ok(envelope),
				Ok(None) => continue,
				Err(error) => {
					self.failed = true;
					return Err(error);
				}
			}
		}
	}

	async fn recv_one(&mut self) -> Result<Option<Envelope>, ControlError> {
		let mut prefix = [0u8; 4];
		self.reader.read_exact(&mut prefix).await?;
		let len = u32::from_be_bytes(prefix) as usize;
		if len < sealed_len(0) || len > CONTROL_RECORD_MAX_BYTES {
			return Err(ControlError::BadLength);
		}
		let mut record = vec![0u8; len];
		self.reader.read_exact(&mut record).await?;

		let header = RecordHeader::parse(&record).map_err(|_| OpenError::Malformed)?;
		if header.lane() != Lane::Control {
			return Err(ControlError::WrongKind);
		}
		// Cheap pre-check without mutation; the window is committed only after acceptance.
		if self
			.last_sequence
			.is_some_and(|last| header.sequence() <= last)
		{
			return Err(ControlError::Replay);
		}
		let opened = self.session.open(&header, &record)?;
		if opened.kind != RecordKind::Envelope {
			return Err(ControlError::WrongKind);
		}
		let envelope =
			Envelope::decode(opened.body.as_slice()).map_err(|_| ControlError::Decode)?;
		validate_envelope(&envelope)?;
		if !self.hello_seen && !matches!(envelope.body, Some(Body::Hello(_))) {
			return Err(ControlError::HelloRequired);
		}
		if let Some(Body::Hello(hello)) = &envelope.body {
			check_hello(hello, &self.session.remote_fingerprint())
				.map_err(|_| ControlError::HelloRequired)?;
		}
		if matches!(envelope.body, Some(Body::SessionMessage(_)))
			&& !self.session_limit.allow(self.clock.now_ms())
		{
			return Err(ControlError::RateLimited);
		}
		let duplicate = self.note_message_id(&envelope)?;

		// Fully accepted: only now does receive state advance.
		self.last_sequence = Some(header.sequence());
		if matches!(envelope.body, Some(Body::Hello(_))) {
			self.hello_seen = true;
		}
		Ok((!duplicate).then_some(envelope))
	}

	/// Returns true for an identical duplicate, errors for a conflicting one.
	fn note_message_id(&mut self, envelope: &Envelope) -> Result<bool, ControlError> {
		let mut normalized = envelope.clone();
		normalized.message_id.clear();
		let digest = blake3::hash(&normalized.encode_to_vec());
		if let Some(previous) = self.seen.get(&envelope.message_id) {
			return if *previous == digest {
				Ok(true)
			} else {
				Err(ControlError::ConflictingDuplicate)
			};
		}
		if self.seen_order.len() >= MESSAGE_ID_RETENTION
			&& let Some(oldest) = self.seen_order.pop_front()
		{
			self.seen.remove(&oldest);
		}
		self.seen.insert(envelope.message_id.clone(), digest);
		self.seen_order.push_back(envelope.message_id.clone());
		Ok(false)
	}
}
