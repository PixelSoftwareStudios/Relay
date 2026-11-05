//! STOI (Taal et al., 2011) and extended STOI (Jensen and Taal, 2016).
//!
//! Follows the published algorithm: 10 kHz analysis, silent-frame removal
//! (40 dB dynamic range on the clean signal), 256-sample Hann frames with
//! 128-sample hop, 512-point DFT, 15 one-third-octave bands from 150 Hz,
//! 30-frame segments. Written from the paper; it has not been validated
//! against a reference implementation, so treat absolute values as
//! indicative and compare candidates only against each other.

use std::f64::consts::TAU;

use relay_audio::convert::StreamResampler;

const FS: u32 = 10_000;
const FRAME: usize = 256;
const HOP: usize = 128;
const NFFT: usize = 512;
const BANDS: usize = 15;
const MIN_FREQ: f64 = 150.0;
const SEGMENT: usize = 30;
const BETA_DB: f64 = -15.0;
const DYN_RANGE_DB: f64 = 40.0;
const EPS: f64 = f64::EPSILON;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StoiScores {
	pub stoi: f64,
	pub estoi: f64,
}

/// Score `degraded` against `clean`; both at `rate_hz`. `None` if there is
/// too little non-silent audio for one 30-frame segment.
pub fn stoi(clean: &[f32], degraded: &[f32], rate_hz: u32) -> Option<StoiScores> {
	let n = clean.len().min(degraded.len());
	let x = to_10k(&clean[..n], rate_hz)?;
	let y = to_10k(&degraded[..n], rate_hz)?;
	let n = x.len().min(y.len());
	let (x, y) = remove_silent_frames(&x[..n], &y[..n]);
	let xt = band_envelopes(&x)?;
	let yt = band_envelopes(&y)?;
	let frames = xt[0].len().min(yt[0].len());
	if frames < SEGMENT {
		return None;
	}
	let segments = frames - SEGMENT + 1;
	let clip = 1.0 + 10f64.powf(-BETA_DB / 20.0);
	let (mut d_stoi, mut d_estoi) = (0.0, 0.0);
	for m in 0..segments {
		for b in 0..BANDS {
			let xs = &xt[b][m..m + SEGMENT];
			let ys = &yt[b][m..m + SEGMENT];
			let alpha = norm(xs) / (norm(ys) + EPS);
			let clipped: Vec<f64> = xs
				.iter()
				.zip(ys)
				.map(|(&x, &y)| (y * alpha).min(x * clip))
				.collect();
			d_stoi += correlation(xs, &clipped);
		}
		d_estoi += segment_estoi(&xt, &yt, m);
	}
	Some(StoiScores {
		stoi: d_stoi / (BANDS * segments) as f64,
		estoi: d_estoi / segments as f64,
	})
}

fn to_10k(samples: &[f32], rate_hz: u32) -> Option<Vec<f64>> {
	if rate_hz == FS {
		return Some(samples.iter().map(|&s| f64::from(s)).collect());
	}
	let chunk = (rate_hz / 100).max(1) as usize;
	let mut resampler = StreamResampler::new_fixed(rate_hz, FS, chunk).ok()?;
	let mut out = Vec::with_capacity(samples.len() * FS as usize / rate_hz as usize + 256);
	resampler
		.push(samples, |b| out.extend(b.iter().map(|&s| f64::from(s))))
		.ok()?;
	Some(out)
}

/// Symmetric Hann window without its zero end points.
fn window() -> Vec<f64> {
	(1..=FRAME)
		.map(|i| 0.5 - 0.5 * (TAU * i as f64 / (FRAME + 1) as f64).cos())
		.collect()
}

fn frame_starts(len: usize) -> impl Iterator<Item = usize> {
	(0..len.saturating_sub(FRAME)).step_by(HOP)
}

/// Drop frames more than 40 dB below the loudest clean frame, from both
/// signals, and overlap-add what remains.
fn remove_silent_frames(x: &[f64], y: &[f64]) -> (Vec<f64>, Vec<f64>) {
	let w = window();
	let energies: Vec<f64> = frame_starts(x.len())
		.map(|s| {
			let e = x[s..s + FRAME]
				.iter()
				.zip(&w)
				.map(|(v, w)| (v * w).powi(2))
				.sum::<f64>()
				.sqrt();
			20.0 * (e / (FRAME as f64).sqrt() + EPS).log10()
		})
		.collect();
	let max = energies.iter().copied().fold(f64::MIN, f64::max);
	let kept: Vec<usize> = frame_starts(x.len())
		.zip(&energies)
		.filter(|(_, e)| max - DYN_RANGE_DB - **e < 0.0)
		.map(|(s, _)| s)
		.collect();
	if kept.is_empty() {
		return (Vec::new(), Vec::new());
	}
	let len = (kept.len() - 1) * HOP + FRAME;
	let (mut xo, mut yo) = (vec![0.0; len], vec![0.0; len]);
	for (k, &s) in kept.iter().enumerate() {
		for i in 0..FRAME {
			xo[k * HOP + i] += x[s + i] * w[i];
			yo[k * HOP + i] += y[s + i] * w[i];
		}
	}
	(xo, yo)
}

