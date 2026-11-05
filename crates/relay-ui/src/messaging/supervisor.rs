//! Messaging supervisor: connect with the `messages` capability (only while
//! the user has opted in), load sessions and transcripts, follow the watch,
//! and rebuild from scratch after any gap, overflow or restart:
//! `ListSessions` -> `GetTranscript` -> `WatchSession(after_revision)`.

use std::sync::Arc;
use std::time::Duration;

use relay_api::SessionItem;
use tokio::time::Instant;

use super::engine::{MsgConnection, MsgEngine, is_denied};
use super::hub::MsgHub;
use super::state::{Feed, MAX_LOCAL_ENTRIES};
use crate::engine::EngineError;
use crate::shell::Shell;
use crate::supervisor::backoff;
use crate::tr;

/// Repaints caused by incoming events are coalesced to this spacing.
pub const FLUSH: Duration = Duration::from_millis(40);
/// How long to wait before asking again after the daemon denied the capability.
pub const DENIED_RETRY: Duration = Duration::from_secs(30);

/// Run forever; drop or abort the task to stop.
pub async fn supervise(engine: Arc<dyn MsgEngine>, hub: Arc<MsgHub>, shell: Arc<dyn Shell>) {
	let mut attempt = 0u32;
	loop {
		if !hub.enabled() {
			hub.changed().await;
			continue;
		}
		let epoch = hub.epoch();
		hub.update(|s| {
			if s.feed != Feed::Live {
				s.feed = Feed::Connecting;
			}
		});
		shell.request_repaint();
		let delay = match engine.connect().await {
			Ok(conn) => {
				attempt = 0;
				let reason = mirror(conn, &hub, shell.as_ref(), epoch).await;
				hub.clear_ops();
				if hub.epoch() == epoch {
					hub.update(|s| {
						s.forget_all();
						s.feed = Feed::Offline(reason);
					});
				}
				backoff(0)
			}
			Err(error) if is_denied(&error) => {
				fail(&hub, epoch, Feed::Denied);
				DENIED_RETRY
			}
			Err(error) => {
				fail(&hub, epoch, Feed::Offline(error.to_string()));
				attempt = attempt.saturating_add(1);
				backoff(attempt)
			}
		};
		shell.request_repaint();
		tokio::select! {
			() = tokio::time::sleep(delay) => {}
			() = hub.changed_from(epoch) => {}
		}
	}
}

fn fail(hub: &MsgHub, epoch: u64, feed: Feed) {
	if hub.epoch() == epoch {
		hub.update(|s| s.feed = feed);
	}
}

fn why(error: &EngineError) -> String {
	error.to_string()
}

/// Mirror one connection until it ends; the text says why.
async fn mirror(conn: MsgConnection, hub: &MsgHub, shell: &dyn Shell, epoch: u64) -> String {
	let MsgConnection { mut feed, ops } = conn;
	hub.set_ops(ops);
	loop {
		// Resync: everything is read again, then a fresh watch resumes at the
		// revision of the list. Replayed events are idempotent.
		let list = match feed.list().await {
			Ok(list) => list,
			Err(e) => return why(&e),
		};
		let mut loaded = Vec::with_capacity(list.sessions.len());
		for session in &list.sessions {
			let from = session
				.next_ordinal
				.saturating_sub(MAX_LOCAL_ENTRIES as u64);
			match feed.transcript(&session.session_id, from).await {
				Ok(entries) => loaded.push((hex::encode(&session.session_id), entries)),
				Err(e) => return why(&e),
			}
		}
		if hub.epoch() != epoch {
			return tr!("msg-turned-off").into();
		}
		hub.update(|s| {
			s.load_list(&list.sessions, list.settings.as_ref());
			for (key, entries) in &loaded {
				s.set_transcript(key, entries);
			}
			s.feed = Feed::Live;
		});
		shell.request_repaint();
		let mut watch = match feed.watch(list.revision).await {
			Ok(w) => w,
			Err(e) => return why(&e),
		};
		let mut flush_at: Option<Instant> = None;
		loop {
			let due = async {
				match flush_at {
					Some(at) => tokio::time::sleep_until(at).await,
					None => std::future::pending().await,
				}
			};
			tokio::select! {
				item = watch.next() => match item {
					Ok(Some(SessionItem::Event { event, .. })) => {
						hub.update(|s| s.apply(&event));
						flush_at.get_or_insert_with(|| Instant::now() + FLUSH);
					}
					Ok(Some(SessionItem::List(l))) => {
						hub.update(|s| s.load_list(&l.sessions, l.settings.as_ref()));
						flush_at.get_or_insert_with(|| Instant::now() + FLUSH);
					}
					Ok(Some(SessionItem::Resync(_))) => break,
					Ok(None) => return tr!("watch-closed").into(),
					Err(e) => return why(&e),
				},
				() = due => {
					flush_at = None;
					shell.request_repaint();
				}
				() = hub.changed() => {
					if hub.epoch() != epoch {
						return tr!("msg-turned-off").into();
					}
				}
			}
		}
	}
}
