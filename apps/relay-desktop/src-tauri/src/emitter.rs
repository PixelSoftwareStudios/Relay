//! Bounded-rate delivery of host state to the UI and the tray.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::{Instant, MissedTickBehavior};

use crate::hub::Hub;
use crate::model::{AppState, Link, Meters, Phase};

/// Receiver of throttled updates (Tauri events and the tray in production).
pub trait Sink: Send + Sync + 'static {
	fn state(&self, state: &AppState);
	fn meters(&self, meters: &Meters);
	fn tray(&self, summary: &TraySummary);
}

#[derive(Debug, Clone, Copy)]
pub struct Cadence {
	/// Minimum spacing of state and meter emissions (10 Hz by default).
	pub tick: Duration,
	/// Minimum spacing of tray refreshes.
	pub tray_min: Duration,
}

impl Default for Cadence {
	fn default() -> Self {
		Self {
			tick: Duration::from_millis(100),
			tray_min: Duration::from_secs(1),
		}
	}
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraySummary {
	pub tooltip: String,
	pub incoming: usize,
}

/// One line of tray text from the current state; pure and cheap.
pub fn tray_summary(state: &AppState) -> TraySummary {
	let incoming = state
		.calls
		.iter()
		.filter(|c| c.phase == Phase::IncomingRinging)
		.count();
	let active = state
		.calls
		.iter()
		.filter(|c| !c.phase.is_terminal() && c.phase != Phase::IncomingRinging)
		.count();
	let tooltip = match state.link {
		_ if state.mock => "Relay (mock data)".to_owned(),
		Link::Connecting => "Relay: connecting to daemon".to_owned(),
		Link::Reconnecting => "Relay: daemon connection lost, reconnecting".to_owned(),
		Link::Offline => "Relay: daemon not running".to_owned(),
		Link::Connected if incoming > 0 => format!("Relay: {incoming} incoming call(s)"),
		Link::Connected if active > 0 => format!("Relay: {active} active call(s)"),
		Link::Connected => "Relay: idle".to_owned(),
	};
	TraySummary { tooltip, incoming }
}

/// Emit at most once per `tick`, coalescing intermediate changes, and refresh
/// the tray at most once per `tray_min` and only when its content changed.
pub async fn run(hub: Arc<Hub>, sink: Arc<dyn Sink>, cadence: Cadence) {
	let mut ticker = tokio::time::interval(cadence.tick);
	ticker.set_missed_tick_behavior(MissedTickBehavior::Delay);
	let mut last_tray: Option<TraySummary> = None;
	let mut last_tray_at: Option<Instant> = None;
	loop {
		ticker.tick().await;
		if let Some(state) = hub.take_state() {
			sink.state(&state);
		}
		if let Some(meters) = hub.take_meters() {
			sink.meters(&meters);
		}
		let due = last_tray_at.is_none_or(|at| at.elapsed() >= cadence.tray_min);
		if due {
			let summary = hub.read(tray_summary);
			if last_tray.as_ref() != Some(&summary) {
				sink.tray(&summary);
				last_tray = Some(summary);
			}
			last_tray_at = Some(Instant::now());
		}
	}
}
