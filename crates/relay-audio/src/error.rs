//! Error types shared by the audio crate.

use thiserror::Error;

/// Failures raised by device, conversion, codec and mixing code.
///
/// Real-time callbacks never construct these; they record counters instead.
#[derive(Debug, Error)]
pub enum AudioError {
	#[error("audio device not found")]
	DeviceNotFound,
	#[error("selected device is missing; pick a replacement explicitly")]
	SelectedDeviceMissing,
	#[error("unsupported stream format: {0}")]
	UnsupportedFormat(String),
	#[error("audio backend failure: {0}")]
	Backend(String),
	#[error("invalid argument: {0}")]
	InvalidArgument(&'static str),
	#[error("resampler error: {0}")]
	Resample(String),
	#[error("opus error: {0}")]
	Opus(String),
	#[error("packet carries {actual} samples, expected {expected}")]
	BadFrameSize { expected: usize, actual: usize },
	#[error("output buffer too small: need {needed}, have {available}")]
	BufferTooSmall { needed: usize, available: usize },
}

impl From<opus::Error> for AudioError {
	fn from(error: opus::Error) -> Self {
		Self::Opus(error.to_string())
	}
}

/// Convenience result alias.
pub type Result<T> = std::result::Result<T, AudioError>;
