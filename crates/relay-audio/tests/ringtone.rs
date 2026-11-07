//! Decoding, levelling and limits of a user-chosen ringtone.

use std::io::Cursor;

use relay_audio::ringtone::{
	PAUSE_SAMPLES, RING_PEAK, RingtoneError, RingtoneLimits, from_stored, process, to_stored,
};

const LIMITS: RingtoneLimits = RingtoneLimits {
	max_bytes: 1024 * 1024,
	max_seconds: 30,
};

fn fixture(name: &str) -> Vec<u8> {
	std::fs::read(format!("tests/fixtures/ringtone/{name}")).unwrap()
}

fn peak(x: &[f32]) -> f32 {
	x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

fn wav(rate: u32, channels: u16, seconds: f32, amplitude: f32) -> Vec<u8> {
	let spec = hound::WavSpec {
		channels,
		sample_rate: rate,
		bits_per_sample: 16,
		sample_format: hound::SampleFormat::Int,
	};
	let mut out = Cursor::new(Vec::new());
	let mut writer = hound::WavWriter::new(&mut out, spec).unwrap();
	for i in 0..(rate as f32 * seconds) as usize {
		let v = amplitude * (i as f32 * 440.0 * std::f32::consts::TAU / rate as f32).sin();
		for _ in 0..channels {
			writer.write_sample((v * 32767.0) as i16).unwrap();
		}
	}
	writer.finalize().unwrap();
	out.into_inner()
}

/// Every supported container decodes to the ring format: mono 48 kHz, faded, levelled and followed
/// by the pause.
#[test]
fn every_supported_format_becomes_a_levelled_faded_ring() {
	for name in [
		"tone.mp3",
		"tone.ogg",
		"tone.flac",
		"tone.m4a",
		"tone.aac",
		"tone24.wav",
	] {
		let ring = process(&fixture(name), LIMITS).unwrap_or_else(|e| panic!("{name}: {e}"));
		let audible = ring.len() - PAUSE_SAMPLES;
		assert!(
			(audible as f32 / 48_000.0 - 1.2).abs() < 0.12,
			"{name}: {audible} samples"
		);
		let loudest = peak(&ring);
		assert!(
			loudest <= RING_PEAK + 1e-4 && loudest > 0.05,
			"{name}: peak {loudest}"
		);
		assert_eq!(ring[0], 0.0, "{name} starts silent");
		assert_eq!(ring[audible - 1], 0.0, "{name} ends silent");
		assert!(ring[audible..].iter().all(|s| *s == 0.0), "{name} pause");
		// No click anywhere: a 440 Hz tone at this level moves under 0.01 per sample.
		let steepest = ring
			.windows(2)
			.map(|w| (w[1] - w[0]).abs())
			.fold(0.0, f32::max);
		assert!(steepest < 0.02, "{name}: step {steepest}");
	}
}

#[test]
fn resampling_keeps_the_pitch() {
	// 440 Hz in 1.2 s: count zero crossings of the middle of the ring.
	let ring = process(&fixture("tone24.wav"), LIMITS).unwrap();
	let middle = &ring[12_000..36_000];
	let crossings = middle
		.windows(2)
		.filter(|w| w[0] < 0.0 && w[1] >= 0.0)
		.count();
	let hz = crossings as f32 / 0.5;
	assert!((hz - 440.0).abs() < 8.0, "pitch {hz} Hz");
}

#[test]
fn loud_and_quiet_files_land_on_the_same_level() {
	let loud = process(&wav(48_000, 2, 1.0, 1.0), LIMITS).unwrap();
	let quiet = process(&wav(44_100, 1, 1.0, 0.01), LIMITS).unwrap();
	// A steady sine is dense, so the RMS ceiling (not the peak target) decides: 0.07 / 0.707.
	assert!(
		(peak(&loud) - peak(&quiet)).abs() < 2e-3,
		"{} {}",
		peak(&loud),
		peak(&quiet)
	);
	assert!(peak(&loud) <= RING_PEAK);
}

#[test]
fn dense_audio_is_held_to_the_rms_ceiling() {
	// A full-scale square-ish signal would sit at the peak target with a high RMS.
	let spec = hound::WavSpec {
		channels: 1,
		sample_rate: 48_000,
		bits_per_sample: 16,
		sample_format: hound::SampleFormat::Int,
	};
	let mut out = Cursor::new(Vec::new());
	let mut writer = hound::WavWriter::new(&mut out, spec).unwrap();
	for i in 0..48_000 {
		writer
			.write_sample(if (i / 50) % 2 == 0 {
				30_000_i16
			} else {
				-30_000
			})
			.unwrap();
	}
	writer.finalize().unwrap();
	let ring = process(&out.into_inner(), LIMITS).unwrap();
	let audible = &ring[..48_000];
	let rms = (audible.iter().map(|s| s * s).sum::<f32>() / audible.len() as f32).sqrt();
	assert!(rms <= relay_audio::ringtone::RING_RMS + 5e-3, "rms {rms}");
}

#[test]
fn files_over_the_limits_are_refused_with_a_code() {
	let small = RingtoneLimits {
		max_bytes: 1000,
		max_seconds: 30,
	};
	let err = process(&fixture("tone.mp3"), small).unwrap_err();
	assert_eq!(err, RingtoneError::TooLarge { max_bytes: 1000 });
	assert_eq!(err.code(), "RINGTONE_TOO_LARGE");

	let short = RingtoneLimits {
		max_bytes: 1 << 20,
		max_seconds: 1,
	};
	let err = process(&fixture("tone.mp3"), short).unwrap_err();
	assert_eq!(err, RingtoneError::TooLong { max_seconds: 1 });

	let err = process(&wav(8_000, 1, 31.0, 0.5), LIMITS).unwrap_err();
	assert_eq!(err, RingtoneError::TooLong { max_seconds: 30 });
}

#[test]
fn bad_files_are_refused_not_played() {
	assert_eq!(process(&[], LIMITS).unwrap_err(), RingtoneError::Empty);
	assert_eq!(
		process(b"this is a text file, not audio at all", LIMITS).unwrap_err(),
		RingtoneError::Unsupported
	);
	assert_eq!(
		process(&wav(48_000, 1, 0.1, 0.5), LIMITS).unwrap_err(),
		RingtoneError::TooShort
	);
	assert_eq!(
		process(&wav(48_000, 1, 1.0, 0.0), LIMITS).unwrap_err(),
		RingtoneError::Silent
	);
	// A truncated header and random bytes after a valid magic never panic.
	let mut cut = wav(48_000, 1, 1.0, 0.5);
	cut.truncate(30);
	assert!(process(&cut, LIMITS).is_err());
	let mut noise = fixture("tone.mp3");
	for (i, b) in noise.iter_mut().enumerate().skip(40) {
		*b = (i as u8).wrapping_mul(167).wrapping_add(13);
	}
	let _ = process(&noise, LIMITS);
}

#[test]
fn the_stored_form_is_small_ogg_opus_and_round_trips_exactly_in_length() {
	let ring = process(&fixture("tone24.wav"), LIMITS).unwrap();
	let stored = to_stored(&ring).unwrap();
	assert_eq!(&stored[..4], b"OggS");
	assert!(stored.windows(8).any(|w| w == b"OpusHead"));
	let seconds = ring.len() as f32 / 48_000.0;
	assert!(
		stored.len() as f32 / seconds < 10_000.0,
		"{} bytes for {seconds} s",
		stored.len()
	);
	let back = from_stored(&stored).unwrap();
	assert_eq!(
		back.len(),
		ring.len(),
		"the loop length is kept to the sample"
	);
}

#[test]
fn the_decoded_ring_keeps_its_loop_seam_and_pitch() {
	for name in ["tone24.wav", "tone.mp3", "tone.flac"] {
		let ring = process(&fixture(name), LIMITS).unwrap();
		let back = from_stored(&to_stored(&ring).unwrap()).unwrap();
		assert!(
			back[0].abs() < 1e-4 && back[back.len() - 1].abs() < 1e-3,
			"{name}: ends"
		);
		let seam = (back[back.len() - 1] - back[0]).abs();
		assert!(seam < 1e-3, "{name}: seam {seam}");
		let steepest = back
			.windows(2)
			.map(|w| (w[1] - w[0]).abs())
			.fold(0.0, f32::max);
		assert!(steepest < 0.03, "{name}: step {steepest}");
		assert!(
			peak(&back) <= RING_PEAK * 1.2,
			"{name}: peak {}",
			peak(&back)
		);
		// The audible part still resembles the source (same energy within 1.5 dB).
		let audible = ring.len() - PAUSE_SAMPLES;
		let rms = |x: &[f32]| (x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt();
		let ratio = rms(&back[..audible]) / rms(&ring[..audible]);
		assert!((0.84..1.19).contains(&ratio), "{name}: level ratio {ratio}");
		// The pause stays a pause.
		assert!(peak(&back[audible + 4800..]) < 2e-3, "{name}: pause");
	}
}

#[test]
fn only_our_own_files_load() {
	assert!(from_stored(&[]).is_none());
	assert!(from_stored(b"RIFFnotopus").is_none());
	let ring = process(&fixture("tone24.wav"), LIMITS).unwrap();
	let mut cut = to_stored(&ring).unwrap();
	cut.truncate(cut.len() / 2);
	assert!(
		from_stored(&cut).is_none(),
		"a cut-off file has no end and is refused"
	);
}
