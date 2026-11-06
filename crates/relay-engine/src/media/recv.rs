//! The per-call receive path: validate, schedule, decode and conceal.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};

use relay_audio::FRAME_SAMPLES;
use relay_audio::codec::CallDecoder;
use relay_core::CallId;
use relay_protocol::jitter::{JitterBuffer, JitterConfig, PacketMeta, Playout};
use relay_protocol::rtp::RtpProfile;

use super::diag::now_ms;
use super::meters::{Level, SPEAKING_DBFS, SPEAKING_HOLD_FRAMES, level_dbfs};
use super::packet::{Ingress, PacketBuf};

/// Identifies the network task a packet came from. A receiver accepts packets
/// only from the source its peer's endpoint was registered under.
pub type SourceId = u64;

/// Counters of one call's receive path, readable from any thread.
#[derive(Debug, Default)]
pub struct RecvStats {
	/// Level of the latest rendered frame.
	pub level: Level,
	/// The remote voice is audibly active (with a short hold).
	pub speaking: AtomicBool,
	/// Packets stored in the jitter buffer.
	pub accepted: AtomicU64,
	/// Packets rejected for a wrong source, call, generation or SSRC.
	pub misbound: AtomicU64,
	/// Packets that failed RTP parsing.
	pub malformed: AtomicU64,
	/// Packets the jitter buffer refused (late, duplicate, too far).
	pub refused: AtomicU64,
	/// Frames rendered from decoded audio.
	pub voice: AtomicU64,
	/// Frames rendered by concealment.
	pub concealed: AtomicU64,
	/// Frames rendered as silence.
	pub silent: AtomicU64,
	/// Packets the sender's sequence numbers say it sent, from the first one seen to the
	/// highest (wrap-extended). Together with the packets received it gives the loss.
	pub expected: AtomicU64,
	/// [`now_ms`] when the last bound packet arrived; 0 before the first.
	pub last_packet_ms: AtomicU64,
	/// RFC 3550 interarrival jitter in microseconds.
	pub jitter_us: AtomicU64,
	/// Audio queued in the jitter buffer, in samples at 48 kHz.
	pub buffered_samples: AtomicU32,
	/// Playout margin the controller wants, in 10 ms frames.
	pub target_margin_frames: AtomicU32,
}

/// What one render tick produced for a call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rendered {
	/// Decoded audio.
	Voice,
	/// Concealed audio.
	Concealed,
	/// Silence; nothing to mix.
	Silence,
}

/// Negotiated identity of the remote stream.
#[derive(Debug, Clone, Copy)]
pub struct RemoteStream {
	/// The call.
	pub call: CallId,
	/// SSRC the remote announced in `MediaReady`.
	pub ssrc: u32,
	/// Stream generation the remote announced.
	pub generation: u32,
	/// The network task this stream arrives through.
	pub source: SourceId,
}

/// One call's jitter buffer and decoder. Owned by the render worker.
pub struct CallReceiver {
	remote: RemoteStream,
	jitter: JitterBuffer<PacketBuf>,
	decoder: CallDecoder,
	stats: Arc<RecvStats>,
	needs_reset: bool,
	hold: u8,
	/// First sequence number seen and the wrap-extended highest.
	seq: Option<(u64, u64)>,
}

impl CallReceiver {
	/// Builds the receiver; allocates only here, never per packet.
	pub fn new(
		remote: RemoteStream,
		jitter: JitterConfig,
	) -> Result<Self, relay_audio::AudioError> {
		Ok(Self {
			remote,
			jitter: JitterBuffer::new(jitter),
			decoder: CallDecoder::new()?,
			stats: Arc::new(RecvStats::default()),
			needs_reset: false,
			hold: 0,
			seq: None,
		})
	}

	/// The call this receiver serves.
	pub fn call(&self) -> CallId {
		self.remote.call
	}

	/// Shared counters.
	pub fn stats(&self) -> Arc<RecvStats> {
		Arc::clone(&self.stats)
	}

	/// Playout margin currently applied, in 10 ms frames.
	pub fn margin_frames(&self) -> u32 {
		self.jitter.margin_frames()
	}