/// One-third-octave band edges as DFT bin ranges `[low, high)`.
fn band_bins() -> [(usize, usize); BANDS] {
	let nearest = |f: f64| -> usize {
		(0..=NFFT / 2)
			.min_by(|&a, &b| {
				let da = (a as f64 * FS as f64 / NFFT as f64 - f).abs();
				let db = (b as f64 * FS as f64 / NFFT as f64 - f).abs();
				da.total_cmp(&db)
			})
			.unwrap_or(0)
	};
	std::array::from_fn(|k| {
		let low = MIN_FREQ * 2f64.powf((2.0 * k as f64 - 1.0) / 6.0);
		let high = MIN_FREQ * 2f64.powf((2.0 * k as f64 + 1.0) / 6.0);
		(nearest(low), nearest(high))
	})
}

/// Per-band envelope over time: `result[band][frame]`.
fn band_envelopes(signal: &[f64]) -> Option<Vec<Vec<f64>>> {
	let w = window();
	let starts: Vec<usize> = frame_starts(signal.len()).collect();
	if starts.is_empty() {
		return None;
	}
	let (cos, sin): (Vec<f64>, Vec<f64>) = (0..NFFT)
		.map(|i| {
			let a = TAU * i as f64 / NFFT as f64;
			(a.cos(), a.sin())
		})
		.unzip();
	let bands = band_bins();
	let mut out: Vec<Vec<f64>> = (0..BANDS)
		.map(|_| Vec::with_capacity(starts.len()))
		.collect();
	let mut power = vec![0.0; NFFT / 2 + 1];
	let mut windowed = [0.0; FRAME];
	for &s in &starts {
		for i in 0..FRAME {
			windowed[i] = signal[s + i] * w[i];
		}
		for (k, p) in power.iter_mut().enumerate() {
			let (mut re, mut im) = (0.0, 0.0);
			for (n, &v) in windowed.iter().enumerate() {
				let idx = (k * n) % NFFT;
				re += v * cos[idx];
				im -= v * sin[idx];
			}
			*p = re * re + im * im;
		}
		for (b, &(lo, hi)) in bands.iter().enumerate() {
			out[b].push(power[lo..hi].iter().sum::<f64>().sqrt());
		}
	}
	Some(out)
}

fn norm(v: &[f64]) -> f64 {
	v.iter().map(|x| x * x).sum::<f64>().sqrt()
}

fn mean(v: &[f64]) -> f64 {
	v.iter().sum::<f64>() / v.len() as f64
}

/// Pearson-style correlation with the paper's epsilon-regularised norms.
fn correlation(a: &[f64], b: &[f64]) -> f64 {
	let (ma, mb) = (mean(a), mean(b));
	let (mut dot, mut na, mut nb) = (0.0, 0.0, 0.0);
	for (x, y) in a.iter().zip(b) {
		let (x, y) = (x - ma, y - mb);
		dot += x * y;
		na += x * x;
		nb += y * y;
	}
	dot / ((na.sqrt() + EPS) * (nb.sqrt() + EPS))
}

/// eSTOI segment score: normalise rows then columns, average the column
/// inner products over the segment length.
fn segment_estoi(xt: &[Vec<f64>], yt: &[Vec<f64>], m: usize) -> f64 {
	let prep = |t: &[Vec<f64>]| -> Vec<Vec<f64>> {
		let mut rows: Vec<Vec<f64>> = t
			.iter()
			.map(|band| {
				let seg = &band[m..m + SEGMENT];
				let mu = mean(seg);
				let centred: Vec<f64> = seg.iter().map(|v| v - mu).collect();
				let n = norm(&centred) + EPS;
				centred.iter().map(|v| v / n).collect()
			})
			.collect();
		for f in 0..SEGMENT {
			let mu = rows.iter().map(|r| r[f]).sum::<f64>() / BANDS as f64;
			let n = rows.iter().map(|r| (r[f] - mu).powi(2)).sum::<f64>().sqrt() + EPS;
			for r in &mut rows {
				r[f] = (r[f] - mu) / n;
			}
		}
		rows
	};
	let (x, y) = (prep(xt), prep(yt));
	let dot: f64 = x
		.iter()
		.zip(&y)
		.map(|(a, b)| a.iter().zip(b).map(|(p, q)| p * q).sum::<f64>())
		.sum();
	dot / SEGMENT as f64
}
