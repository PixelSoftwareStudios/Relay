//! Connection supervisor: connect, mirror state, and rebuild everything from
//! a fresh snapshot after any gap, overflow or daemon restart.

use std::sync::Arc;
use std::time::Duration;

use relay_api::WatchItem;

use crate::autostart::{AutoStart, Decision};
use crate::controller::SessionSlot;
use crate::convert;
use crate::engine::{Connection, Engine, EngineError};
use crate::hub::Hub;
use crate::model::Link;
use crate::shell::Shell;
use crate::tr;

const BACKOFF_BASE: Duration = Duration::from_millis(250);
const BACKOFF_MAX: Duration = Duration::from_secs(5);
/// How often a service that waits for the keychain prompt is asked again.
const STARTUP_POLL: Duration = Duration::from_secs(1);

/// Exponential delay between connection attempts, capped.
pub fn backoff(attempt: u32) -> Duration {
	BACKOFF_BASE
		.saturating_mul(1u32.checked_shl(attempt).unwrap_or(u32::MAX))
		.min(BACKOFF_MAX)
}

/// Run forever; drop or abort the task to stop.
pub async fn supervise(
	engine: Arc<dyn Engine>,
	hub: Arc<Hub>,
	slot: Arc<SessionSlot>,
	shell: Arc<dyn Shell>,
	autostart: Arc<AutoStart>,
) {
	let mut attempt = 0u32;
	let mut had_session = false;
	loop {
		match engine.connect().await {
			Ok(connection) => {
				attempt = 0;
				had_session = true;
				let reason = mirror(connection, &hub, &slot, &autostart).await;
				slot.clear();
				hub.clear_meters();
				hub.update(|s| s.set_link_down(Link::Reconnecting, Some(reason)));
			}
			Err(EngineError::Starting(startup)) => {
				// The service is alive: never start or restart it, only show what it waits for.
				hub.update(|s| {
					s.set_link_down(Link::Starting, None);
					s.startup = Some(startup);
				});
				tokio::time::sleep(STARTUP_POLL).await;
				continue;
			}
			Err(error) => {
				let started = if matches!(error, EngineError::Unavailable(_)) {
					start_if_wanted(&hub, shell.as_ref(), &autostart).await
				} else {
					Started::No
				};
				match started {
					Started::Retry => continue,
					Started::Shown => {
						tokio::time::sleep(BACKOFF_MAX).await;
						continue;
					}
					Started::No => {}
				}
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

/// The clock the policy runs on; tokio's, so tests can pause it.
fn now() -> std::time::Instant {
	tokio::time::Instant::now().into_std()
}

/// Says why automatic starts stopped, and keeps saying it.
fn gave_up(hub: &Hub, autostart: &AutoStart) -> Started {
	let why = autostart
		.last_failure()
		.unwrap_or_else(|| tr!("daemon-stopped-again").to_owned());
	let text = tr!("daemon-keeps-stopping", why = why);
	hub.update(|s| s.set_link_down(Link::Offline, Some(text)));
	Started::Shown
}

/// What trying to start the daemon led to.
enum Started {
	/// Nothing was started; show the connect error as usual.
	No,
	/// Try to connect again now.
	Retry,
	/// The link text already says why; wait before looking again.
	Shown,
}

/// Starts the daemon when the policy allows, showing the app as "starting"
/// meanwhile.
async fn start_if_wanted(hub: &Hub, shell: &dyn Shell, autostart: &AutoStart) -> Started {
	if !shell.can_manage_daemon() {
		return Started::No;
	}
	match autostart.decide(now()) {
		Decision::Idle if autostart.gave_up() => gave_up(hub, autostart),
		Decision::Idle => Started::No,
		Decision::Wait(delay) => {
			hub.update(|s| s.set_link_down(Link::Offline, None));
			tokio::time::sleep(delay).await;
			Started::Retry
		}
		Decision::Start => {
			hub.update(|s| s.set_link_down(Link::Starting, None));
			let outcome = shell.start_daemon().await;
			autostart.record_start(now(), &outcome);
			match outcome {
				Ok(_) => Started::Retry,
				Err(message) => {
					hub.update(|s| s.set_link_down(Link::Offline, Some(message)));
					Started::Shown
				}
			}
		}
		Decision::GiveUp => gave_up(hub, autostart),
	}
}

/// Mirror one session until it ends; the returned text says why.
async fn mirror(
	mut conn: Connection,
	hub: &Hub,
	slot: &SessionSlot,
	autostart: &AutoStart,
) -> String {
	if let Some(settings) = &conn.snapshot.settings {
		autostart.set_setting(settings.autostart_daemon);
	}
	hub.update(|s| {
		s.client_id = conn.client_id.clone();
		s.engine = Some(convert::engine_info(&conn.info));
		s.load_snapshot(&conn.snapshot);
		s.link = Link::Connected;
		s.link_error = None;
		s.startup = None;
	});
	slot.set(conn.ops.clone());
	let mut history_seen = None;
	refresh_history(&conn.ops, hub, &mut history_seen).await;

	loop {
		tokio::select! {
			item = conn.watch.next() => {
				match item {
					Ok(Some(WatchItem::Snapshot(snapshot))) => hub.update(|s| s.load_snapshot(&snapshot)),
					Ok(Some(WatchItem::Event { revision, event })) => hub.update(|s| s.apply_event(revision, &event)),
					Ok(Some(WatchItem::Resync(r))) => return tr!("watch-resync", reason = r.reason),
					Ok(None) => return tr!("watch-closed").into(),
					Err(e) => return e.to_string(),
				}
				refresh_history(&conn.ops, hub, &mut history_seen).await;
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

/// Reads the call history again when the engine says it changed (its revision moved), and
/// once per connection. A failed read keeps the list shown and is tried again at the next
/// change.
async fn refresh_history(
	ops: &Arc<dyn crate::engine::Operations>,
	hub: &Hub,
	seen: &mut Option<u64>,
) {
	let revision = hub.read(|s| s.settings.as_ref().map(|g| g.history_revision));
	if revision.is_none() || *seen == revision {
		return;
	}
	match ops.call_history().await {
		Ok(entries) => {
			*seen = revision;
			hub.update(|s| s.history = entries);
		}
		Err(error) => tracing::debug!(%error, "call history not read"),
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
