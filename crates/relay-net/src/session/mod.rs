//! A completed Noise handshake turned into directional, nonce-explicit record protection.

mod epoch;
pub mod switch;

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use snow::StatelessTransportState;
use thiserror::Error;

pub use epoch::LinkEpoch;

use crate::identity::{Fingerprint, PublicKey};
use crate::limits::{AEAD_TAG_BYTES, REKEY_MAX_AGE, REKEY_MAX_RECORDS};
use crate::record::{INNER_PREFIX_BYTES, RecordHeader, RecordKind, sealed_len};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
	Initiator,
	Responder,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum SealError {
	#[error("record kind does not belong on this lane")]
	WrongLane,
	#[error("key lifetime exhausted; a new handshake is required")]
	Exhausted,
	#[error("body too large")]
	TooLarge,
	#[error("encryption failed")]
	Crypto,
}

/// Deliberately coarse: callers learn that a record was rejected, not why authentication
/// failed, so the receive path offers no oracle.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum OpenError {
	#[error("record is malformed")]
	Malformed,
	#[error("record epoch does not belong to this session")]
	WrongEpoch,
	#[error("record failed authentication")]
	Authentication,
}

/// An authenticated, structurally validated record.
#[derive(Debug)]
pub struct Opened {
	pub header: RecordHeader,
	pub kind: RecordKind,
	pub body: Vec<u8>,
}

/// One epoch's keys. Send counters are owned here and never reset: a fresh epoch means a
/// fresh `SecureSession`. A counter is consumed even when the send later fails.
pub struct SecureSession {
	transport: StatelessTransportState,
	epoch: LinkEpoch,
	role: Role,
	remote: PublicKey,
	handshake_hash: [u8; 32],
	next_sequence: [AtomicU64; 2],
	created: Instant,
}

impl SecureSession {
	pub(crate) fn new(
		transport: StatelessTransportState,
		epoch: LinkEpoch,
		role: Role,
		remote: PublicKey,
		handshake_hash: [u8; 32],
	) -> Self {
		Self {
			transport,
			epoch,
			role,
			remote,
			handshake_hash,
			next_sequence: [AtomicU64::new(0), AtomicU64::new(0)],
			created: Instant::now(),
		}
	}

	pub const fn epoch(&self) -> &LinkEpoch {
		&self.epoch
	}

	pub const fn role(&self) -> Role {
		self.role
	}

	pub const fn remote_key(&self) -> &PublicKey {
		&self.remote
	}

	/// The Noise handshake hash `h`: identical on both ends, and it commits to the prologue
	/// (protocol and epoch), both static keys and every handshake message. It is a public
	/// channel-binding value (not a key); upper layers bind their own proofs to it.
	pub const fn handshake_hash(&self) -> &[u8; 32] {
		&self.handshake_hash
	}

	pub fn remote_fingerprint(&self) -> Fingerprint {
		self.remote.fingerprint()
	}

	/// True once the key lifetime or per-lane record budget is nearly spent (or when the
	/// hard limit is hit, sending fails). Callers start a fresh handshake before this.
	pub fn needs_rekey(&self) -> bool {
		let soft = REKEY_MAX_RECORDS / 2;
		self.created.elapsed() >= REKEY_MAX_AGE / 2
			|| self
				.next_sequence
				.iter()
				.any(|c| c.load(Ordering::Relaxed) >= soft)
	}

	/// Encrypts `body` into a complete record (public header followed by ciphertext).
	pub fn seal(&self, kind: RecordKind, body: &[u8]) -> Result<Vec<u8>, SealError> {
		let lane = kind.lane();
		if sealed_len(body.len()) > snow_limit() {
			return Err(SealError::TooLarge);
		}
		let sequence = self.next_sequence[lane.index()].fetch_add(1, Ordering::Relaxed);
		if sequence >= REKEY_MAX_RECORDS || self.created.elapsed() >= REKEY_MAX_AGE {
			return Err(SealError::Exhausted);
		}
		let header = RecordHeader::new(lane, self.epoch, sequence);
		let header_bytes = header.encode();

		let mut plaintext = Vec::with_capacity(INNER_PREFIX_BYTES + body.len());
		plaintext.extend_from_slice(&header_bytes);
		plaintext.push(kind as u8);
		plaintext.extend_from_slice(body);

		let mut record = vec![0u8; sealed_len(body.len())];
		record[..header_bytes.len()].copy_from_slice(&header_bytes);
		let written = self
			.transport
			.write_message(
				header.nonce(),
				&plaintext,
				&mut record[header_bytes.len()..],
			)
			.map_err(|_| SealError::Crypto)?;
		record.truncate(header_bytes.len() + written);
		Ok(record)
	}

	/// Authenticates and decrypts a record already parsed to `header`. This takes `&self`
	/// and touches no replay state: the caller commits ordering only after this succeeds.
	pub fn open(&self, header: &RecordHeader, record: &[u8]) -> Result<Opened, OpenError> {
		if header.epoch() != &self.epoch {
			return Err(OpenError::WrongEpoch);
		}
		let ciphertext = record
			.get(crate::limits::RECORD_HEADER_BYTES..)
			.ok_or(OpenError::Malformed)?;
		if ciphertext.len() < AEAD_TAG_BYTES + INNER_PREFIX_BYTES {
			return Err(OpenError::Malformed);
		}
		let mut plaintext = vec![0u8; ciphertext.len()];
		let n = self
			.transport
			.read_message(header.nonce(), ciphertext, &mut plaintext)
			.map_err(|_| OpenError::Authentication)?;
		plaintext.truncate(n);

		// Compare the complete inner and outer headers before acting on anything else.
		let (inner_header, rest) = plaintext.split_at(crate::limits::RECORD_HEADER_BYTES);
		if inner_header != header.encode() {
			return Err(OpenError::Authentication);
		}
		let kind = RecordKind::from_byte(rest[0]).ok_or(OpenError::Malformed)?;
		if kind.lane() != header.lane() {
			return Err(OpenError::Malformed);
		}
		Ok(Opened {
			header: *header,
			kind,
			body: rest[1..].to_vec(),
		})
	}

	/// Convenience: parse the header, then [`Self::open`].
	pub fn open_record(&self, record: &[u8]) -> Result<Opened, OpenError> {
		let header = RecordHeader::parse(record).map_err(|_| OpenError::Malformed)?;
		self.open(&header, record)
	}
}

impl std::fmt::Debug for SecureSession {
	fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
		f.debug_struct("SecureSession")
			.field("epoch", &self.epoch)
			.field("role", &self.role)
			.finish_non_exhaustive()
	}
}

const fn snow_limit() -> usize {
	// Noise messages are limited to 65535 bytes, the ciphertext part of a record.
	65_535 + crate::limits::RECORD_HEADER_BYTES
}
