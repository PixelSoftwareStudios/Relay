//! A fake engine for the screens that show a call: it records commands and counts how often a
//! call-statistics stream is opened and dropped.

#![allow(dead_code)]

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use relay_api::pb;
use relay_ui::convert::call_stats;
use relay_ui::engine::{CallStatsSource, Command, EngineError, Operations, Outcome};
use relay_ui::model::CallStats;

/// The call of `phone::busy_state`'s running call.
pub const CALL: [u8; 16] = [2; 16];

#[derive(Default)]
pub struct Ops {
	pub commands: Mutex<Vec<Command>>,
	pub opened: AtomicUsize,
	pub closed: Arc<AtomicUsize>,
}

impl Ops {
	pub fn sent(&self) -> Vec<Command> {
		self.commands.lock().unwrap().clone()
	}
}

struct Source {
	first: bool,
	pub closed: Arc<AtomicUsize>,
}

impl Drop for Source {
	fn drop(&mut self) {
		self.closed.fetch_add(1, Ordering::SeqCst);
	}
}

pub fn sample() -> CallStats {
	call_stats(&pb::CallStats {
		call_id: CALL.to_vec(),
		path_state: pb::MediaPathState::Failed as i32,
		path_reason: "no reply to the path challenge".into(),
		sending: true,
		rtt_ms: Some(41.0),
		rtt_min_ms: Some(38.0),
		rtt_max_ms: Some(52.0),
		jitter_ms: Some(3.0),
		loss_percent_5s: 0.4,
		loss_percent_total: 0.1,
		jitter_buffer_target_ms: 40.0,
		jitter_buffer_depth_ms: 36.0,
		mouth_to_ear_ms: Some(120.0),
		codec_bitrate_bps: 32_000,
		transport: "lan".into(),
		local_addr: "10.0.0.2:5000".into(),
		remote_addr: "10.0.0.3:5000".into(),
		clock_drift_ppm: Some(12.0),
		capture_buffer_ms: 10.0,
		playback_buffer_ms: 20.0,
		..pb::CallStats::default()
	})
}

#[async_trait]
impl CallStatsSource for Source {
	async fn next(&mut self) -> Result<Option<CallStats>, EngineError> {
		if std::mem::take(&mut self.first) {
			return Ok(Some(sample()));
		}
		std::future::pending().await
	}
}

#[async_trait]
impl Operations for Ops {
	async fn run(&self, command: Command) -> Result<Outcome, EngineError> {
		self.commands.lock().unwrap().push(command);
		Ok(Outcome::default())
	}

	async fn watch_call_stats(
		&self,
		_call_id: Vec<u8>,
	) -> Result<Box<dyn CallStatsSource>, EngineError> {
		self.opened.fetch_add(1, Ordering::SeqCst);
		Ok(Box::new(Source {
			first: true,
			closed: self.closed.clone(),
		}))
	}
}
