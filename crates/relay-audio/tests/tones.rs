//! Tone player behaviour and mixer integration.

use relay_audio::mixer::Mixer;
use relay_audio::tones::{FADE_SAMPLES, Sound, ToneControl, TonePlayer, ToneState};

fn render(player: &mut TonePlayer, samples: usize) -> Vec<f32> {
	let mut out = vec![0.0; samples];
	for chunk in out.chunks_mut(480) {
		player.render_into(chunk);
	}
	out
}

fn max_delta(x: &[f32]) -> f32 {
	x.windows(2)
		.map(|w| (w[1] - w[0]).abs())
		.fold(0.0, f32::max)
}

fn peak(x: &[f32]) -> f32 {
	x.iter().fold(0.0, |m, s| m.max(s.abs()))
}

fn new() -> (TonePlayer, ToneControl) {
	TonePlayer::new()
}

#[test]
fn idle_renders_nothing() {
	let (mut p, _c) = new();
	assert_eq!(p.state(), ToneState::Idle);
	assert!(p.is_idle());
	assert!(render(&mut p, 960).iter().all(|&s| s == 0.0));
}

#[test]
fn render_is_deterministic() {
	let run = || {
		let (mut p, mut c) = new();
		c.start(Sound::Ring, true, 0.7).unwrap();
		render(&mut p, 48_000 * 3)
	};
	assert_eq!(run(), run());
}

#[test]
fn render_adds_to_existing_content() {
	let (mut p, mut c) = new();
	c.start(Sound::Ring, true, 1.0).unwrap();
	let base = render(&mut p, 4800);
	let (mut p2, mut c2) = new();
	c2.start(Sound::Ring, true, 1.0).unwrap();
	let mut out = vec![0.25; 4800];
	for chunk in out.chunks_mut(480) {
		p2.render_into(chunk);
	}
	for (o, b) in out.iter().zip(&base) {
		assert!((o - (0.25 + b)).abs() < 1e-6);
	}
}

#[test]
fn loop_has_no_discontinuity() {
	for sound in [Sound::Ring, Sound::Ringback] {
		let (mut p, mut c) = new();
		c.start(sound, true, 1.0).unwrap();
		let out = render(&mut p, sound.period_samples() * 4);
		assert!(peak(&out) > 0.2, "{sound:?} audible");
		let period = sound.period_samples();
		let inside = max_delta(&out[period..2 * period]);
		let across = max_delta(&out[period - 8..period + 8]);
		assert!(across <= inside, "{sound:?} loop point jumps: {across}");
		assert!(inside < 0.2, "{sound:?} steep: {inside}");
		assert_eq!(out[period - 1], 0.0);
		// Periodic after the start fade.
		assert_eq!(out[period..2 * period], out[2 * period..3 * period]);
	}
}

#[test]
fn start_fades_in_and_stop_fades_out() {
	let (mut p, mut c) = new();
	c.start(Sound::Ring, true, 1.0).unwrap();
	let out = render(&mut p, 4800);
	assert!(out[0].abs() < 0.01);
	assert!(max_delta(&out) < 0.2);
	c.stop().unwrap();
	let tail = render(&mut p, 4800);
	assert!(max_delta(&tail) < 0.2);
	assert!(
		tail[FADE_SAMPLES + 1..].iter().all(|&s| s == 0.0),
		"silent after the fade"
	);
	assert_eq!(p.state(), ToneState::Idle);
}

#[test]
fn start_and_stop_are_idempotent() {
	let (mut p, mut c) = new();
	c.stop().unwrap();
	assert!(render(&mut p, 480).iter().all(|&s| s == 0.0));
	c.start(Sound::Ring, true, 0.5).unwrap();
	let a = render(&mut p, 2400);
	c.start(Sound::Ring, true, 0.5).unwrap();
	let b = render(&mut p, 480);
	// Not restarted: position continues rather than refading from zero.
	assert!(b[0].abs() > 0.0 || a[2399] == 0.0);
	let ToneState::Playing { position, .. } = p.state() else {
		panic!("playing");
	};
	assert_eq!(position, 2880);
	c.stop().unwrap();
	c.stop().unwrap();
	render(&mut p, 2400);
	assert!(p.is_idle());
}

