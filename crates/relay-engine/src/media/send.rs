//! The per-call send path: encode, packetize, gate, seal and send.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use relay_audio::FRAME_SAMPLES;
use relay_audio::codec::{CallEncoder, EncoderConfig, MAX_PACKET_BYTES};
use relay_core::CallId;
use relay_protocol::rtp::{HEADER_LEN, RtpHeader, RtpProfile};

use super::gate::{FrameStamp, GateCell};
use super::packet::RTP_MAX;
use crate::media::MediaError;

/// Where finished RTP packets go. The production sink seals and sends on a
/// UDP endpoint; tests collect them.
pub trait PacketSink: Send {
	/// Sends one RTP packet bound to the call's wire identity.
	fn send(&mut self, call: [u8; 16], generation: u32, rtp: &[u8]) -> Result<(), MediaError>;
}

/// Counters of one call's send path, readable from any thread.
#[derive(Debug, Default)]
pub struct SendStats {
	/// Packets handed to the sink.
	pub sent: AtomicU64,
	/// Frames dropped because the gate was closed.
	pub gate_closed: AtomicU64,
	/// Frames dropped because their capture or route generation was stale.
	pub stale: AtomicU64,
	/// Encode or send failures.
	pub failed: AtomicU64,
}

/// Why a frame produced (or did not produce) a packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Emit {
	/// A packet was sent.
	Sent,
	/// The gate is closed.
	Closed,
	/// The frame's generations no longer match the latest gate update.
	Stale,
	/// Encoding or sending failed.
	Failed,
}

/// Identity of one outgoing stream.
#[derive(Debug, Clone, Copy)]
pub struct StreamIdentity {
	/// The call this stream belongs to. Never changes.
	pub call: CallId,
	/// RTP synchronization source.
	pub ssrc: u32,
	/// Stream generation bound into every datagram.
	pub generation: u32,
	/// Random offset added to the RTP timestamp.
	pub timestamp_base: u32,
}

/// One call's encoder, RTP state and gate. Owned by the capture worker.
pub struct CallSender {
	identity: StreamIdentity,
	gate: Arc<GateCell>,
	encoder: CallEncoder,
	sink: Box<dyn PacketSink>,
	stats: Arc<SendStats>,
	sequence: u16,
	in_spurt: bool,
	opus: [u8; MAX_PACKET_BYTES],
	rtp: [u8; RTP_MAX],
}

impl CallSender {
	/// Builds the sender; allocates only here, never per frame.
	pub fn new(
		identity: StreamIdentity,
		gate: Arc<GateCell>,
		encoder: EncoderConfig,
		sink: Box<dyn PacketSink>,
	) -> Result<Self, relay_audio::AudioError> {
		Ok(Self {
			identity,
			gate,
			encoder: CallEncoder::new(encoder)?,
			sink,
			stats: Arc::new(SendStats::default()),
			sequence: 0,
			in_spurt: false,
			opus: [0; MAX_PACKET_BYTES],
			rtp: [0; RTP_MAX],
		})
	}

	/// The call this sender serves.
	pub fn call(&self) -> CallId {
		self.identity.call
	}

	/// Shared counters.
	pub fn stats(&self) -> Arc<SendStats> {
		Arc::clone(&self.stats)
	}

	/// The stamp for frames read from the capture path right now.
	pub fn stamp(&self, capture: relay_core::AudioGeneration) -> FrameStamp {
		FrameStamp {
			capture,
			route: self.gate.route(),
		}
	}

	/// Encodes and sends one 10 ms frame, or drops it.
	///
	/// The gate is consulted twice: before encoding, so a closed gate costs no
	/// work and the encoder sees no audio it will not send, and again
	/// immediately before the packet is sealed, so an update that landed
	/// during encoding still wins.
	pub fn emit(&mut self, frame: &[f32], stamp: FrameStamp, first_sample: u64) -> Emit {
		if let Some(refusal) = self.refuse(stamp) {
			return refusal;
		}
		let marker = !self.in_spurt;
		if marker {
			// A new talk spurt starts from clean codec state, not from audio
			// captured before the gate closed.
			let _ = self.encoder.reset();
		}
		let Ok(len) = self.encoder.encode(&frame[..FRAME_SAMPLES], &mut self.opus) else {
			return self.fail();
		};
		if let Some(refusal) = self.refuse(stamp) {
			return refusal;
		}
		let header = RtpHeader {
			marker,
			payload_type: RtpProfile::V1.payload_type,
			sequence: self.sequence,
			timestamp: (first_sample as u32).wrapping_add(self.identity.timestamp_base),
			ssrc: self.identity.ssrc,
		};
		let Ok(total) = RtpProfile::V1.write(&header, &self.opus[..len], &mut self.rtp) else {
			return self.fail();
		};
		debug_assert!(total >= HEADER_LEN);
		let sent = self.sink.send(
			self.identity.call.to_bytes(),
			self.identity.generation,
			&self.rtp[..total],
		);
		if sent.is_err() {
			return self.fail();
		}
		self.sequence = self.sequence.wrapping_add(1);
		self.in_spurt = true;
		self.stats.sent.fetch_add(1, Ordering::Relaxed);
		Emit::Sent
	}

	fn refuse(&mut self, stamp: FrameStamp) -> Option<Emit> {
		let gate = self.gate.load();
		let (counter, emit) = if !gate.open {
			(&self.stats.gate_closed, Emit::Closed)
		} else if gate.route != stamp.route || gate.capture != stamp.capture {
			(&self.stats.stale, Emit::Stale)
		} else {
			return None;
		};
		counter.fetch_add(1, Ordering::Relaxed);
		self.in_spurt = false;
		Some(emit)
	}

	fn fail(&mut self) -> Emit {
		self.stats.failed.fetch_add(1, Ordering::Relaxed);
		self.in_spurt = false;
		Emit::Failed
	}
}
