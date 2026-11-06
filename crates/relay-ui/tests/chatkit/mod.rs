//! Shared set-up of the chat-in-call tests: a connected call with the seeded
//! Ada session, optionally a second call that rings, and the messaging feed.
#![allow(dead_code)]

use relay_api::pb::{self, session_event::Event, transcript_entry};
use relay_ui::messaging::mock::{ADA_CALL, ALAN_CALL, seed};
use relay_ui::messaging::state::Feed;
use relay_ui::model::{AppState, HistoryDirection, HistoryEntry, HistoryOutcome, Phase};

use crate::phone::Phone;

pub fn ada_call() -> String {
	hex::encode([ADA_CALL; 16])
}

pub fn alan_call() -> String {
	hex::encode([ALAN_CALL; 16])
}

/// The mock state with one connected call (the id of the seeded Ada session),
/// and a ringing one when asked.
pub fn state(ringing: bool) -> AppState {
	let mut state = relay_ui::mock::seed();
	let mut connected = state.calls[0].clone();
	connected.call_id = ada_call();
	connected.phase = Phase::Connected;
	let mut ring = state.calls[0].clone();
	ring.call_id = "03".repeat(16);
	ring.peer_id = state.peers[1].peer_id.clone();
	state.calls = vec![connected];
	if ringing {
		state.calls.push(ring);
	}
	state
}

/// A finished call in the history.
pub fn history_entry(id: u64, name: &str, call_id: String, peer_id: String) -> HistoryEntry {
	HistoryEntry {
		id,
		call_id,
		peer_id,
		peer_name: name.into(),
		fingerprint_short: String::new(),
		direction: HistoryDirection::Incoming,
		outcome: HistoryOutcome::Completed,
		started_at_ms: 1_793_880_000_000 - 600_000,
		duration_ms: 65_000,
	}
}

/// Opt in and fill the messaging state with the seeded sessions.
pub fn live_messages(phone: &Phone) {
	let hub = phone.actions.shared.messages.clone();
	seed(&hub);
	hub.set_enabled(true);
	hub.update(|s| s.feed = Feed::Live);
}

/// A text message arriving from the other side of the Ada session.
pub fn incoming(phone: &Phone, ordinal: u64) {
	let entry = pb::TranscriptEntry {
		ordinal,
		outgoing: false,
		seq: ordinal,
		sent_at_unix_ms: 1_700_000_000_000,
		received_at_unix_ms: 1_700_000_000_000,
		delivery: pb::DeliveryState::Delivered as i32,
		body: Some(transcript_entry::Body::Text("new text".into())),
		..Default::default()
	};
	let event = pb::SessionEvent {
		event: Some(Event::Entry(pb::NewEntry {
			session_id: vec![ADA_CALL; 16],
			entry: Some(entry),
		})),
	};
	phone.actions.shared.messages.update(|s| s.apply(&event));
}

pub fn has(drawn: &crate::phone::Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l.contains(text))
}

pub fn has_exact(drawn: &crate::phone::Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

/// The Messages button label while the Ada chat is closed: the unread count in it.
pub fn badge(phone: &Phone) -> String {
	let count = phone.actions.shared.messages.read(|s| {
		let key = hex::encode([ADA_CALL; 16]);
		s.transcript(&key)
			.map_or(0, |t| t.entries.iter().filter(|e| !e.outgoing).count())
	});
	format!("Messages ({count})")
}

/// Whether a painted text sits on an enabled, tappable control.
pub fn is_tappable(phone: &Phone, drawn: &crate::phone::Drawn, label: &str) -> bool {
	let _ = phone;
	let (_, text) = drawn
		.named
		.iter()
		.find(|(t, _)| t == label)
		.unwrap_or_else(|| panic!("no {label:?} in {:?}", drawn.labels));
	drawn.tappable.iter().any(|r| r.contains(text.center()))
}

/// The state with a finished call whose session the daemon holds (Alan) and
/// one whose session it does not.
pub fn history_state() -> AppState {
	let mut state = state(false);
	let peer = state.peers[0].peer_id.clone();
	state.history = vec![
		history_entry(2, "Kept Caller", alan_call(), peer.clone()),
		history_entry(1, "Gone Caller", "ee".repeat(16), peer),
	];
	state
}
