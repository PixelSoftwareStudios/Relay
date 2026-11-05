//! Objective metrics. SI-SDR and delay are implemented here; STOI has no
//! vetted Rust library available and is deliberately not computed.

/// Scale-invariant SDR in dB of `estimate` against `reference` (equal length).
pub fn si_sdr_db(reference: &[f32], estimate: &[f32]) -> Option<f64> {
	let n = reference.len().min(estimate.len());
	if n == 0 {
		return None;
	}
	let (mut dot, mut energy) = (0.0_f64, 0.0_f64);
	for i in 0..n {
		dot += f64::from(reference[i]) * f64::from(estimate[i]);
		energy += f64::from(reference[i]).powi(2);
	}
	if energy == 0.0 {
		return None;
	}
	let alpha = dot / energy;
	let (mut target, mut residual) = (0.0_f64, 0.0_f64);
	for i in 0..n {
		let t = alpha * f64::from(reference[i]);
		target += t * t;
		residual += (f64::from(estimate[i]) - t).powi(2);
	}
	(residual > 0.0 && target > 0.0).then(|| 10.0 * (target / residual).log10())
}

/// Lag in samples by which `output` trails `input`, found by
/// cross-correlation over `-max_lag..=max_lag` on a central (up to 8 s)
/// segment: a 4x-decimated coarse search, then a full-rate refinement.
pub fn correlation_delay(input: &[f32], output: &[f32], max_lag: usize) -> Option<i64> {
	const DECIM: usize = 4;
	let n = input.len().min(output.len());
	let seg = 384_000.min(n.saturating_sub(2 * max_lag + DECIM));
	if seg < 4_000 {
		return None;
	}
	let start = (n - seg) / 2;
	let score = |lag: i64, step: usize| -> f64 {
		(start..start + seg)
			.step_by(step)
			.filter_map(|i| {
				let j = i as i64 + lag;
				(j >= 0 && (j as usize) < n)
					.then(|| f64::from(input[i]) * f64::from(output[j as usize]))
			})
			.sum()
	};
	let max = max_lag as i64;
	let coarse = (-max..=max)
		.step_by(DECIM)
		.map(|lag| (lag, score(lag, DECIM)))
		.max_by(|a, b| a.1.total_cmp(&b.1))?;
	let fine = (coarse.0 - DECIM as i64..=coarse.0 + DECIM as i64)
		.filter(|l| l.abs() <= max)
		.map(|lag| (lag, score(lag, 1)))
		.max_by(|a, b| a.1.total_cmp(&b.1))?;
	(fine.1 > 0.0).then_some(fine.0)
}

/// Shift `output` back by `delay` so it lines up with `reference`, and
/// return equal-length slices over the steady-state region after `skip`.
pub fn align<'a>(
	reference: &'a [f32],
	output: &'a [f32],
	delay: i64,
	skip: usize,
) -> (&'a [f32], &'a [f32]) {
	let d = delay.max(0) as usize;
	let n = reference.len().min(output.len().saturating_sub(d));
	if n <= skip {
		return (&[], &[]);
	}
	(&reference[skip..n], &output[d + skip..d + n])
}

/// Taps before and after the aligned position used by [`filtered_sdr_db`].
pub const FILTER_BEFORE: usize = 224;
pub const FILTER_AFTER: usize = 224;
/// Evaluation length cap for [`filtered_sdr_db`], in samples (2.5 s at 48 kHz).
const FILTER_MAX_SAMPLES: usize = 120_000;

/// SDR in dB after allowing `estimate` to differ from `reference` by the
/// best linear filter of `FILTER_BEFORE + FILTER_AFTER` taps (in the style of
/// BSS-eval). Unlike [`si_sdr_db`] it tolerates frequency-dependent delay,
/// which minimum-phase or band-split processors introduce. `reference` and
/// `estimate` must already be aligned; the central 2.5 s are used.
pub fn filtered_sdr_db(reference: &[f32], estimate: &[f32]) -> Option<f64> {
	let taps = FILTER_BEFORE + FILTER_AFTER;
	let n = reference.len().min(estimate.len());
	if n < taps * 8 {
		return None;
	}
	let len = n.min(FILTER_MAX_SAMPLES);
	let start = (n - len) / 2;
	let x: Vec<f64> = reference[start..start + len]
		.iter()
		.map(|&v| f64::from(v))
		.collect();
	let y: Vec<f64> = estimate[start..start + len]
		.iter()
		.map(|&v| f64::from(v))
		.collect();
	let lo = FILTER_BEFORE; // first valid t
	let hi = len - FILTER_AFTER; // one past the last valid t
	// Tap j multiplies x[t + AFTER - j], so taps cover x from AFTER samples
	// ahead to BEFORE samples behind the aligned position.
	let mut r = vec![0.0_f64; taps];
	for (m, slot) in r.iter_mut().enumerate() {
		*slot = (lo..hi.saturating_sub(m)).map(|t| x[t] * x[t + m]).sum();
	}
	let mut p = vec![0.0_f64; taps];
	for (j, slot) in p.iter_mut().enumerate() {
		*slot = (lo..hi).map(|t| y[t] * x[t + FILTER_AFTER - j]).sum();
	}
	let energy_y: f64 = (lo..hi).map(|t| y[t] * y[t]).sum();
	let h = solve_toeplitz_spd(&r, &p)?;
	let target: f64 = h.iter().zip(&p).map(|(a, b)| a * b).sum();
	// Numerical floor: report at most 120 dB for an exact fit.
	let error = (energy_y - target).max(energy_y * 1e-12);
	(target > 0.0).then(|| 10.0 * (target / error).log10())
}

/// Solve `R h = p` for a symmetric Toeplitz `R` given by its first row `r`,
/// via a regularised dense Cholesky factorisation.
fn solve_toeplitz_spd(r: &[f64], p: &[f64]) -> Option<Vec<f64>> {
	let n = p.len();
	let eps = r[0] * 1e-9 + 1e-18;
	let at = |i: usize, j: usize| r[i.abs_diff(j)] + if i == j { eps } else { 0.0 };
	let mut l = vec![0.0_f64; n * n];
	for i in 0..n {
		for j in 0..=i {
			let mut sum = at(i, j);
			for k in 0..j {
				sum -= l[i * n + k] * l[j * n + k];
			}
			if i == j {
				if sum <= 0.0 {
					return None;
				}
				l[i * n + j] = sum.sqrt();
			} else {
				l[i * n + j] = sum / l[j * n + j];
			}
		}
	}
	let mut z = vec![0.0_f64; n];
	for i in 0..n {
		let mut sum = p[i];
		for k in 0..i {
			sum -= l[i * n + k] * z[k];
		}
		z[i] = sum / l[i * n + i];
	}
	let mut h = vec![0.0_f64; n];
	for i in (0..n).rev() {
		let mut sum = z[i];
		for k in i + 1..n {
			sum -= l[k * n + i] * h[k];
		}
		h[i] = sum / l[i * n + i];
	}
	Some(h)
}
