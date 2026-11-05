//! `WatchTelemetry`: rate-limited level meters and counters.
//!
//! Samples are read from atomics the media workers keep up to date, so the
//! audio threads never wait for a client. Each stream keeps a few samples
//! and drops the oldest when its client is slow.

use std::collections::HashSet;
use std::time::Duration;

use relay_api::pb::{CallTelemetry, TelemetryRequest, TelemetrySnapshot};
use relay_core::ClientId;
use tokio::sync::mpsc;
use tokio::time::{Instant, MissedTickBehavior, interval};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Response, Status};

use super::EngineService;
use crate::service::convert::call_id;
use crate::service::error::ServiceError;
use crate::service::ports::{CallMeter, MediaTelemetry};
use crate::service::telemetry::DropOldest;

/// The fastest a stream is sampled.
pub(super) const MAX_HZ: u32 = 20;
const DEFAULT_HZ: u32 = 10;
const QUEUE: usize = 8;
const MAX_FILTER_CALLS: usize = 8;

fn period(request: &TelemetryRequest) -> Duration {
	let wanted = request.meters_hz.max(request.stats_hz);
	let hz = if wanted == 0 {
		DEFAULT_HZ
	} else {
		wanted.min(MAX_HZ)
	};
	Duration::from_millis(1000 / u64::from(hz))
}

fn call_telemetry(meter: &CallMeter, call: relay_core::CallId) -> CallTelemetry {
	CallTelemetry {
		call_id: call.to_bytes().to_vec(),
		speaking: meter.speaking,
		receive_dbfs: meter.receive_dbfs,
		rtt_ms: None,
		rtp_interarrival_jitter_ms: None,
		playout_target_ms: meter.playout_target_ms,
		packets_received: meter.packets_received,
		packets_late: meter.packets_late,
		packets_missing_at_deadline: meter.packets_missing,
		concealed_samples: meter.concealed_samples,
		estimated_mouth_to_ear_ms: None,
		latency_estimate_method: "unmeasured".to_owned(),
	}
}

fn snapshot(
	sequence: u64,
	elapsed: Duration,
	incarnation: &[u8],
	media: &MediaTelemetry,
	filter: &HashSet<Vec<u8>>,
) -> TelemetrySnapshot {
	let calls = media
		.calls
		.iter()
		.filter_map(|meter| Some(call_telemetry(meter, meter.call?)))
		.filter(|call| filter.is_empty() || filter.contains(&call.call_id))
		.collect();
	TelemetrySnapshot {
		engine_incarnation: incarnation.to_vec(),
		sample_sequence: sequence,
		monotonic_ms_since_engine_start: u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX),
		microphone_dbfs: media.microphone_dbfs,
		capture_overruns: media.capture_overruns,
		render_underruns: media.render_underruns,
		telemetry_samples_dropped: 0,
		calls,
	}
}

pub(super) fn open(
	service: &EngineService,
	caller: ClientId,
	body: TelemetryRequest,
) -> Result<Response<ReceiverStream<Result<TelemetrySnapshot, Status>>>, Status> {
	if body.call_ids.len() > MAX_FILTER_CALLS {
		return Err(ServiceError::InvalidArgument("too many call filters".to_owned()).into());
	}
	for id in &body.call_ids {
		call_id(id)?;
	}
	let shared = &service.shared;
	let slot = shared.limiter.acquire()?;
	let ticket = shared.book.open_stream(caller);
	let (tx, rx) = mpsc::channel(1);
	let media = shared.media.clone();
	let started = shared.started;
	let incarnation = service.incarnation();
	let tick = period(&body);
	let filter: HashSet<Vec<u8>> = body.call_ids.into_iter().collect();
	tokio::spawn(async move {
		let mut queue = DropOldest::new(QUEUE);
		let mut sequence = 0u64;
		let mut ticker = interval(tick);
		ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
		loop {
			tokio::select! {
				_ = ticker.tick() => {
					sequence += 1;
					let sample = snapshot(
						sequence,
						Instant::now().duration_since(started),
						&incarnation,
						&media.telemetry(),
						&filter,
					);
					queue.push(sample);
				}
				permit = tx.reserve(), if !queue.is_empty() => {
					let Ok(permit) = permit else { break };
					if let Some(mut sample) = queue.pop() {
						sample.telemetry_samples_dropped = queue.dropped();
						permit.send(Ok(sample));
					}
				}
			}
		}
		drop((slot, ticket));
	});
	Ok(Response::new(ReceiverStream::new(rx)))
}
