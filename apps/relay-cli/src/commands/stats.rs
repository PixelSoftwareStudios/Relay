//! `relay stats` and `relay calls`: live measurements of a call and the live calls with them.

use std::fmt::{self, Display, Formatter};
use std::time::Duration;

use relay_api::pb::{CallStats, MediaPathState};
use serde::Serialize;

use super::Out;
use super::resolve::resolve_call;
use super::views::CallList;
use crate::cli::{CliError, StatsArgs};
use crate::client::{Control, Feed, Subscribe};

/// How long a one-shot command waits for the first sample.
const FIRST_SAMPLE: Duration = Duration::from_secs(5);

/// Diagnostics of one call, as shown and as JSON.
#[derive(Debug, Clone, Serialize)]
pub struct StatsView {
	pub path: &'static str,
	pub path_reason: Option<String>,
	pub sending: bool,
	pub receiving: bool,
	pub packets_sent: u64,
	pub packets_received: u64,
	pub mic_level_dbfs: Option<f32>,
	pub rtt_ms: Option<f64>,
	pub rtt_min_ms: Option<f64>,
	pub rtt_max_ms: Option<f64>,
	pub jitter_ms: Option<f64>,
	pub loss_percent_5s: f64,
	pub loss_percent_total: f64,
	pub jitter_buffer_target_ms: f64,
	pub jitter_buffer_depth_ms: f64,
	pub playout_delay_ms: f64,
	pub mouth_to_ear_ms: Option<f64>,
	pub codec_bitrate_bps: u32,
	pub packet_bytes: u32,
	pub local_addr: Option<String>,
	pub remote_addr: Option<String>,
	pub transport: String,
	pub clock_drift_ppm: Option<f64>,
	pub capture_buffer_frames: u32,
	pub capture_buffer_ms: f64,
	pub playback_buffer_frames: u32,
	pub playback_buffer_ms: f64,
}

fn text(s: &str) -> Option<String> {
	(!s.is_empty()).then(|| s.to_owned())
}

impl StatsView {
	/// The wire sample as the CLI reports it.
	pub fn new(s: &CallStats) -> Self {
		Self {
			path: match MediaPathState::try_from(s.path_state) {
				Ok(MediaPathState::Connected) => "connected",
				Ok(MediaPathState::Failed) => "failed",
				_ => "waiting",
			},
			path_reason: text(&s.path_reason),
			sending: s.sending,
			receiving: s.receiving,
			packets_sent: s.packets_sent,
			packets_received: s.packets_received,
			mic_level_dbfs: s.mic_level_dbfs,
			rtt_ms: s.rtt_ms,
			rtt_min_ms: s.rtt_min_ms,
			rtt_max_ms: s.rtt_max_ms,
			jitter_ms: s.jitter_ms,
			loss_percent_5s: s.loss_percent_5s,
			loss_percent_total: s.loss_percent_total,
			jitter_buffer_target_ms: s.jitter_buffer_target_ms,
			jitter_buffer_depth_ms: s.jitter_buffer_depth_ms,
			playout_delay_ms: s.playout_delay_ms,
			mouth_to_ear_ms: s.mouth_to_ear_ms,
			codec_bitrate_bps: s.codec_bitrate_bps,
			packet_bytes: s.packet_bytes,
			local_addr: text(&s.local_addr),
			remote_addr: text(&s.remote_addr),
			transport: s.transport.clone(),
			clock_drift_ppm: s.clock_drift_ppm,
			capture_buffer_frames: s.capture_buffer_frames,
			capture_buffer_ms: s.capture_buffer_ms,
			playback_buffer_frames: s.playback_buffer_frames,
			playback_buffer_ms: s.playback_buffer_ms,
		}
	}
}

fn ms(value: Option<f64>) -> String {
	value.map_or_else(|| "-".to_owned(), |v| format!("{v:.1} ms"))
}

