//! Mono 48 kHz WAV reading and writing.

use std::path::Path;

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use relay_audio::INTERNAL_RATE;
use relay_audio::convert::StreamResampler;

use super::{BenchError, Result};

fn wav_err(e: hound::Error) -> BenchError {
	BenchError::Wav(e.to_string())
}

/// Read the first channel as `f32`, resampled to 48 kHz when needed.
pub fn read_mono_48k(path: &Path) -> Result<Vec<f32>> {
	let mut reader = WavReader::open(path).map_err(wav_err)?;
	let spec = reader.spec();
	let channels = usize::from(spec.channels.max(1));
	let samples: Vec<f32> = match (spec.sample_format, spec.bits_per_sample) {
		(SampleFormat::Float, 32) => reader
			.samples::<f32>()
			.collect::<std::result::Result<_, _>>()
			.map_err(wav_err)?,
		(SampleFormat::Int, bits @ 8..=32) => {
			let scale = 1.0 / (1_i64 << (bits - 1)) as f32;
			reader
				.samples::<i32>()
				.map(|s| s.map(|v| v as f32 * scale))
				.collect::<std::result::Result<_, _>>()
				.map_err(wav_err)?
		}
		other => return Err(BenchError::Wav(format!("unsupported format {other:?}"))),
	};
	let mono: Vec<f32> = samples.iter().step_by(channels).copied().collect();
	if spec.sample_rate == INTERNAL_RATE {
		return Ok(mono);
	}
	let chunk = (spec.sample_rate / 100).max(1) as usize;
	let mut resampler = StreamResampler::new(spec.sample_rate, INTERNAL_RATE, chunk)
		.map_err(|e| BenchError::Wav(e.to_string()))?;
	let mut out = Vec::with_capacity(mono.len() * 48_000 / spec.sample_rate as usize + 480);
	resampler
		.push(&mono, |b| out.extend_from_slice(b))
		.map_err(|e| BenchError::Wav(e.to_string()))?;
	Ok(out)
}

/// Write 16-bit mono PCM at 48 kHz, saturating.
pub fn write_mono_48k(path: &Path, samples: &[f32]) -> Result<()> {
	let spec = WavSpec {
		channels: 1,
		sample_rate: INTERNAL_RATE,
		bits_per_sample: 16,
		sample_format: SampleFormat::Int,
	};
	let mut writer = WavWriter::create(path, spec).map_err(wav_err)?;
	for &s in samples {
		let v = (s.clamp(-1.0, 1.0) * 32_767.0).round() as i16;
		writer.write_sample(v).map_err(wav_err)?;
	}
	writer.finalize().map_err(wav_err)
}
