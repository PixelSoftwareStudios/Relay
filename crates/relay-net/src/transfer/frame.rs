//! Data-link frame bodies. Hand-rolled and tiny: a tag byte, fixed-width big-endian fields and
//! (for chunks) the data to the end of the body. Anything else is an error.
//!
//! | tag | frame | direction | body |
//! |---|---|---|---|
//! | 1 | `Chunk` | sender to receiver | offset u64, 1..=32 KiB of data |
//! | 2 | `Done` | sender to receiver | empty (all bytes sent) |
//! | 3 | `Credit` | receiver to sender | granted bytes `[0, n)` as u64 |
//! | 4 | `Result` | receiver to sender | verdict byte 0..=3 |
//! | 5 | `Cancel` | either | empty |

use relay_messages::transfer::{TransferMsg, Verdict};

use super::DataError;
use super::limits::{CHUNK_HEADER_BYTES, DATA_CHUNK_MAX_BYTES};
use super::link::TransferRole;

const CHUNK: u8 = 1;
const DONE: u8 = 2;
const CREDIT: u8 = 3;
const RESULT: u8 = 4;
const CANCEL: u8 = 5;

fn verdict_byte(v: Verdict) -> u8 {
	match v {
		Verdict::Ok => 0,
		Verdict::HashMismatch => 1,
		Verdict::SizeMismatch => 2,
		Verdict::Failed => 3,
	}
}

fn verdict_from(b: u8) -> Option<Verdict> {
	Some(match b {
		0 => Verdict::Ok,
		1 => Verdict::HashMismatch,
		2 => Verdict::SizeMismatch,
		3 => Verdict::Failed,
		_ => return None,
	})
}

/// May `role` send this message on its end of the link?
pub(super) fn may_send(role: TransferRole, msg: &TransferMsg) -> bool {
	matches!(
		(role, msg),
		(_, TransferMsg::Cancel)
			| (
				TransferRole::Sender,
				TransferMsg::Chunk { .. } | TransferMsg::Done
			) | (
			TransferRole::Receiver,
			TransferMsg::Credit(_) | TransferMsg::Result(_)
		)
	)
}

/// Encodes one frame body. Chunks must already fit [`DATA_CHUNK_MAX_BYTES`].
pub(super) fn encode(msg: &TransferMsg) -> Result<Vec<u8>, DataError> {
	Ok(match msg {
		TransferMsg::Chunk { offset, data } => {
			if data.is_empty() || data.len() > DATA_CHUNK_MAX_BYTES {
				return Err(DataError::BadFrame("chunk size out of bounds"));
			}
			let mut out = Vec::with_capacity(CHUNK_HEADER_BYTES + data.len());
			out.push(CHUNK);
			out.extend_from_slice(&offset.to_be_bytes());
			out.extend_from_slice(data);
			out
		}
		TransferMsg::Done => vec![DONE],
		TransferMsg::Credit(n) => {
			let mut out = vec![CREDIT];
			out.extend_from_slice(&n.to_be_bytes());
			out
		}
		TransferMsg::Result(v) => vec![RESULT, verdict_byte(*v)],
		TransferMsg::Cancel => vec![CANCEL],
		TransferMsg::Accept | TransferMsg::Reject => {
			return Err(DataError::BadFrame("not a data-link message"));
		}
	})
}

pub(super) fn decode(body: &[u8]) -> Result<TransferMsg, DataError> {
	let (&tag, rest) = body
		.split_first()
		.ok_or(DataError::BadFrame("empty frame"))?;
	let exact = |n: usize| {
		if rest.len() == n {
			Ok(())
		} else {
			Err(DataError::BadFrame("frame length does not match its tag"))
		}
	};
	match tag {
		CHUNK => {
			if rest.len() <= 8 || rest.len() - 8 > DATA_CHUNK_MAX_BYTES {
				return Err(DataError::BadFrame("chunk size out of bounds"));
			}
			let (offset, data) = rest.split_at(8);
			let offset = u64::from_be_bytes(offset.try_into().expect("8 bytes"));
			Ok(TransferMsg::Chunk {
				offset,
				data: data.to_vec(),
			})
		}
		DONE => exact(0).map(|()| TransferMsg::Done),
		CREDIT => {
			exact(8)?;
			Ok(TransferMsg::Credit(u64::from_be_bytes(
				rest.try_into().expect("8 bytes"),
			)))
		}
		RESULT => {
			exact(1)?;
			verdict_from(rest[0])
				.map(TransferMsg::Result)
				.ok_or(DataError::BadFrame("unknown verdict"))
		}
		CANCEL => exact(0).map(|()| TransferMsg::Cancel),
		_ => Err(DataError::BadFrame("unknown frame tag")),
	}
}