impl Display for StatsView {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		write!(f, "path        {}", self.path)?;
		if let Some(reason) = &self.path_reason {
			write!(f, " ({reason})")?;
		}
		writeln!(
			f,
			"\nroute       {} -> {} ({})",
			self.local_addr.as_deref().unwrap_or("-"),
			self.remote_addr.as_deref().unwrap_or("-"),
			self.transport
		)?;
		writeln!(
			f,
			"sending     {}  ({} packets)    receiving  {}  ({} packets)",
			yes(self.sending),
			self.packets_sent,
			yes(self.receiving),
			self.packets_received
		)?;
		if let Some(level) = self.mic_level_dbfs {
			writeln!(f, "mic level   {level:.1} dBFS")?;
		}
		writeln!(
			f,
			"rtt         {} (min {}, max {})",
			ms(self.rtt_ms),
			ms(self.rtt_min_ms),
			ms(self.rtt_max_ms)
		)?;
		writeln!(
			f,
			"jitter      {}    loss {:.1} % (5 s), {:.1} % (total)",
			ms(self.jitter_ms),
			self.loss_percent_5s,
			self.loss_percent_total
		)?;
		writeln!(
			f,
			"jitter buf  {:.0} ms deep, target {:.0} ms    playout {:.1} ms",
			self.jitter_buffer_depth_ms, self.jitter_buffer_target_ms, self.playout_delay_ms
		)?;
		writeln!(f, "mouth/ear   {} (estimate)", ms(self.mouth_to_ear_ms))?;
		writeln!(
			f,
			"codec       {} bit/s, {} byte packets    drift {}",
			self.codec_bitrate_bps,
			self.packet_bytes,
			self.clock_drift_ppm
				.map_or_else(|| "-".to_owned(), |p| format!("{p:.1} ppm"))
		)?;
		write!(
			f,
			"buffers     capture {} frames ({:.1} ms), playback {} frames ({:.1} ms)",
			self.capture_buffer_frames,
			self.capture_buffer_ms,
			self.playback_buffer_frames,
			self.playback_buffer_ms
		)
	}
}

fn yes(v: bool) -> &'static str {
	if v { "yes" } else { "no" }
}

/// `relay stats [CALL] [--watch]`.
pub async fn run<C: Control + Subscribe>(
	client: &mut C,
	args: &StatsArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let snapshot = client.snapshot().await?;
	let id = resolve_call(&snapshot, args.call.as_deref())?
		.call_id
		.clone();
	let mut stream = client.open_call_stats(&id).await?;
	if args.watch {
		while let Some(sample) = stream.next().await? {
			out.emit(&StatsView::new(&sample))?;
		}
		return Ok(());
	}
	let sample = first_sample(&mut stream).await?;
	out.emit(&StatsView::new(&sample))
}

async fn first_sample<F: Feed<Item = CallStats>>(stream: &mut F) -> Result<CallStats, CliError> {
	match tokio::time::timeout(FIRST_SAMPLE, stream.next()).await {
		Ok(Ok(Some(sample))) => Ok(sample),
		Ok(Ok(None)) => Err(CliError::Usage("the call has no statistics".into())),
		Ok(Err(error)) => Err(error.into()),
		Err(_) => Err(CliError::Usage(
			"the engine sent no statistics in time".into(),
		)),
	}
}

/// `relay calls`: the live calls with their transmit state and, best effort, diagnostics.
pub async fn calls<C: Control + Subscribe>(
	client: &mut C,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	let snapshot = client.snapshot().await?;
	let mut list = CallList::from_snapshot(&snapshot);
	for view in &mut list.calls {
		if view.state != "connected" {
			continue;
		}
		let Ok(id) = hex::decode(&view.id) else {
			continue;
		};
		if let Ok(mut stream) = client.open_call_stats(&id).await
			&& let Ok(sample) = first_sample(&mut stream).await
		{
			view.diagnostics = Some(StatsView::new(&sample));
		}
	}
	out.emit(&list)
}
