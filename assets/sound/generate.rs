//! Deterministic generator for the embedded tones.
//!
//! Build and run from this directory: `rustc -O --edition 2024 generate.rs -o /tmp/gen && /tmp/gen`.
//! Output is raw little-endian signed 16-bit mono PCM at 48 kHz holding only
//! the audible part of each sound; the loop period (trailing silence) lives in
//! `relay-audio`'s `tones` module.
//!
//! Levels are part of the design. Every sound is normalised to an exact peak
//! (`RING_PEAK` and friends); the volume setting then scales it, and the engine's
//! defaults (ring volume 0.4, ringback 0.5) land the ring near -20 dBFS and the
//! ringback and blips near -18 dBFS. Nothing here has a square wave or a partial
//! above the fourth.

use std::f64::consts::{PI, TAU};
use std::fs;

const RATE: f64 = 48_000.0;

/// Peak of the ring at volume 1.0: -12 dBFS.
const RING_PEAK: f64 = 0.25;
/// Peak of the ringback at volume 1.0: -12 dBFS.
const RINGBACK_PEAK: f64 = 0.25;
/// Peak of the one-shot blips at volume 1.0: -14 dBFS.
const BLIP_PEAK: f64 = 0.20;

/// A struck, marimba-like note mixed into `out` starting at `start` seconds: a sine fundamental,
/// a quiet second partial that dies quickly and a faint fourth. Slow raised-cosine attack,
/// exponential decay with time constant `tau`, a short raised-cosine release at `len`.
fn strike(out: &mut Vec<f64>, start: f64, hz: f64, len: f64, tau: f64, attack: f64) {
	let first = (start * RATE).round() as usize;
	let n = (len * RATE).round() as usize;
	let a = (attack * RATE).round() as usize;
	let release = (0.060 * RATE).round() as usize;
	if out.len() < first + n {
		out.resize(first + n, 0.0);
	}
	for i in 0..n {
		let t = i as f64 / RATE;
		let tone = (TAU * hz * t).sin()
			+ 0.22 * (-t / (tau * 0.45)).exp() * (TAU * 2.0 * hz * t).sin()
			+ 0.05 * (-t / (tau * 0.25)).exp() * (TAU * 4.0 * hz * t).sin();
		let up = if i < a { 0.5 - 0.5 * (PI * i as f64 / a as f64).cos() } else { 1.0 };
		let k = n - 1 - i;
		let down = if k < release { 0.5 - 0.5 * (PI * k as f64 / release as f64).cos() } else { 1.0 };
		out[first + i] += tone * (-t / tau).exp() * up * down;
	}
}

/// A steady two-frequency swell (the ringback): sines with a slow raised-cosine envelope.
fn swell(out: &mut Vec<f64>, start: f64, freqs: &[f64], len: f64, fade: f64) {
	let first = (start * RATE).round() as usize;
	let n = (len * RATE).round() as usize;
	let f = (fade * RATE).round() as usize;
	if out.len() < first + n {
		out.resize(first + n, 0.0);
	}
	for i in 0..n {
		let t = i as f64 / RATE;
		let s: f64 = freqs.iter().map(|&hz| (TAU * hz * t).sin()).sum::<f64>() / freqs.len() as f64;
		let up = if i < f { 0.5 - 0.5 * (PI * i as f64 / f as f64).cos() } else { 1.0 };
		let k = n - 1 - i;
		let down = if k < f { 0.5 - 0.5 * (PI * k as f64 / f as f64).cos() } else { 1.0 };
		out[first + i] += s * up * down;
	}
}

/// Scales so the loudest sample is exactly `peak`.
fn normalise(samples: &mut [f64], peak: f64) {
	let loudest = samples.iter().fold(0.0_f64, |m, s| m.max(s.abs()));
	for s in samples.iter_mut() {
		*s *= peak / loudest;
	}
}

fn write(name: &str, samples: &[f64]) {
	let mut bytes = Vec::with_capacity(samples.len() * 2);
	for &s in samples {
		let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
		bytes.extend_from_slice(&v.to_le_bytes());
	}
	fs::write(name, bytes).expect("write sound");
}

fn main() {
	// Ring: a soft two-note chime, E4 then B4 (a rising fifth), 25 ms attack, decay over about a
	// second. The audible part is 1.9 s; the loop period is 4.0 s, so the pauses are long.
	let mut ring = Vec::new();
	strike(&mut ring, 0.0, 329.63, 1.30, 0.28, 0.025);
	strike(&mut ring, 0.50, 493.88, 1.40, 0.30, 0.025);
	normalise(&mut ring, RING_PEAK);
	write("ring.raw", &ring);

	// Ringback: two slow, soft swells of a low dual tone (350 and 440 Hz). Period 4.0 s.
	let mut back = Vec::new();
	swell(&mut back, 0.0, &[350.0, 440.0], 0.90, 0.220);
	swell(&mut back, 1.20, &[350.0, 440.0], 0.90, 0.220);
	normalise(&mut back, RINGBACK_PEAK);
	write("ringback.raw", &back);

	// Call end: a falling pair of soft strikes.
	let mut end = Vec::new();
	strike(&mut end, 0.0, 440.0, 0.16, 0.06, 0.010);
	strike(&mut end, 0.07, 329.63, 0.20, 0.07, 0.010);
	normalise(&mut end, BLIP_PEAK);
	write("call-end.raw", &end);

	// Connected: a rising pair.
	let mut up = Vec::new();
	strike(&mut up, 0.0, 329.63, 0.16, 0.06, 0.010);
	strike(&mut up, 0.07, 493.88, 0.20, 0.07, 0.010);
	normalise(&mut up, BLIP_PEAK);
	write("connected.raw", &up);
}
