//! The TUI's messages feed: list, transcripts, events, and reopening on gaps.

mod common;

use std::time::Duration;

use common::FakeControl;
use common::chat::SESSION_A;
use relay_api::SessionItem;
use relay_api::pb::session_event::Event;
use relay_api::pb::*;
use relay_cli::client::{Update, run_session_feed};
use relay_cli::tui::Model;
use relay_cli::tui::messages::Link;
use tokio::sync::mpsc;

async fn settle(
	model: &mut Model,
	rx: &mut mpsc::Receiver<Update>,
	until: impl Fn(&Model) -> bool,
) {
	for _ in 0..200 {
		while let Ok(u) = rx.try_recv() {
			model.apply(u);
		}
		if until(model) {
			return;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	panic!(
		"feed did not reach the expected state: {:?}",
		model.messages.link
	);
}

#[tokio::test]
async fn the_feed_loads_sessions_transcripts_and_events() {
	let mut c = FakeControl::new();
	c.chat.incoming_text(&SESSION_A, "first");
	c.chat.incoming_text(&SESSION_A, "second");
	c.chat.incoming_text(&SESSION_A, "third");
	let list = c.chat.list();
	let fresh = c.chat.incoming_text(&SESSION_A, "fourth, by event");
	c.session_watches.push_back(vec![
		Ok(SessionItem::List(SessionList {
			sessions: list.sessions,
			settings: list.settings,
		})),
		Ok(SessionItem::Event {
			revision: 20,
			event: Box::new(SessionEvent {
				event: Some(Event::Entry(NewEntry {
					session_id: SESSION_A.to_vec(),
					entry: Some(fresh),
				})),
			}),
		}),
	]);
	let (tx, mut rx) = mpsc::channel(64);
	tokio::spawn(run_session_feed(c, tx, Duration::from_millis(10)));
	let mut model = Model::new();
	settle(&mut model, &mut rx, |m| {
		m.messages.pane().is_some_and(|p| p.rows.len() == 4)
	})
	.await;
	// The scripted stream ends after its events, which the pane reports honestly.
	settle(&mut model, &mut rx, |m| matches!(m.messages.link, Link::Lost(_))).await;
}

#[tokio::test]
async fn a_resync_marks_the_link_and_reloads_what_was_missed() {
	let mut c = FakeControl::new();
	let list = c.chat.list();
	c.session_watches.push_back(vec![
		Ok(SessionItem::List(SessionList {
			sessions: list.sessions.clone(),
			settings: list.settings,
		})),
		Ok(SessionItem::Resync(SessionResync {
			current_revision: 99,
			reason: "lagged".into(),
		})),
	]);
	c.on_watch_open.push_back(Box::new(|_| {}));
	c.on_watch_open.push_back(Box::new(|chat| {
		chat.incoming_text(&SESSION_A, "missed while resyncing");
	}));
	c.session_watches
		.push_back(vec![Ok(SessionItem::List(SessionList {
			sessions: list.sessions,
			settings: list.settings,
		}))]);
	let (tx, mut rx) = mpsc::channel(64);
	tokio::spawn(run_session_feed(c, tx, Duration::from_millis(10)));
	let mut model = Model::new();
	let mut saw_resync = false;
	for _ in 0..300 {
		while let Ok(u) = rx.try_recv() {
			model.apply(u);
			saw_resync |= matches!(model.messages.link, Link::Resyncing(ref r) if r == "lagged");
		}
		if model.messages.pane().is_some_and(|p| !p.rows.is_empty()) {
			break;
		}
		tokio::time::sleep(Duration::from_millis(10)).await;
	}
	assert!(saw_resync, "the pane says it is resyncing");
	assert_eq!(model.messages.pane().unwrap().rows.len(), 1);
}

#[tokio::test]
async fn a_closed_stream_shows_offline_and_is_retried() {
	let mut c = FakeControl::new();
	c.session_watches.push_back(vec![]);
	let (tx, mut rx) = mpsc::channel(64);
	tokio::spawn(run_session_feed(c, tx, Duration::from_millis(10)));
	let mut model = Model::new();
	settle(&mut model, &mut rx, |m| {
		matches!(&m.messages.link, Link::Lost(_)) || m.messages.pane().is_some()
	})
	.await;
	// The retry opens the default watch (the list), so the pane appears.
	settle(&mut model, &mut rx, |m| m.messages.pane().is_some()).await;
}
