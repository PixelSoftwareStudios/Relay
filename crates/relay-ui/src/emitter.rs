//! Bounded-rate delivery of host state to the UI and the tray.

use std::sync::Arc;
use std::time::Duration;

use tokio::time::{Instant, MissedTickBehavior};

use crate::hub::Hub;
use crate::i18n::Language;
use crate::model::{AppState, Link, Meters, Phase};
use crate::tr;

/// Receiver of throttled updates (Tauri events and the tray in production).
pub trait Sink: Send + Sync + 'static {
	fn state(&self, state: &AppState);
	fn meters(&self, meters: &Meters);
	fn tray(&self, summary: &TraySummary);
}

#[derive(Debug, Clone, Copy)]
pub struct Cadence {
	/// Minimum spacing of state and meter emissions (25 Hz by default).
	pub tick: Duration,
	/// Minimum spacing of tray refreshes.
	pub tray_min: Duration,
}

impl Default for Cadence {
	fn default() -> Self {
		Self {
			tick: Duration::from_millis(40),
			tray_min: Duration::from_secs(1),
		}
	}
}

/// Which tray glyph to show.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
	Idle,
	/// At least one call is ringing or connected.
	InCall,
	/// The microphone is globally muted (takes priority over the others).
	Muted,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraySummary {
	pub tooltip: String,
	/// The language the tooltip and the tray menu are worded in; a change
	/// of language makes the summary differ so the tray is redrawn.
	pub language: Language,
	pub incoming: usize,
	pub icon: TrayState,
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
		_ if state.mock => tr!("tray-mock").to_owned(),
		Link::Starting => tr!("tray-starting").to_owned(),
		Link::Connecting => tr!("tray-connecting").to_owned(),
		Link::Reconnecting => tr!("tray-reconnecting").to_owned(),
		Link::Offline => tr!("tray-offline").to_owned(),
		Link::Connected if incoming > 0 => tr!("tray-incoming", count = incoming),
		Link::Connected if active > 0 => tr!("tray-active", count = active),
		Link::Connected => tr!("tray-idle").to_owned(),
	};
	let muted = state.settings.as_ref().is_some_and(|s| s.global_muted);
	let icon = if state.link == Link::Connected && muted {
		TrayState::Muted
	} else if incoming + active > 0 {
		TrayState::InCall
	} else {
		TrayState::Idle
	};
	TraySummary {
		tooltip,
		language: crate::i18n::active(),
		incoming,
		icon,
	}
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
