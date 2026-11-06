//! `WatchCallStats`: one call's live measurements once per second.
//!
//! Kept apart from the state stream: the numbers change every second and would flood it. A
//! client opens this only while it shows them.

use std::time::Duration;

use relay_api::pb::{CallStats, MediaPathState, WatchCallStatsRequest};
use relay_core::ClientId;
use tokio::sync::mpsc;
use tokio::time::{MissedTickBehavior, interval};
use tokio_stream::wrappers::ReceiverStream;
use tonic::{Response, Status};

use super::EngineService;
use crate::service::call_stats::{StatsWindow, to_pb};
use crate::service::convert::call_id;

/// The sampling period.
const PERIOD: Duration = Duration::from_secs(1);

pub(super) fn open(
	service: &EngineService,
	caller: ClientId,
	body: WatchCallStatsRequest,
) -> Result<Response<ReceiverStream<Result<CallStats, Status>>>, Status> {
	let call = call_id(&body.call_id)?;
	let shared = &service.shared;
	let slot = shared.limiter.acquire()?;
	let ticket = shared.book.open_stream(caller);
	let (tx, rx) = mpsc::channel(2);
	let media = shared.media.clone();
	tokio::spawn(async move {
		let mut window = StatsWindow::default();
		let mut ticker = interval(PERIOD);
		ticker.set_missed_tick_behavior(MissedTickBehavior::Skip);
		let started = tokio::time::Instant::now();
		loop {
			ticker.tick().await;
			let at_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX);
			let sample = match media.call_stats(call) {
				Some(raw) => to_pb(call, &raw, &window.push(at_ms, &raw)),
				None => CallStats {
					call_id: call.to_bytes().to_vec(),
					path_state: MediaPathState::Waiting as i32,
					path_reason: "media is not prepared".to_owned(),
					transport: "lan".to_owned(),
					..CallStats::default()
				},
			};
			// A slow client misses a second rather than queueing up old numbers.
			match tx.try_send(Ok(sample)) {
				Err(mpsc::error::TrySendError::Closed(_)) => break,
				Ok(()) | Err(mpsc::error::TrySendError::Full(_)) => {}
			}
		}
		drop((slot, ticket));
	});
	Ok(Response::new(ReceiverStream::new(rx)))
}
