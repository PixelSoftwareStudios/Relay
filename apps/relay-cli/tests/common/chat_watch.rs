//! The `WatchSession` stream of the socket engine: a list or a replay, then live events.

use relay_api::pb::session_event::Event;
use relay_api::pb::*;

use super::chat::ChatState;

fn event_session(event: &SessionEvent) -> Vec<u8> {
	match event.event.as_ref() {
		Some(Event::Entry(e)) => e.session_id.clone(),
		Some(Event::Delivery(e)) => e.session_id.clone(),
		Some(Event::Offer(e)) => e.session_id.clone(),
		Some(Event::Permissions(e)) => e.session_id.clone(),
		Some(Event::Pin(e)) => e.session_id.clone(),
		Some(Event::Session(e)) => e.session_id.clone(),
		Some(Event::Cleared(e)) => e.session_id.clone(),
		Some(Event::Evicted(e)) => e.session_id.clone(),
		None => Vec::new(),
	}
}

/// The `WatchSession` stream: a list or a replay, then new events as they appear.
pub async fn serve_watch(
	chat: std::sync::Arc<std::sync::Mutex<ChatState>>,
	incarnation: Vec<u8>,
	after: Option<u64>,
	only: Vec<u8>,
	tx: tokio::sync::mpsc::Sender<Result<SessionEnvelope, tonic::Status>>,
) {
	use relay_api::pb::session_envelope::Body;
	let envelope = |revision, body| {
		Ok(SessionEnvelope {
			engine_incarnation: incarnation.clone(),
			revision,
			body: Some(body),
		})
	};
	let mut cursor = 0;
	let (resync, first) = {
		let mut c = chat.lock().unwrap();
		let resync = std::mem::take(&mut c.resync_next_watch);
		match after {
			None => {
				cursor = c.revision;
				let mut list = c.list();
				list.sessions
					.retain(|s| only.is_empty() || s.session_id == only);
				let body = Body::List(SessionList {
					sessions: list.sessions,
					settings: list.settings,
				});
				(resync, Some(envelope(cursor, body)))
			}
			Some(rev) if rev < c.replay_floor => {
				let body = Body::Resync(SessionResync {
					current_revision: c.revision,
					reason: "window".into(),
				});
				(true, Some(envelope(c.revision, body)))
			}
			Some(rev) => {
				cursor = rev;
				(resync, None)
			}
		}
	};
	if let Some(first) = first
		&& tx.send(first).await.is_err()
	{
		return;
	}
	if resync {
		let body = Body::Resync(SessionResync {
			current_revision: 0,
			reason: "lagged".into(),
		});
		let _ = tx.send(envelope(0, body)).await;
		return;
	}
	loop {
		let due: Vec<(u64, SessionEvent)> = {
			let c = chat.lock().unwrap();
			c.events
				.iter()
				.filter(|(r, e)| *r > cursor && (only.is_empty() || event_session(e) == only))
				.cloned()
				.collect()
		};
		for (rev, event) in due {
			cursor = rev;
			if tx.send(envelope(rev, Body::Event(event))).await.is_err() {
				return;
			}
		}
		if tx.is_closed() {
			return;
		}
		tokio::time::sleep(std::time::Duration::from_millis(10)).await;
	}
}
