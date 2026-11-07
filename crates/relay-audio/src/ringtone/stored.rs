//! The stored form of a processed ringtone: mono 48 kHz Ogg Opus (RFC 7845), music tuning at
//! 64 kbit/s. About 8 KB per second, so a 30 s ring is under 250 KB, against 2.9 MB of PCM.
//!
//! The pre-skip written in `OpusHead` is the encoder's look-ahead, and the final granule
//! position holds the exact sample count, so decoding returns exactly the samples that were
//! encoded (the last packet's padding is trimmed) and the loop length is preserved.

use std::io::Cursor;

use ogg::{PacketReader, PacketWriteEndInfo, PacketWriter};
use opus::{Application, Bitrate, Channels, Decoder, Encoder};

use crate::format::INTERNAL_RATE;

/// Samples per Opus packet (20 ms).
const FRAME: usize = 960;
/// Bitrate of the stored ring.
const BITRATE: i32 = 64_000;
/// Largest packet the encoder may produce.
const MAX_PACKET: usize = 4000;
/// Ogg stream serial (one stream per file, so any constant will do).
const SERIAL: u32 = 0x52_4c_59_52;
/// First samples after a decode are eased in over this many samples (1 ms), so a codec
/// transient can never click at the loop seam.
const EASE_IN: usize = 48;

fn tags() -> Vec<u8> {
	let vendor = b"relay";
	let mut out = b"OpusTags".to_vec();
	out.extend_from_slice(&(vendor.len() as u32).to_le_bytes());
	out.extend_from_slice(vendor);
	out.extend_from_slice(&0u32.to_le_bytes());
	out
}

/// Encodes `samples` (mono, 48 kHz). `None` only if the encoder cannot be created.
pub fn encode(samples: &[f32]) -> Option<Vec<u8>> {
	let mut encoder = Encoder::new(INTERNAL_RATE, Channels::Mono, Application::Audio).ok()?;
	encoder.set_bitrate(Bitrate::Bits(BITRATE)).ok()?;
	let pre_skip = u16::try_from(encoder.get_lookahead().ok()?).ok()?;
	let mut head = b"OpusHead".to_vec();
	head.extend_from_slice(&[1, 1]);
	head.extend_from_slice(&pre_skip.to_le_bytes());
	head.extend_from_slice(&INTERNAL_RATE.to_le_bytes());
	head.extend_from_slice(&0i16.to_le_bytes());
	head.push(0);

	let mut file = Vec::new();
	{
		let mut writer = PacketWriter::new(&mut file);
		writer
			.write_packet(head, SERIAL, PacketWriteEndInfo::EndPage, 0)
			.ok()?;
		writer
			.write_packet(tags(), SERIAL, PacketWriteEndInfo::EndPage, 0)
			.ok()?;
		// The look-ahead worth of silence flushes the encoder; the granule position of the
		// last packet then says exactly where the real audio ends.
		let total = samples.len() + usize::from(pre_skip);
		let frames = total.div_ceil(FRAME);
		let mut frame = [0.0_f32; FRAME];
		let mut packet = [0u8; MAX_PACKET];
		for index in 0..frames {
			frame.fill(0.0);
			// Input is delayed by nothing: the first `pre_skip` output samples are skipped by
			// the decoder, so audio is fed from sample 0 and the tail is zero padding.
			let from = index * FRAME;
			let have = samples.len().saturating_sub(from).min(FRAME);
			frame[..have].copy_from_slice(&samples[from..from + have]);
			let n = encoder.encode_float(&frame, &mut packet).ok()?;
			let last = index + 1 == frames;
			let granule = if last {
				total as u64
			} else {
				((index + 1) * FRAME) as u64
			};
			let end = if last {
				PacketWriteEndInfo::EndStream
			} else {
				PacketWriteEndInfo::NormalPacket
			};
			writer
				.write_packet(packet[..n].to_vec(), SERIAL, end, granule)
				.ok()?;
		}
	}
	Some(file)
}

/// Decodes [`encode`] output; `None` when it is not a mono 48 kHz Ogg Opus file of this kind.
pub fn decode(bytes: &[u8]) -> Option<Vec<f32>> {
	let mut reader = PacketReader::new(Cursor::new(bytes));
	let head = reader.read_packet().ok()??;
	if head.data.len() < 19 || &head.data[..8] != b"OpusHead" || head.data[9] != 1 {
		return None;
	}
	let pre_skip = usize::from(u16::from_le_bytes([head.data[10], head.data[11]]));
	let tags = reader.read_packet().ok()??;
	if !tags.data.starts_with(b"OpusTags") {
		return None;
	}
	let mut decoder = Decoder::new(INTERNAL_RATE, Channels::Mono).ok()?;
	let mut out: Vec<f32> = Vec::new();
	let mut frame = [0.0_f32; FRAME];
	let mut end = None;
	while let Some(packet) = reader.read_packet().ok()? {
		let n = decoder.decode_float(&packet.data, &mut frame, false).ok()?;
		out.extend_from_slice(&frame[..n]);
		if packet.last_in_stream() {
			end = Some(packet.absgp_page());
		}
	}
	let total = usize::try_from(end?).ok()?;
	out.truncate(total);
	if out.len() <= pre_skip {
		return None;
	}
	out.drain(..pre_skip);
	for (i, s) in out.iter_mut().take(EASE_IN).enumerate() {
		*s *= i as f32 / EASE_IN as f32;
	}
	Some(out)
}
