use thiserror::Error;

use crate::limits::{EPOCH_BYTES, RECORD_HEADER_BYTES};
use crate::session::LinkEpoch;

const MAGIC: &[u8; 4] = b"RLY1";
const VERSION: u8 = 1;
const LANE_BIT: u64 = 1 << 63;

/// Nonce namespace. Media and control share a key but never a nonce.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lane {
	Media = 0,
	Control = 1,
}

impl Lane {
	pub const fn index(self) -> usize {
		self as usize
	}
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum HeaderError {
	#[error("record shorter than its header")]
	Short,
	#[error("bad magic, version, flags or lane")]
	Malformed,
	#[error("nonce lane bit disagrees with the lane field")]
	LaneMismatch,
}

/// The public header. Until the inner copy is verified it is only a demultiplexing hint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordHeader {
	lane: Lane,
	epoch: LinkEpoch,
	nonce: u64,
}

impl RecordHeader {
	/// Builds a header for the `sequence`th record of `lane`. The lane bit is derived here
	/// so it can never be chosen independently of the lane field.
	pub const fn new(lane: Lane, epoch: LinkEpoch, sequence: u64) -> Self {
		let nonce = match lane {
			Lane::Media => sequence & !LANE_BIT,
			Lane::Control => sequence | LANE_BIT,
		};
		Self { lane, epoch, nonce }
	}

	pub const fn lane(&self) -> Lane {
		self.lane
	}

	pub const fn epoch(&self) -> &LinkEpoch {
		&self.epoch
	}

	/// Full 64-bit nonce including the lane bit.
	pub const fn nonce(&self) -> u64 {
		self.nonce
	}

	/// Per-lane sequence number (nonce without the lane bit).
	pub const fn sequence(&self) -> u64 {
		self.nonce & !LANE_BIT
	}

	pub fn encode(&self) -> [u8; RECORD_HEADER_BYTES] {
		let mut out = [0u8; RECORD_HEADER_BYTES];
		out[..4].copy_from_slice(MAGIC);
		out[4] = VERSION;
		out[5] = self.lane as u8;
		// bytes 6..8 reserved flags stay zero
		out[8..8 + EPOCH_BYTES].copy_from_slice(self.epoch.as_bytes());
		out[24..].copy_from_slice(&self.nonce.to_be_bytes());
		out
	}

	/// Parses and structurally validates the leading header of `record`.
	pub fn parse(record: &[u8]) -> Result<Self, HeaderError> {
		let raw: &[u8; RECORD_HEADER_BYTES] = record
			.get(..RECORD_HEADER_BYTES)
			.and_then(|s| s.try_into().ok())
			.ok_or(HeaderError::Short)?;
		if &raw[..4] != MAGIC || raw[4] != VERSION || raw[6] != 0 || raw[7] != 0 {
			return Err(HeaderError::Malformed);
		}
		let lane = match raw[5] {
			0 => Lane::Media,
			1 => Lane::Control,
			_ => return Err(HeaderError::Malformed),
		};
		let mut epoch = [0u8; EPOCH_BYTES];
		epoch.copy_from_slice(&raw[8..8 + EPOCH_BYTES]);
		let nonce = u64::from_be_bytes(raw[24..].try_into().map_err(|_| HeaderError::Short)?);
		let bit_lane = if nonce & LANE_BIT == 0 {
			Lane::Media
		} else {
			Lane::Control
		};
		if bit_lane != lane {
			return Err(HeaderError::LaneMismatch);
		}
		Ok(Self {
			lane,
			epoch: LinkEpoch::from_bytes(epoch),
			nonce,
		})
	}
}
