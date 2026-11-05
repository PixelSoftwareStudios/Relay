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
