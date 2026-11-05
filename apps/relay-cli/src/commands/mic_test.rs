//! `relay mic-test`: sample the microphone level for a few seconds.

use std::fmt::{self, Display, Formatter};
use std::time::Duration;

use serde::Serialize;

use super::{Out, outcome};
use crate::cli::{CliError, MicTestArgs};
use crate::client::{Control, Feed, Subscribe};

/// Summary of one mic test.
#[derive(Debug, Serialize)]
pub struct MicTestResult {
	pub seconds: u64,
	pub samples: u32,
	pub peak_dbfs: Option<f32>,
	pub capture_overruns: u64,
}

impl Display for MicTestResult {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		match self.peak_dbfs {
			Some(peak) => write!(
				f,
				"peak {peak:.1} dBFS over {} samples in {}s ({} overruns)",
				self.samples, self.seconds, self.capture_overruns
			),
			None => write!(
				f,
				"no microphone level reported (is an input device selected?)"
			),
		}
	}
}

pub async fn run<C: Control + Subscribe>(
	client: &mut C,
	args: &MicTestArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let started = client
		.start_mic_test(args.local_playback, args.accept_feedback)
		.await?;
	outcome(&started)?;
	let result = sample(client, args.seconds).await;
	// Always stop the test, even when sampling failed.
	let stopped = client.stop_mic_test().await;
	let result = result?;
	if let Err(err) = stopped {
		return Err(err.into());
	}
	out.emit(&result)
}

async fn sample<C: Subscribe>(client: &mut C, seconds: u64) -> Result<MicTestResult, CliError> {
	let mut stream = client.open_telemetry(10).await?;
	let mut result = MicTestResult {
		seconds,
		samples: 0,
		peak_dbfs: None,
		capture_overruns: 0,
	};
	let deadline = tokio::time::Instant::now() + Duration::from_secs(seconds);
	while let Ok(item) = tokio::time::timeout_at(deadline, stream.next()).await {
		let Some(sample) = item? else { break };
		result.capture_overruns = sample.capture_overruns;
		if let Some(db) = sample.microphone_dbfs {
			result.samples += 1;
			result.peak_dbfs = Some(result.peak_dbfs.map_or(db, |p| p.max(db)));
		}
	}
	Ok(result)
}
