//! Per-transfer data links: one extra encrypted TCP connection per file transfer, so a saturated
//! file can never head-of-line block call signaling on the control link.
//!
//! Flow (see `docs/net.md`): the listening side calls [`TransferHub::expect`], which mints a
//! one-time [`TransferToken`] bound to the offer, the session and the expected peer key, and
//! the engine sends it over the authenticated control link as a `DataLinkGrant`. The other
//! side calls [`TransferHub::dial`] with the control link's remote address and the grant's
//! port. The Noise handshake must prove the expected key and carry the token; then both ends
//! pump `relay_messages::transfer::TransferMsg` values through [`DataTx`] and [`DataRx`].

mod frame;
mod handshake;
mod hub;
pub mod limits;
mod link;
mod pacer;
mod token;

use thiserror::Error;

/// Frame codec entry points for the fuzz targets and tests; not part of the API.
#[doc(hidden)]
pub mod fuzzing {
	use relay_messages::transfer::TransferMsg;

	use super::{DataError, frame};

	/// Decodes one data-link frame body.
	pub fn decode_frame(body: &[u8]) -> Result<TransferMsg, DataError> {
		frame::decode(body)
	}

	/// Encodes one data-link frame body.
	pub fn encode_frame(msg: &TransferMsg) -> Result<Vec<u8>, DataError> {
		frame::encode(msg)
	}
}

pub use handshake::Ticket;
pub use hub::{HubConfig, PendingLink, Priority, TransferHub};
pub use link::{CloseHandle, DataLink, DataRx, DataTx, LinkParams, TransferRole};
pub use pacer::{Pacer, SharedLimit};
pub use token::{Expectation, TOKEN_BYTES, TransferToken};

use crate::AddressError;
use crate::handshake::HandshakeError;
use crate::session::{OpenError, SealError};

#[derive(Debug, Error)]
pub enum DataError {
	#[error("i/o error: {0}")]
	Io(#[from] std::io::Error),
	#[error("handshake failed: {0}")]
	Handshake(#[from] HandshakeError),
	#[error("transfer token rejected")]
	TokenRejected,
	#[error("transfer token expired or revoked")]
	Expired,
	#[error("too many pending or open data links")]
	TooManyPending,
	#[error("frame refused: {0}")]
	BadFrame(&'static str),
	#[error("record rejected: {0}")]
	Record(#[from] OpenError),
	#[error("failed to protect outgoing frame: {0}")]
	Seal(#[from] SealError),
	#[error("no data for too long")]
	Idle,
	#[error("data link closed")]
	Closed,
	#[error("address refused: {0}")]
	Address(#[from] AddressError),
}
