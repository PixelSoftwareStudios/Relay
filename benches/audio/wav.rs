//! Mono 48 kHz WAV reading and writing.

use std::path::Path;

use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use relay_audio::INTERNAL_RATE;
use relay_audio::convert::StreamResampler;

use super::{BenchError, Result};

fn wav_err(e: hound::Error) -> BenchError {
	BenchError::Wav(e.to_string())
}

/// Read the first channel as `f32` at the file's own rate.
pub fn read_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
	if path.extension().and_then(|e| e.to_str()) == Some("flac") {
		return read_flac_mono(path);
	}
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
	Ok((
		samples.iter().step_by(channels).copied().collect(),
		spec.sample_rate,
	))
}

fn read_flac_mono(path: &Path) -> Result<(Vec<f32>, u32)> {
	let mut reader = claxon::FlacReader::open(path).map_err(|e| BenchError::Wav(e.to_string()))?;
	let info = reader.streaminfo();
	let channels = info.channels.max(1) as usize;
	let scale = 1.0 / (1_i64 << (info.bits_per_sample - 1)) as f32;
	let mut mono = Vec::with_capacity(info.samples.unwrap_or(0) as usize);
	for (i, sample) in reader.samples().enumerate() {
		let value = sample.map_err(|e| BenchError::Wav(e.to_string()))?;
		if i % channels == 0 {
			mono.push(value as f32 * scale);
		}
	}
	Ok((mono, info.sample_rate))
}

/// Convert to the 48 kHz internal rate with the band-limited resampler.
pub fn resample_to_48k(samples: &[f32], rate: u32) -> Result<Vec<f32>> {
	if rate == INTERNAL_RATE {
		return Ok(samples.to_vec());
	}
	let chunk = (rate / 100).max(1) as usize;
	let mut resampler = StreamResampler::new_fixed(rate, INTERNAL_RATE, chunk)
		.map_err(|e| BenchError::Wav(e.to_string()))?;
	let mut out = Vec::with_capacity(samples.len() * 48_000 / rate as usize + 480);
	resampler
		.push(samples, |b| out.extend_from_slice(b))
		.map_err(|e| BenchError::Wav(e.to_string()))?;
	Ok(out)
}

/// Read WAV or FLAC, first channel, resampled to 48 kHz.
pub fn read_mono_48k(path: &Path) -> Result<Vec<f32>> {
	let (samples, rate) = read_mono(path)?;
	resample_to_48k(&samples, rate)
}

/// Streaminfo duration in seconds for a FLAC file, without decoding.
pub fn flac_duration_s(path: &Path) -> Result<f64> {
	let reader = claxon::FlacReader::open(path).map_err(|e| BenchError::Wav(e.to_string()))?;
	let info = reader.streaminfo();
	Ok(info.samples.unwrap_or(0) as f64 / f64::from(info.sample_rate))
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
