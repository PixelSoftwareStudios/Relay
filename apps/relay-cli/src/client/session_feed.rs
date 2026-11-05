//! The messages connection of the TUI: a session watch that reopens on gaps.
//!
//! A fresh watch begins with the session list; each transcript is then read from the start
//! (bounded) so nothing that happened while the stream was down is missing. The same steps
//! `docs/api.md` gives for recovery: `ListSessions` + `GetTranscript` + a fresh `WatchSession`.

use std::time::Duration;

use relay_api::{ApiError, SessionItem};
use tokio::sync::mpsc::Sender;

use super::{Feed, Messaging, Update, needs_resync};
use crate::tui::messages::{Link, MsgUpdate};

/// Pages read per session when (re)loading a transcript.
const PAGES_PER_SESSION: usize = 8;

fn send(tx: &Sender<Update>, update: MsgUpdate) -> impl Future<Output = bool> + '_ {
	let update = Update::Messages(Box::new(update));
	async move { tx.send(update).await.is_ok() }
}

enum End {
	Resync(String),
	Closed,
}

async fn load<M: Messaging>(
	source: &mut M,
	list: relay_api::pb::SessionList,
	tx: &Sender<Update>,
) -> Result<bool, ApiError> {
	let ids: Vec<Vec<u8>> = list.sessions.iter().map(|s| s.session_id.clone()).collect();
	if !send(tx, MsgUpdate::Sessions(list.sessions, list.settings)).await {
		return Ok(false);
	}
	for id in ids {
		let mut from = 0;
		for _ in 0..PAGES_PER_SESSION {
			let page = source.get_transcript(&id, from, 0).await?;
			let next = page.next_ordinal;
			if !send(tx, MsgUpdate::Transcript(id.clone(), page.entries)).await {
				return Ok(false);
			}
			if !page.more || next <= from {
				break;
			}
			from = next;
		}
	}
	Ok(true)
}

async fn pump<M: Messaging>(source: &mut M, tx: &Sender<Update>) -> Result<Option<End>, ApiError> {
	let mut watch = source.open_session_watch(None, None).await?;
	loop {
		match watch.next().await? {
			None => return Ok(Some(End::Closed)),
			Some(SessionItem::Resync(r)) => return Ok(Some(End::Resync(r.reason))),
			Some(SessionItem::List(list)) => {
				if !load(source, list, tx).await? {
					return Ok(None);
				}
				if !send(tx, MsgUpdate::Link(Link::Live)).await {
					return Ok(None);
				}
			}
			Some(SessionItem::Event { event, .. }) => {
				if !send(tx, MsgUpdate::Event(event)).await {
					return Ok(None);
				}
			}
		}
	}
}

/// Keep the session watch open until the receiver is dropped.
pub async fn run_session_feed<M: Messaging>(mut source: M, tx: Sender<Update>, retry: Duration) {
	loop {
		let (link, delay) = match pump(&mut source, &tx).await {
			Ok(None) => return,
			Ok(Some(End::Resync(reason))) => (Link::Resyncing(reason), Duration::ZERO),
			Ok(Some(End::Closed)) => (Link::Lost("the daemon closed the stream".into()), retry),
			Err(err) if needs_resync(&err) => (Link::Resyncing(err.to_string()), Duration::ZERO),
			Err(ApiError::Status(status)) if status.code() == tonic::Code::PermissionDenied => {
				let _ = send(
					&tx,
					MsgUpdate::Link(Link::Unavailable(
						"the daemon refused the messages capability".into(),
					)),
				)
				.await;
				return;
			}
			Err(err) => (Link::Lost(err.to_string()), retry),
		};
		if !send(&tx, MsgUpdate::Link(link)).await {
			return;
		}
		tokio::time::sleep(delay).await;
	}
}
