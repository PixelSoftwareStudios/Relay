//! Synchronous receive pipeline implementing the documented order: length, header, epoch,
//! read-only replay check, authenticated decrypt, structural checks, then replay commit.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

use thiserror::Error;

use super::datagram::{CHALLENGE_BYTES, CallId, decode_audio, decode_probe};
use super::replay::{ReplayVerdict, ReplayWindow};
use crate::limits::{EPOCH_OVERLAP_MAX, EPOCH_OVERLAP_MAX_COUNT, UDP_RECORD_MAX_BYTES};
use crate::record::{Lane, RecordHeader, RecordKind, sealed_len};
use crate::session::{LinkEpoch, SecureSession};

const BOUND_STREAMS_MAX: usize = 64;

/// Why a datagram was dropped. Local diagnostics only; nothing is sent back to the source.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum RejectReason {
	#[error("datagram length out of bounds")]
	Length,
	#[error("malformed header")]
	Header,
	#[error("unknown or retired epoch")]
	Epoch,
	#[error("replayed or stale sequence")]
	Replay,
	#[error("authentication failed")]
	Authentication,
	#[error("malformed body")]
	Body,
	#[error("stream is not bound to an accepted call")]
	UnboundStream,
}

/// An authenticated, accepted media-lane record.
#[derive(Debug, PartialEq, Eq)]
pub enum Received {
	Audio {
		call_id: CallId,
		generation: u32,
		payload: Vec<u8>,
		epoch: LinkEpoch,
	},
	PathChallenge {
		challenge: [u8; CHALLENGE_BYTES],
		epoch: LinkEpoch,
	},
	PathResponse {
		challenge: [u8; CHALLENGE_BYTES],
		epoch: LinkEpoch,
	},
}

struct EpochSlot {
	session: Arc<SecureSession>,
	window: ReplayWindow,
	retire_at: Option<Instant>,
}

pub struct MediaReceiver {
	slots: Vec<EpochSlot>,
	bound: HashSet<(CallId, u32)>,
	overlap: Duration,
}

impl MediaReceiver {
	pub fn new(session: Arc<SecureSession>) -> Self {
		Self::with_overlap(session, EPOCH_OVERLAP_MAX)
	}

	/// `overlap` is clamped to the documented maximum.
	pub fn with_overlap(session: Arc<SecureSession>, overlap: Duration) -> Self {
		Self {
			slots: vec![EpochSlot {
				session,
				window: ReplayWindow::new(),
				retire_at: None,
			}],
			bound: HashSet::new(),
			overlap: overlap.min(EPOCH_OVERLAP_MAX),
		}
	}

	/// Admits `session` as the newest epoch. Older epochs stay for the overlap window and are
	/// then retired; at most two are ever held.
	pub fn install_epoch(&mut self, session: Arc<SecureSession>, now: Instant) {
		for slot in &mut self.slots {
			slot.retire_at.get_or_insert(now + self.overlap);
		}
		self.slots.push(EpochSlot {
			session,
			window: ReplayWindow::new(),
			retire_at: None,
		});
		while self.slots.len() > EPOCH_OVERLAP_MAX_COUNT {
			self.slots.remove(0);
		}
	}

	pub fn session_for(&self, epoch: &LinkEpoch) -> Option<Arc<SecureSession>> {
		self.slots
			.iter()
			.find(|s| s.session.epoch() == epoch)
			.map(|s| Arc::clone(&s.session))
	}

	pub fn epoch_count(&self) -> usize {
		self.slots.len()
	}

	/// Allows audio for `(call, generation)`. Unbound streams never reach a decoder.
	pub fn bind_stream(&mut self, call_id: CallId, generation: u32) -> bool {
		if self.bound.len() >= BOUND_STREAMS_MAX && !self.bound.contains(&(call_id, generation)) {
			return false;
		}
		self.bound.insert((call_id, generation));
		true
	}

	pub fn unbind_call(&mut self, call_id: &CallId) {
		self.bound.retain(|(id, _)| id != call_id);
	}

	pub fn receive(&mut self, datagram: &[u8], now: Instant) -> Result<Received, RejectReason> {
		if datagram.len() > UDP_RECORD_MAX_BYTES || datagram.len() < sealed_len(0) {
			return Err(RejectReason::Length);
		}
		let header = RecordHeader::parse(datagram).map_err(|_| RejectReason::Header)?;
		if header.lane() != Lane::Media {
			return Err(RejectReason::Header);
		}
		self.slots.retain(|s| s.retire_at.is_none_or(|t| now < t));
		let slot = self
			.slots
			.iter_mut()
			.find(|s| s.session.epoch() == header.epoch())
			.ok_or(RejectReason::Epoch)?;

		// Read-only: a forged counter must not move the window.
		if slot.window.check(header.sequence()) != ReplayVerdict::Fresh {
			return Err(RejectReason::Replay);
		}
		let opened = slot
			.session
			.open(&header, datagram)
			.map_err(|_| RejectReason::Authentication)?;
		let epoch = *header.epoch();
		let received = match opened.kind {
			RecordKind::Audio => {
				let (call_id, generation, payload) =
					decode_audio(&opened.body).map_err(|_| RejectReason::Body)?;
				if !self.bound.contains(&(call_id, generation)) {
					return Err(RejectReason::UnboundStream);
				}
				Received::Audio {
					call_id,
					generation,
					payload: payload.to_vec(),
					epoch,
				}
			}
			RecordKind::PathChallenge => Received::PathChallenge {
				challenge: decode_probe(&opened.body).map_err(|_| RejectReason::Body)?,
				epoch,
			},
			RecordKind::PathResponse => Received::PathResponse {
				challenge: decode_probe(&opened.body).map_err(|_| RejectReason::Body)?,
				epoch,
			},
			RecordKind::Envelope => return Err(RejectReason::Header),
		};
		// Authenticated and structurally accepted: only now commit ordering state.
		slot.window.commit(header.sequence());
		Ok(received)
	}
}
