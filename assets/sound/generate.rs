//! Deterministic generator for the embedded tones.
//!
//! Build and run from this directory: `rustc -O generate.rs && ./generate`.
//! Output is raw little-endian signed 16-bit mono PCM at 48 kHz holding only
//! the audible part of each sound; the loop period (trailing silence) lives in
//! `relay-audio`'s `tones` module.

use std::f64::consts::TAU;
use std::fs;

const RATE: f64 = 48_000.0;

/// One note: fundamental plus two soft harmonics, raised-cosine attack/release.
fn note(out: &mut Vec<f32>, freqs: &[f64], secs: f64, peak: f64, fade: f64) {
	let n = (secs * RATE).round() as usize;
	let f = (fade * RATE).round() as usize;
	for i in 0..n {
		let t = i as f64 / RATE;
		let mut s = 0.0;
		for &hz in freqs {
			s += (TAU * hz * t).sin() + 0.28 * (TAU * 2.0 * hz * t).sin() + 0.08 * (TAU * 3.0 * hz * t).sin();
		}
		s /= freqs.len() as f64 * 1.36;
		let up = if i < f { 0.5 - 0.5 * (std::f64::consts::PI * i as f64 / f as f64).cos() } else { 1.0 };
		let k = n - 1 - i;
		let down = if k < f { 0.5 - 0.5 * (std::f64::consts::PI * k as f64 / f as f64).cos() } else { 1.0 };
		out.push((s * peak * up * down) as f32);
	}
}

fn gap(out: &mut Vec<f32>, secs: f64) {
	out.extend(std::iter::repeat_n(0.0, (secs * RATE).round() as usize));
}

fn write(name: &str, samples: &[f32]) {
	let mut bytes = Vec::with_capacity(samples.len() * 2);
	for &s in samples {
		let v = (s.clamp(-1.0, 1.0) * 32767.0).round() as i16;
		bytes.extend_from_slice(&v.to_le_bytes());
	}
	fs::write(name, bytes).expect("write sound");
}

fn main() {
	// Ring: E5, A5, short rest, E5, A5. Period 2.0 s.
	let mut ring = Vec::new();
	note(&mut ring, &[659.25], 0.20, 0.45, 0.012);
	note(&mut ring, &[880.0], 0.20, 0.45, 0.012);
	gap(&mut ring, 0.04);
	note(&mut ring, &[659.25], 0.20, 0.45, 0.012);
	note(&mut ring, &[880.0], 0.16, 0.45, 0.030);
	write("ring.raw", &ring);

	// Ringback: two soft dual-tone bursts. Period 3.0 s.
	let mut back = Vec::new();
	note(&mut back, &[420.0, 500.0], 0.40, 0.40, 0.020);
	gap(&mut back, 0.15);
	note(&mut back, &[420.0, 500.0], 0.40, 0.40, 0.020);
	write("ringback.raw", &back);

	// Call end: falling pair.
	let mut end = Vec::new();
	note(&mut end, &[660.0], 0.07, 0.40, 0.010);
	note(&mut end, &[440.0], 0.09, 0.40, 0.020);
	write("call-end.raw", &end);

	// Connected: rising pair.
	let mut up = Vec::new();
	note(&mut up, &[523.25], 0.07, 0.40, 0.010);
	note(&mut up, &[784.0], 0.09, 0.40, 0.020);
	write("connected.raw", &up);
}
