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
	/// Frames dropped because their audio may predate the gate opening.
	pub unarmed: AtomicU64,
	/// Packets dropped because the socket buffer was full.
	pub send_dropped: AtomicU64,
	/// Encode or send failures.
	pub failed: AtomicU64,
	/// [`super::diag::now_ms`] when the last packet was sent; 0 before the first.
	pub last_sent_ms: AtomicU64,
	/// Size of the last sealed datagram in bytes.
	pub packet_bytes: AtomicU64,
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
	/// The frame holds audio read before the gate had been open for a full poll.
	Unarmed,
	/// The socket buffer was full; the packet was dropped.
	Dropped,
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
	/// The gate was open at the previous poll.
	prev_open: bool,
	/// Capture-timeline index below which audio may predate the gate opening.
	taint_until: u64,
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
			prev_open: false,
			taint_until: 0,
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
	///
	/// Call once at the start of every poll. `stalled` is true when the worker
	/// was late, so the poll may drain a backlog whose age is unknown. A frame
	/// is armed only when the gate is open now *and* was open at the previous
	/// poll: audio captured between the two polls may precede the gate opening.
	pub fn stamp(&mut self, capture: relay_core::AudioGeneration, stalled: bool) -> FrameStamp {
		let gate = self.gate.load();
		let clean = gate.open && self.prev_open && !stalled;
		self.prev_open = gate.open;
		FrameStamp {
			capture,
			route: gate.route,
			clean,
		}
	}

	/// Ends a poll. `read_end` is the capture-timeline index just past the audio
	/// the poll drained and `delay` the conversion and suppression group delay:
	/// when the poll was not clean, every frame still carrying that audio is
	/// refused, even if it comes out of a later poll.
	pub fn end_poll(&mut self, stamp: FrameStamp, read_end: u64, delay: u64) {
		if !stamp.clean {
			self.taint_until = self.taint_until.max(read_end.saturating_add(delay));
		}
	}

	/// Forgets the timeline and the previous gate observation (new stream).
	pub fn rearm(&mut self) {
		self.prev_open = false;
		self.taint_until = 0;
		self.in_spurt = false;
	}

	/// Forgets the timeline only: the capture index space was restarted.
	pub fn reset_timeline(&mut self) {
		self.taint_until = 0;
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
		if !stamp.clean || first_sample < self.taint_until {
			self.stats.unarmed.fetch_add(1, Ordering::Relaxed);
			self.in_spurt = false;
			return Emit::Unarmed;
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
		match sent {
			Ok(()) => {}
			Err(MediaError::WouldBlock) => {
				// Consume the sequence number: the receiver sees a loss, not a
				// timeline that silently shifted.
				self.sequence = self.sequence.wrapping_add(1);
				self.stats.send_dropped.fetch_add(1, Ordering::Relaxed);
				return Emit::Dropped;
			}
			Err(_) => return self.fail(),
		}
		self.sequence = self.sequence.wrapping_add(1);
		self.in_spurt = true;
		if self.stats.sent.fetch_add(1, Ordering::Relaxed) == 0 {
			tracing::info!(call = self.identity.call.short(), "first media packet sent");
		}
		self.stats
			.last_sent_ms
			.store(super::diag::now_ms().max(1), Ordering::Relaxed);
		// Call id (16) and generation (4) travel inside the sealed body next to the RTP packet.
		self.stats.packet_bytes.store(
			relay_net::record::sealed_len(20 + total) as u64,
			Ordering::Relaxed,
		);
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
