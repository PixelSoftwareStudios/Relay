//! Levels and shape of the built-in sounds, and the custom ring slot of the tone player.

use std::sync::Arc;

use relay_audio::tones::{Sound, ToneControl, TonePlayer};

/// Ring volume the engine ships with (`audio.ringtone_volume`) and the fixed ringback volume.
const DEFAULT_RING_VOLUME: f32 = 0.4;
const RINGBACK_VOLUME: f32 = 0.5;

fn render(player: &mut TonePlayer, samples: usize) -> Vec<f32> {
	let mut out = vec![0.0; samples];
	for chunk in out.chunks_mut(480) {
		player.render_into(chunk);
	}
	out
}

fn peak(x: &[f32]) -> f32 {
	x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

fn rms(x: &[f32]) -> f32 {
	(x.iter().map(|s| s * s).sum::<f32>() / x.len() as f32).sqrt()
}

fn dbfs(linear: f32) -> f32 {
	20.0 * linear.log10()
}

fn played(sound: Sound, looping: bool, volume: f32, samples: usize) -> Vec<f32> {
	let (mut p, mut c) = TonePlayer::new();
	c.start(sound, looping, volume).unwrap();
	render(&mut p, samples)
}

#[test]
fn default_ring_peaks_near_minus_twenty_dbfs_and_never_clips() {
	let out = played(Sound::Ring, true, DEFAULT_RING_VOLUME, 3 * 48_000);
	let level = dbfs(peak(&out));
	assert!((-22.0..=-18.0).contains(&level), "ring peak {level} dBFS");
	let loudest = dbfs(peak(&played(Sound::Ring, true, 1.0, 3 * 48_000)));
	assert!(
		loudest <= -11.5,
		"the ring at full volume peaks at {loudest} dBFS"
	);
}

#[test]
fn default_ring_is_well_below_speech_in_a_call() {
	// Speech in a call sits around -20 dBFS RMS; the ring's loudest second stays far under it.
	let out = played(Sound::Ring, true, DEFAULT_RING_VOLUME, 4 * 48_000);
	let loudest_second = out.chunks(48_000).map(rms).fold(0.0, f32::max);
	let level = dbfs(loudest_second);
	assert!(level <= -28.0, "ring rms {level} dBFS");
}

#[test]
fn ringback_and_blips_are_as_gentle_as_the_ring() {
	let back = dbfs(peak(&played(
		Sound::Ringback,
		true,
		RINGBACK_VOLUME,
		4 * 48_000,
	)));
	assert!((-20.0..=-16.0).contains(&back), "ringback peak {back} dBFS");
	for blip in [Sound::CallEnd, Sound::Connected] {
		let samples = blip.period_samples() + 480;
		let level = dbfs(peak(&played(blip, false, DEFAULT_RING_VOLUME, samples)));
		assert!(level <= -18.0, "{blip:?} peak {level} dBFS");
	}
}

#[test]
fn builtin_sounds_have_no_hard_edges_or_high_partials() {
	for sound in Sound::ALL {
		let looping = matches!(sound, Sound::Ring | Sound::Ringback);
		let out = played(sound, looping, 1.0, 2 * sound.period_samples());
		// A sine at 600 Hz and amplitude 0.25 moves 0.0079 per sample at most; the chime peaks at 0.0215 with its partials
		// for the second partial. A click or a square edge would be orders above this.
		let steepest = out
			.windows(2)
			.map(|w| (w[1] - w[0]).abs())
			.fold(0.0, f32::max);
		assert!(steepest < 0.03, "{sound:?} steepest step {steepest}");
	}
}

#[test]
fn ring_loops_seamlessly_after_a_long_pause() {
	let period = Sound::Ring.period_samples();
	let out = played(Sound::Ring, true, 1.0, 3 * period);
	assert_eq!(out[period - 1], 0.0);
	assert_eq!(out[period], 0.0, "the next cycle starts from silence");
	assert_eq!(out[period..2 * period], out[2 * period..3 * period]);
	let silent = out[..period]
		.iter()
		.rev()
		.take_while(|s| **s == 0.0)
		.count();
	assert!(silent >= 2 * 48_000, "pause of {silent} samples");
}

fn custom_ring() -> Arc<[f32]> {
	let mut ring: Vec<f32> = (0..48_000).map(|i| 0.2 * (i as f32 * 0.05).sin()).collect();
	ring.resize(60_000, 0.0);
	ring.into()
}

#[test]
fn custom_ring_replaces_the_chime_and_loops_at_its_own_length() {
	let (mut p, mut c) = TonePlayer::new();
	let ring = custom_ring();
	c.set_custom_ring(Some(ring.clone())).unwrap();
	c.start(Sound::Ring, true, 1.0).unwrap();
	let out = render(&mut p, 3 * ring.len());
	// After the 10 ms fade-in the output is the custom samples.
	assert!((out[5_000] - ring[5_000]).abs() < 1e-6);
	assert_eq!(
		out[ring.len()..2 * ring.len()],
		out[2 * ring.len()..3 * ring.len()]
	);
	// Other sounds are unaffected.
	let (mut p2, mut c2) = TonePlayer::new();
	c2.set_custom_ring(Some(ring)).unwrap();
	c2.start(Sound::Ringback, true, 1.0).unwrap();
	let reference = played(Sound::Ringback, true, 1.0, 48_000);
	assert_eq!(render(&mut p2, 48_000), reference);
}

#[test]
fn restoring_the_default_brings_the_chime_back_and_hands_the_old_buffer_back() {
	let (mut p, mut c) = TonePlayer::new();
	let ring = custom_ring();
	c.set_custom_ring(Some(ring.clone())).unwrap();
	p.poll_commands();
	assert_eq!(Arc::strong_count(&ring), 2);
	c.set_custom_ring(None).unwrap();
	p.poll_commands();
	// The player let go of it on the control side's queue, not by freeing.
	assert_eq!(Arc::strong_count(&ring), 2);
	c.set_custom_ring(None).unwrap();
	assert_eq!(
		Arc::strong_count(&ring),
		1,
		"returned buffer is dropped by the control side"
	);
	c.start(Sound::Ring, true, 1.0).unwrap();
	let chime = played(Sound::Ring, true, 1.0, 48_000);
	assert_eq!(render(&mut p, 48_000), chime);
}

#[test]
fn custom_ring_queue_is_bounded() {
	let (_p, mut c): (TonePlayer, ToneControl) = TonePlayer::new();
	let mut refused = false;
	for _ in 0..16 {
		refused |= c.set_custom_ring(None).is_err();
	}
	assert!(refused);
}
