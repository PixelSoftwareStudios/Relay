//! Decoding of an uploaded audio file to mono samples at the file's own rate.

use std::io::Cursor;

use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;

use super::RingtoneError;

/// Mono samples and the rate they were decoded at.
pub(super) struct Decoded {
	pub samples: Vec<f32>,
	pub rate: u32,
}

/// Decodes `bytes` (any supported container), averaging channels to mono. Refuses more than
/// `max_seconds` of audio as soon as that many samples were produced, so a small file that
/// expands a lot never fills memory.
pub(super) fn decode(bytes: &[u8], max_seconds: u32) -> Result<Decoded, RingtoneError> {
	let source = MediaSourceStream::new(Box::new(Cursor::new(bytes.to_vec())), Default::default());
	let probed = symphonia::default::get_probe()
		.format(
			&Hint::new(),
			source,
			&FormatOptions::default(),
			&MetadataOptions::default(),
		)
		.map_err(|_| RingtoneError::Unsupported)?;
	let mut format = probed.format;
	let track = format
		.tracks()
		.iter()
		.find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
		.ok_or(RingtoneError::Unsupported)?;
	let id = track.id;
	let rate = track
		.codec_params
		.sample_rate
		.ok_or(RingtoneError::Unsupported)?;
	if !(8_000..=192_000).contains(&rate) {
		return Err(RingtoneError::Unsupported);
	}
	let limit = u64::from(max_seconds) * u64::from(rate);
	if track.codec_params.n_frames.is_some_and(|n| n > limit) {
		return Err(RingtoneError::TooLong { max_seconds });
	}
	let mut decoder = symphonia::default::get_codecs()
		.make(&track.codec_params, &DecoderOptions::default())
		.map_err(|_| RingtoneError::Unsupported)?;
	let mut samples: Vec<f32> = Vec::new();
	let mut scratch: Option<SampleBuffer<f32>> = None;
	loop {
		let packet = match format.next_packet() {
			Ok(packet) => packet,
			Err(SymphoniaError::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => {
				break;
			}
			Err(SymphoniaError::ResetRequired) => break,
			Err(e) => return Err(RingtoneError::Corrupt(e.to_string())),
		};
		if packet.track_id() != id {
			continue;
		}
		let decoded = match decoder.decode(&packet) {
			Ok(decoded) => decoded,
			// A damaged packet is skipped; a file that is all damage ends up empty below.
			Err(SymphoniaError::DecodeError(_)) => continue,
			Err(e) => return Err(RingtoneError::Corrupt(e.to_string())),
		};
		let spec = *decoded.spec();
		let channels = spec.channels.count().max(1);
		let buffer = scratch
			.get_or_insert_with(|| SampleBuffer::<f32>::new(decoded.capacity() as u64, spec));
		if buffer.capacity() < decoded.capacity() * channels {
			*buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
		}
		buffer.copy_interleaved_ref(decoded);
		for frame in buffer.samples().chunks_exact(channels) {
			samples.push(frame.iter().sum::<f32>() / channels as f32);
		}
		if samples.len() as u64 > limit {
			return Err(RingtoneError::TooLong { max_seconds });
		}
	}
	if samples.is_empty() {
		return Err(RingtoneError::Corrupt(
			"no audio could be decoded".to_owned(),
		));
	}
	Ok(Decoded { samples, rate })
}
