//! Connection supervisor: connect, mirror state, and rebuild everything from
//! a fresh snapshot after any gap, overflow or daemon restart.

use std::sync::Arc;
use std::time::Duration;

use relay_api::WatchItem;

use crate::controller::SessionSlot;
use crate::convert;
use crate::engine::{Connection, Engine, EngineError};
use crate::hub::Hub;
use crate::model::Link;

const BACKOFF_BASE: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(5);

/// Exponential delay between connection attempts, capped.
pub fn backoff(attempt: u32) -> Duration {
	BACKOFF_BASE
		.saturating_mul(1u32.checked_shl(attempt).unwrap_or(u32::MAX))
		.min(BACKOFF_MAX)
}

/// Run forever; drop or abort the task to stop.
pub async fn supervise(engine: Arc<dyn Engine>, hub: Arc<Hub>, slot: Arc<SessionSlot>) {
	let mut attempt = 0u32;
	let mut had_session = false;
	loop {
		match engine.connect().await {
			Ok(connection) => {
				attempt = 0;
				had_session = true;
				let reason = mirror(connection, &hub, &slot).await;
				slot.clear();
				hub.clear_meters();
				hub.update(|s| s.set_link_down(Link::Reconnecting, Some(reason)));
			}
			Err(error) => {
				let link = match error {
					EngineError::Unavailable(_) => Link::Offline,
					_ if had_session => Link::Reconnecting,
					_ => Link::Connecting,
				};
				hub.update(|s| s.set_link_down(link, Some(error.to_string())));
			}
		}
		tokio::time::sleep(backoff(attempt)).await;
		attempt = attempt.saturating_add(1);
	}
}

/// Mirror one session until it ends; the returned text says why.
async fn mirror(mut conn: Connection, hub: &Hub, slot: &SessionSlot) -> String {
	hub.update(|s| {
		s.client_id = conn.client_id.clone();
		s.engine = Some(convert::engine_info(&conn.info));
		s.load_snapshot(&conn.snapshot);
		s.link = Link::Connected;
		s.link_error = None;
	});
	slot.set(conn.ops.clone());

	loop {
		tokio::select! {
			item = conn.watch.next() => match item {
				Ok(Some(WatchItem::Snapshot(snapshot))) => hub.update(|s| s.load_snapshot(&snapshot)),
				Ok(Some(WatchItem::Event { revision, event })) => hub.update(|s| s.apply_event(revision, &event)),
				Ok(Some(WatchItem::Resync(r))) => return format!("engine asked for a resync: {}", r.reason),
				Ok(None) => return "the daemon closed the connection".into(),
				Err(e) => return e.to_string(),
			},
			sample = next_telemetry(&mut conn.telemetry) => match sample {
				Ok(Some(t)) => hub.set_meters(convert::meters(&t)),
				// Telemetry is best effort: stop meters, keep the session.
				Ok(None) | Err(_) => {
					conn.telemetry = None;
					hub.clear_meters();
				}
			},
		}
	}
}

async fn next_telemetry(
	source: &mut Option<Box<dyn crate::engine::TelemetrySource>>,
) -> Result<Option<relay_api::pb::TelemetrySnapshot>, EngineError> {
	match source {
		Some(source) => source.next().await,
		None => std::future::pending().await,
	}
}