#[test]
fn switching_sound_fades_out_then_in() {
	let (mut p, mut c) = new();
	c.start(Sound::Ring, true, 1.0).unwrap();
	render(&mut p, 4800);
	c.start(Sound::Ringback, true, 1.0).unwrap();
	let out = render(&mut p, 4800);
	assert!(max_delta(&out) < 0.2);
	assert!(matches!(
		p.state(),
		ToneState::Playing {
			sound: Sound::Ringback,
			..
		}
	));
}

#[test]
fn one_shot_ends_by_itself() {
	let (mut p, mut c) = new();
	c.start(Sound::CallEnd, false, 1.0).unwrap();
	let out = render(&mut p, Sound::CallEnd.period_samples() + 480);
	assert!(peak(&out) > 0.1);
	assert!(p.is_idle());
}

#[test]
fn volume_bounds_and_scaling() {
	let (mut p, mut c) = new();
	for bad in [-0.1, 1.1, f32::NAN, f32::INFINITY] {
		assert!(c.start(Sound::Ring, true, bad).is_err());
		assert!(c.set_volume(bad).is_err());
	}
	assert!(c.start(Sound::Ring, true, 0.0).is_ok());
	assert_eq!(peak(&render(&mut p, 9600)), 0.0);
	c.set_volume(1.0).unwrap();
	let full = peak(&render(&mut p, 48_000));
	let (mut p2, mut c2) = new();
	c2.start(Sound::Ring, true, 0.5).unwrap();
	let half = peak(&render(&mut p2, 48_000));
	assert!(full <= 0.25 + 1e-3 && full > 0.2);
	assert!((half - full * 0.5).abs() < 1e-3);
}

#[test]
fn muted_tone_is_silent_and_keeps_position() {
	let (mut p, mut c) = new();
	c.start(Sound::Ring, true, 1.0).unwrap();
	render(&mut p, 4800);
	c.set_muted(true).unwrap();
	let out = render(&mut p, 4800);
	assert!(max_delta(&out) < 0.2);
	assert!(out[FADE_SAMPLES + 1..].iter().all(|&s| s == 0.0));
	assert!(matches!(
		p.state(),
		ToneState::Playing { position: 9600, .. }
	));
	c.set_muted(false).unwrap();
	assert!(peak(&render(&mut p, 9600)) > 0.1);
}

#[test]
fn command_queue_is_bounded() {
	let (_p, mut c) = new();
	let mut full = false;
	for _ in 0..100 {
		full |= c.stop().is_err();
	}
	assert!(full);
}

#[test]
fn mixer_applies_master_gain_and_limiter() {
	let (mut p, mut c) = new();
	c.start(Sound::Ring, true, 1.0).unwrap();
	let mut mixer = Mixer::new(480);
	let slot = mixer.open_slot().unwrap();
	let loud = vec![0.9_f32; 480];
	let mut out = [0.0_f32; 480];
	let mut tone_only = Vec::new();
	let mut max_out = 0.0_f32;
	for _ in 0..40 {
		mixer.begin();
		mixer.add(slot, &loud).unwrap();
		mixer.add_tones(&mut p);
		mixer.finish(&mut out).unwrap();
		max_out = max_out.max(peak(&out));
	}
	assert!(max_out <= 1.0, "limiter bounds the sum: {max_out}");
	// Tone alone follows master gain.
	let (mut p2, mut c2) = new();
	c2.start(Sound::Ring, true, 1.0).unwrap();
	mixer.close_slot(slot);
	mixer.set_master_gain(0.0).unwrap();
	for _ in 0..4 {
		mixer.begin();
		mixer.add_tones(&mut p2);
		mixer.finish(&mut out).unwrap();
		tone_only.extend_from_slice(&out);
	}
	assert!(tone_only[480..].iter().all(|&s| s == 0.0));
}
