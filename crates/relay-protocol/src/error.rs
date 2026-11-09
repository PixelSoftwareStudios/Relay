//! Error types for the protocol crate.

use thiserror::Error;

/// A configuration field cannot satisfy the protocol component's numeric invariants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("invalid configuration: {0}")]
pub struct ConfigError(pub &'static str);

/// A nearest extended counter value lies beyond the representable timeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
#[error("extended counter exhausted")]
pub struct ExtendError;

/// Reasons an RTP datagram is rejected or cannot be serialized.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum RtpError {
	#[error("datagram shorter than the fixed RTP header")]
	TooShort,
	#[error("unsupported RTP version {0}")]
	BadVersion(u8),
	#[error("padding is not part of the Relay RTP profile")]
	Padding,
	#[error("header extensions are not part of the Relay RTP profile")]
	Extension,
	#[error("CSRC lists are not part of the Relay RTP profile")]
	Csrc,
	#[error("unexpected payload type {0}")]
	PayloadType(u8),
	#[error("empty payload")]
	EmptyPayload,
	#[error("payload of {len} bytes exceeds the {max} byte limit")]
	PayloadTooLarge { len: usize, max: usize },
	#[error("output buffer too small: need {need} bytes")]
	OutputTooSmall { need: usize },
}

/// Failures of the frame assembler.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum AssemblyError {
	#[error("pushing {pushed} samples would overflow the {capacity} sample assembler")]
	Overflow { pushed: usize, capacity: usize },
}
