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

/// The chat icon of the call card's Messages button.
pub fn chat_glyph() -> String {
	relay_ui::icons::Icon::Chat.codepoint().to_string()
}

/// The text of the Messages button's unread badge while the Ada chat is closed (the count).
pub fn badge(phone: &Phone) -> String {
	let count = phone.actions.shared.messages.read(|s| {
		let key = hex::encode([ADA_CALL; 16]);
		s.transcript(&key)
			.map_or(0, |t| t.entries.iter().filter(|e| !e.outgoing).count())
	});
	count.to_string()
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

/// The fixed clock of the chat tests: twenty minutes after the first seeded message.
pub const NOW_MS: u64 = 1_760_000_000_000 + 20 * 60_000;

/// Move the pointer to `pos`, press and release.
pub fn click_at(phone: &mut Phone, pos: egui::Pos2) {
	let button = |pressed| egui::Event::PointerButton {
		pos,
		button: egui::PointerButton::Primary,
		pressed,
		modifiers: Default::default(),
	};
	phone.frame_with(vec![egui::Event::PointerMoved(pos)]);
	phone.frame_with(vec![button(true)]);
	phone.frame_with(vec![button(false)]);
}

/// Tappable rectangles that are `side` points square: the three-dot buttons.
pub fn squares(drawn: &crate::phone::Drawn, side: f32) -> Vec<egui::Rect> {
	let mut found: Vec<egui::Rect> = drawn
		.tappable
		.iter()
		.copied()
		.filter(|r| (r.width() - side).abs() < 0.6 && (r.height() - side).abs() < 0.6)
		.collect();
	found.sort_by(|a, b| a.min.y.total_cmp(&b.min.y));
	found
}

/// A phone or desktop rig with the Ada chat open on the seeded messages and a fixed clock.
pub fn open_chat(phone: &mut Phone) -> crate::phone::Drawn {
	phone.show_state(&state(false));
	live_messages(phone);
	phone
		.shell
		.now_ms
		.store(NOW_MS, std::sync::atomic::Ordering::SeqCst);
	phone.app.open_chat(&ada_call());
	phone.settle()
}

/// Replace the Ada transcript by a short conversation that ends in text: a
/// two-day-old greeting, yesterday's answer, then today's exchange with a
/// failed and a sending message of our own.
pub fn talk(phone: &Phone) {
	use relay_api::pb::DeliveryState;
	let day = 86_400_000;
	let at = |e: &mut pb::TranscriptEntry, ms: u64| {
		e.sent_at_unix_ms = ms;
		e.received_at_unix_ms = ms;
	};
	let msg = |n: u64, own: bool, text: &str, ms: u64, d: DeliveryState| {
		let mut e = pb::TranscriptEntry {
			ordinal: n,
			outgoing: own,
			seq: n,
			delivery: d as i32,
			body: Some(transcript_entry::Body::Text(text.into())),
			..Default::default()
		};
		at(&mut e, ms);
		e
	};
	let now = NOW_MS;
	let mut entries = vec![
		msg(
			1,
			false,
			"Hi! Are we still on for the demo?",
			now - 2 * day,
			DeliveryState::Delivered,
		),
		msg(
			2,
			true,
			"Yes, see you at 10.",
			now - day,
			DeliveryState::Delivered,
		),
		msg(
			3,
			false,
			"Great. Here is the **link** to the notes: https://relay.example/notes",
			now - 600_000,
			DeliveryState::Delivered,
		),
		msg(
			4,
			false,
			"Can you also send the build?",
			now - 590_000,
			DeliveryState::Delivered,
		),
		msg(
			5,
			true,
			"Sure, give me a minute.",
			now - 400_000,
			DeliveryState::Delivered,
		),
		msg(
			6,
			true,
			"Sending it over now",
			now - 380_000,
			DeliveryState::Delivered,
		),
		msg(7, false, "Thanks!", now - 120_000, DeliveryState::Delivered),
		msg(
			8,
			true,
			"Did that arrive?",
			now - 60_000,
			DeliveryState::Failed,
		),
		msg(
			9,
			true,
			"Retrying in a moment.",
			now - 30_000,
			DeliveryState::Sending,
		),
	];
	entries[7].failure = "timeout".into();
	entries[2].pinned = true;
	phone.actions.shared.messages.update(|s| {
		s.set_transcript(&hex::encode([ADA_CALL; 16]), &entries);
	});
}
