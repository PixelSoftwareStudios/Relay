//! Aggregation of per-file results by condition and overall.

use serde::Serialize;

use super::runner::FileResult;

#[derive(Debug, Clone, Serialize)]
pub struct ConditionSummary {
	/// `snrm05`, `snr10`, `clean`, ... taken from the file name.
	pub condition: String,
	pub files: usize,
	pub si_sdr_noisy_db: Option<f64>,
	pub si_sdr_output_db: Option<f64>,
	pub si_sdr_improvement_db: Option<f64>,
	pub stoi_noisy: Option<f64>,
	pub stoi_output: Option<f64>,
	pub stoi_improvement: Option<f64>,
	pub estoi_noisy: Option<f64>,
	pub estoi_output: Option<f64>,
	pub estoi_improvement: Option<f64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DelaySummary {
	pub files_with_estimate: usize,
	pub min: i64,
	pub median: i64,
	pub max: i64,
}

pub fn condition_of(name: &str) -> String {
	name.split('_')
		.find(|t| t.starts_with("snr") || *t == "clean")
		.unwrap_or("all")
		.to_owned()
}

fn mean(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
	let v: Vec<f64> = values.flatten().filter(|x| x.is_finite()).collect();
	(!v.is_empty()).then(|| v.iter().sum::<f64>() / v.len() as f64)
}

pub fn by_condition(files: &[FileResult]) -> Vec<ConditionSummary> {
	let mut keys: Vec<String> = files.iter().map(|f| condition_of(&f.name)).collect();
	keys.sort();
	keys.dedup();
	keys.into_iter()
		.map(|condition| {
			let group: Vec<&FileResult> = files
				.iter()
				.filter(|f| condition_of(&f.name) == condition)
				.collect();
			let m = |f: fn(&FileResult) -> Option<f64>| mean(group.iter().map(|r| f(r)));
			ConditionSummary {
				condition,
				files: group.len(),
				si_sdr_noisy_db: m(|r| r.si_sdr_noisy_db),
				si_sdr_output_db: m(|r| r.si_sdr_output_db),
				si_sdr_improvement_db: m(|r| r.si_sdr_improvement_db),
				stoi_noisy: m(|r| r.stoi_noisy),
				stoi_output: m(|r| r.stoi_output),
				stoi_improvement: m(|r| r.stoi_output.zip(r.stoi_noisy).map(|(o, i)| o - i)),
				estoi_noisy: m(|r| r.estoi_noisy),
				estoi_output: m(|r| r.estoi_output),
				estoi_improvement: m(|r| r.estoi_output.zip(r.estoi_noisy).map(|(o, i)| o - i)),
			}
		})
		.collect()
}

pub fn delays(files: &[FileResult]) -> Option<DelaySummary> {
	let mut d: Vec<i64> = files.iter().filter_map(|f| f.delay_xcorr_samples).collect();
	if d.is_empty() {
		return None;
	}
	d.sort_unstable();
	Some(DelaySummary {
		files_with_estimate: d.len(),
		min: d[0],
		median: d[d.len() / 2],
		max: d[d.len() - 1],
	})
}
