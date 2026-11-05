//! Blind listening tests: render the same utterances through several
//! suppressors, shuffle the clips under neutral names, collect 1-5 ratings
//! and a preferred clip per trial, then de-blind into a summary.
//!
//! Nothing that identifies a backend is ever written outside `key/key.json`.

pub mod cli;
pub mod key;
pub mod prepare;
pub mod score;
pub mod select;

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ListenError {
	#[error("{0}")]
	Bad(String),
	#[error("io error: {0}")]
	Io(#[from] std::io::Error),
	#[error("json error: {0}")]
	Json(#[from] serde_json::Error),
	#[error("audio error: {0}")]
	Audio(#[from] crate::audio::BenchError),
}

pub type Result<T> = std::result::Result<T, ListenError>;

pub fn bad<T>(message: impl Into<String>) -> Result<T> {
	Err(ListenError::Bad(message.into()))
}