	/// Offers one authenticated packet from `source`.
	///
	/// The datagram's sealed call id and generation, the RTP SSRC and the
	/// source must all match this call's negotiated stream; anything else is
	/// counted and dropped, so a packet can never be scheduled into a
	/// different call.
	pub fn ingest(&mut self, source: SourceId, packet: &Ingress) {
		let bound = source == self.remote.source
			&& packet.call == self.remote.call.to_bytes()
			&& packet.generation == self.remote.generation;
		if !bound {
			self.stats.misbound.fetch_add(1, Ordering::Relaxed);
			return;
		}
		let Ok(rtp) = RtpProfile::V1.parse(packet.rtp()) else {
			self.stats.malformed.fetch_add(1, Ordering::Relaxed);
			return;
		};
		if rtp.header.ssrc != self.remote.ssrc {
			self.stats.misbound.fetch_add(1, Ordering::Relaxed);
			return;
		}
		let Some(payload) = PacketBuf::from_payload(rtp.payload) else {
			self.stats.malformed.fetch_add(1, Ordering::Relaxed);
			return;
		};
		let meta = PacketMeta {
			sequence: rtp.header.sequence,
			timestamp: rtp.header.timestamp,
			arrival_us: packet.arrival_us,
		};
		self.note_arrival(rtp.header.sequence);
		let counter = match self.jitter.insert(meta, payload) {
			relay_protocol::jitter::InsertOutcome::Stored => &self.stats.accepted,
			_ => &self.stats.refused,
		};
		if counter.fetch_add(1, Ordering::Relaxed) == 0
			&& self.stats.accepted.load(Ordering::Relaxed)
				+ self.stats.refused.load(Ordering::Relaxed)
				== 1
		{
			tracing::info!(call = %self.remote.call, "first media packet received");
		}
		let jitter_us = u64::from(self.jitter.jitter_samples()) * 1000 / 48;
		self.stats.jitter_us.store(jitter_us, Ordering::Relaxed);
	}

	/// Tracks the sequence range (for loss) and the arrival time (for "receiving").
	fn note_arrival(&mut self, sequence: u16) {
		let (first, high) = self
			.seq
			.get_or_insert((u64::from(sequence), u64::from(sequence)));
		// Distance forward in 16-bit space; a small backwards step is a reordered packet.
		let step = sequence.wrapping_sub((*high & 0xffff) as u16) as i16;
		if step > 0 {
			*high += u64::from(step.unsigned_abs());
		}
		let expected = *high - *first + 1;
		self.stats.expected.store(expected, Ordering::Relaxed);
		self.stats
			.last_packet_ms
			.store(now_ms().max(1), Ordering::Relaxed);
	}

	/// Produces exactly one 10 ms frame into `out`.
	pub fn render(&mut self, out: &mut [f32; FRAME_SAMPLES]) -> Rendered {
		self.stats
			.buffered_samples
			.store(self.jitter.occupancy_samples(), Ordering::Relaxed);
		self.stats
			.target_margin_frames
			.store(self.jitter.target_margin_frames(), Ordering::Relaxed);
		match self.jitter.poll() {
			Playout::Play(played) => {
				if std::mem::take(&mut self.needs_reset) {
					let _ = self.decoder.reset();
				}
				if self.decoder.decode(played.payload.payload(), out).is_ok() {
					return self.note(Rendered::Voice, out);
				}
				self.conceal(out, false)
			}
			Playout::Conceal(c) => self.conceal(out, c.fade),
			Playout::Skip(_) => {
				self.needs_reset = true;
				self.note(Rendered::Silence, out)
			}
		}
	}

	fn conceal(&mut self, out: &mut [f32; FRAME_SAMPLES], fade: bool) -> Rendered {
		if self.decoder.conceal(out).is_err() {
			return self.note(Rendered::Silence, out);
		}
		if fade {
			let step = 1.0 / FRAME_SAMPLES as f32;
			for (i, sample) in out.iter_mut().enumerate() {
				*sample *= 1.0 - step * (i + 1) as f32;
			}
		}
		self.note(Rendered::Concealed, out)
	}

	fn note(&mut self, rendered: Rendered, out: &mut [f32; FRAME_SAMPLES]) -> Rendered {
		let counter = match rendered {
			Rendered::Voice => &self.stats.voice,
			Rendered::Concealed => &self.stats.concealed,
			Rendered::Silence => {
				out.fill(0.0);
				&self.stats.silent
			}
		};
		counter.fetch_add(1, Ordering::Relaxed);
		self.measure(rendered, out);
		rendered
	}

	/// Publishes the frame's level and whether the remote is speaking.
	fn measure(&mut self, rendered: Rendered, out: &[f32; FRAME_SAMPLES]) {
		let level = level_dbfs(out);
		self.stats.level.set(level);
		if rendered == Rendered::Voice && level > SPEAKING_DBFS {
			self.hold = SPEAKING_HOLD_FRAMES;
		} else {
			self.hold = self.hold.saturating_sub(1);
		}
		self.stats.speaking.store(self.hold > 0, Ordering::Relaxed);
	}
}
