//! `relay history [--limit N]` and `relay history clear`.
//!
//! The list is metadata only (who, which way, how it ended, when, how long) and is kept on
//! this device; it never leaves it.

use std::fmt::{self, Display, Formatter};

use relay_api::pb::{CallDirection, CallHistoryEntry, CallOutcome};
use relay_io::localtime::LocalZone;
use serde::Serialize;

use super::table::{self, Layout};
use super::{Out, names, report};
use crate::cli::{CliError, HistoryArgs, HistoryCommand};
use crate::client::Manage;

/// One past call.
#[derive(Debug, Serialize)]
pub struct EntryView {
	pub id: u64,
	/// 16 bytes as hex: the id of the call, which also names its chat session.
	pub call_id: String,
	pub peer_name: String,
	/// The peer's full fingerprint as hex.
	pub peer_id: String,
	pub peer_fingerprint_short: String,
	pub direction: &'static str,
	pub outcome: &'static str,
	pub started_at_unix_ms: u64,
	pub duration_ms: u64,
	#[serde(skip)]
	when: String,
}

fn direction(value: i32) -> &'static str {
	match CallDirection::try_from(value) {
		Ok(CallDirection::Incoming) => "incoming",
		Ok(CallDirection::Outgoing) => "outgoing",
		_ => "unknown",
	}
}

fn outcome(value: i32) -> &'static str {
	match CallOutcome::try_from(value) {
		Ok(CallOutcome::Completed) => "completed",
		Ok(CallOutcome::Missed) => "missed",
		Ok(CallOutcome::Declined) => "declined",
		Ok(CallOutcome::Failed) => "failed",
		Ok(CallOutcome::Cancelled) => "cancelled",
		_ => "unknown",
	}
}

impl EntryView {
	fn new(entry: &CallHistoryEntry, zone: &LocalZone) -> Self {
		Self {
			id: entry.id,
			call_id: names::hex_id(&entry.call_id),
			peer_name: entry.peer_name.clone(),
			peer_id: hex::encode(&entry.peer_id),
			peer_fingerprint_short: entry.peer_fingerprint_short.clone(),
			direction: direction(entry.direction),
			outcome: outcome(entry.outcome),
			started_at_unix_ms: entry.started_at_unix_ms,
			duration_ms: entry.duration_ms,
			when: zone.full(entry.started_at_unix_ms),
		}
	}
}

/// `m:ss`, or `h:mm:ss` from an hour up.
fn clock(ms: u64) -> String {
	let seconds = ms / 1000;
	match seconds {
		0..=3599 => format!("{}:{:02}", seconds / 60, seconds % 60),
		_ => format!(
			"{}:{:02}:{:02}",
			seconds / 3600,
			seconds / 60 % 60,
			seconds % 60
		),
	}
}

/// The listed calls, newest first.
#[derive(Debug, Serialize)]
#[serde(transparent)]
pub struct HistoryList(pub Vec<EntryView>);

impl HistoryList {
	/// Text table, or the empty-result message.
	pub fn render(&self, layout: Layout) -> String {
		if self.0.is_empty() {
			return "no call history".into();
		}
		let mut t = table::with_header(
			layout,
			&[
				"WHEN",
				"PEER",
				"FINGERPRINT",
				"DIRECTION",
				"OUTCOME",
				"DURATION",
			],
		);
		for e in &self.0 {
			t.add_row([
				e.when.clone(),
				e.peer_name.clone(),
				e.peer_fingerprint_short.clone(),
				e.direction.to_owned(),
				e.outcome.to_owned(),
				if e.outcome == "completed" {
					clock(e.duration_ms)
				} else {
					"-".to_owned()
				},
			]);
		}
		table::render(t)
	}
}

impl Display for HistoryList {
	fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
		f.write_str(&self.render(Layout::detect()))
	}
}

pub async fn run<C: Manage>(
	client: &mut C,
	args: HistoryArgs,
	out: &mut Out<'_>,
) -> Result<(), CliError> {
	match args.command {
		Some(HistoryCommand::Clear) => report(&client.clear_call_history().await?, out),
		None => {
			let page = client.call_history(args.limit, None).await?;
			let zone = out.zone().clone();
			out.emit(&HistoryList(
				page.entries
					.iter()
					.map(|e| EntryView::new(e, &zone))
					.collect(),
			))
		}
	}
}

#[cfg(test)]
mod tests {
	use super::clock;

	#[test]
	fn durations_read_as_minutes_or_hours() {
		assert_eq!(clock(0), "0:00");
		assert_eq!(clock(61_000), "1:01");
		assert_eq!(clock(3_599_000), "59:59");
		assert_eq!(clock(3_661_000), "1:01:01");
	}
}
