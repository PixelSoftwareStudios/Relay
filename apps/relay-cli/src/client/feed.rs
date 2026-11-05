//! Background feeds that turn engine streams into [`Update`]s and resync on gaps.

use std::time::Duration;

use relay_api::pb::TelemetrySnapshot;
use relay_api::{ApiError, WatchItem};
use tokio::sync::mpsc::Sender;

use super::{Feed, Subscribe};
use crate::hotkey::HotkeyUpdate;

/// State of the link to the engine as the UI should show it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectionState {
	/// Receiving a consistent stream.
	Live,
	/// A gap or overflow was detected; reopening the watch.
	Resyncing(String),
	/// The stream or engine is gone; retrying.
	Lost(String),
}

/// Something the UI should apply.
#[derive(Debug, Clone)]
pub enum Update {
	/// A validated state item (snapshot or event).
	State(Box<WatchItem>),
	/// Meter and statistics sample.
	Telemetry(TelemetrySnapshot),
	/// Global push-to-talk hotkey state.
	Hotkey(HotkeyUpdate),
	/// Link state changed.
	Connection(ConnectionState),
}

/// Whether the error means "open a fresh watch" rather than "give up for now".
pub(crate) fn needs_resync(err: &ApiError) -> bool {
	matches!(
		err,
		ApiError::RevisionGap { .. }
			| ApiError::StaleRevision { .. }
			| ApiError::IncarnationChanged
			| ApiError::SubscriptionOverflow
	)
}

/// Keep a state watch open until the receiver is dropped. A fresh watch starts
/// with a snapshot, so resyncing is just reopening it.
pub async fn run_state_feed<S: Subscribe>(mut source: S, tx: Sender<Update>, retry: Duration) {
	loop {
		let outcome = match source.open_watch().await {
			Ok(mut watch) => {
				if tx
					.send(Update::Connection(ConnectionState::Live))
					.await
					.is_err()
				{
					return;
				}
				pump_state(&mut watch, &tx).await
			}
			Err(err) => Err(err),
		};
		let (state, delay) = match outcome {
			Ok(StreamEnd::Resync(reason)) => (ConnectionState::Resyncing(reason), Duration::ZERO),
			Ok(StreamEnd::Closed) => (
				ConnectionState::Lost("engine closed the stream".into()),
				retry,
			),
			Err(err) if needs_resync(&err) => {
				(ConnectionState::Resyncing(err.to_string()), Duration::ZERO)
			}
			Err(err) => (ConnectionState::Lost(err.to_string()), retry),
		};
		if tx.send(Update::Connection(state)).await.is_err() {
			return;
		}
		tokio::time::sleep(delay).await;
	}
}

enum StreamEnd {
	Resync(String),
	Closed,
}

async fn pump_state<F>(watch: &mut F, tx: &Sender<Update>) -> Result<StreamEnd, ApiError>
where
	F: Feed<Item = WatchItem>,
{
	loop {
		match watch.next().await? {
			None => return Ok(StreamEnd::Closed),
			Some(WatchItem::Resync(resync)) => return Ok(StreamEnd::Resync(resync.reason)),
			Some(item) => {
				if tx.send(Update::State(Box::new(item))).await.is_err() {
					// Receiver gone: treat as a resync that the caller never sees.
					return Err(ApiError::StreamClosed);
				}
			}
		}
	}
}

/// Keep telemetry flowing until the receiver is dropped. Telemetry is
/// best-effort: errors just reopen the stream after `retry`.
pub async fn run_telemetry_feed<S: Subscribe>(
	mut source: S,
	tx: Sender<Update>,
	meters_hz: u32,
	retry: Duration,
) {
	loop {
		if let Ok(mut stream) = source.open_telemetry(meters_hz).await {
			while let Ok(Some(sample)) = stream.next().await {
				if tx.send(Update::Telemetry(sample)).await.is_err() {
					return;
				}
			}
		}
		if tx.is_closed() {
			return;
		}
		tokio::time::sleep(retry).await;
	}
}
