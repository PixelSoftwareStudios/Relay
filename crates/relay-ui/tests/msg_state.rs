mod msgkit;

use msgkit::*;
use relay_api::pb::{self, session_event::Event};
use relay_ui::messaging::model::{Body, Delivery, OfferStatus};
use relay_ui::messaging::state::{MAX_LOCAL_ENTRIES, MessagesState};

fn loaded() -> MessagesState {
	let mut s = MessagesState::default();
	s.load_list(&[summary(1, "Ada", true)], None);
	s.set_transcript(
		&key(1),
		&[
			text(1, false, "hi"),
			text(2, true, "yo"),
			file(3, 9, pb::OfferState::Offered, false),
		],
	);
	s
}

fn entries(s: &MessagesState) -> &[relay_ui::messaging::model::EntryView] {
	&s.transcript(&key(1)).unwrap().entries
}

#[test]
fn new_entries_append_once_in_order() {
	let mut s = loaded();
	s.apply(&new_entry(1, text(4, false, "more")));
	s.apply(&new_entry(1, text(4, false, "more")));
	s.apply(&new_entry(1, text(2, false, "old replay")));
	assert_eq!(entries(&s).len(), 4);
	assert_eq!(entries(&s)[3].ordinal, 4);
}

#[test]
fn entries_for_unknown_sessions_are_ignored() {
	let mut s = loaded();
	s.apply(&new_entry(7, text(1, false, "x")));
	assert!(s.transcript(&key(7)).is_none());
}

#[test]
fn delivery_walks_sending_delivered_failed_and_back() {
	let mut s = loaded();
	let mut sending = text(5, true, "pending");
	sending.delivery = pb::DeliveryState::Sending as i32;
	s.apply(&new_entry(1, sending));
	assert_eq!(entries(&s)[3].delivery, Delivery::Sending);
	let change = |d: pb::DeliveryState, f: &str| {
		event(Event::Delivery(pb::DeliveryChanged {
			session_id: sid(1),
			seq: 5,
			delivery: d as i32,
			failure: f.into(),
		}))
	};
	s.apply(&change(pb::DeliveryState::Failed, "timeout"));
	assert_eq!(
		(entries(&s)[3].delivery, entries(&s)[3].failure.as_str()),
		(Delivery::Failed, "timeout")
	);
	s.apply(&change(pb::DeliveryState::Sending, ""));
	s.apply(&change(pb::DeliveryState::Delivered, ""));
	assert_eq!(entries(&s)[3].delivery, Delivery::Delivered);
	assert!(entries(&s)[3].failure.is_empty());
}

#[test]
fn offers_pins_permissions_and_session_updates() {
	let mut s = loaded();
	let mut o = offer(9, pb::OfferState::InProgress, false);
	o.transferred_bytes = 500_000;
	s.apply(&event(Event::Offer(pb::OfferChanged {
		session_id: sid(1),
		ordinal: 3,
		offer: Some(o),
	})));
	let Body::File(f) = &entries(&s)[2].body else {
		panic!()
	};
	assert_eq!(
		(f.status, f.transferred),
		(OfferStatus::InProgress, 500_000)
	);
	s.apply(&event(Event::Pin(pb::PinChanged {
		session_id: sid(1),
		message: Some(pb::MessageRef {
			ordinal: 1,
			outgoing: false,
			seq: 1,
		}),
		pinned: true,
	})));
	assert!(entries(&s)[0].pinned && !entries(&s)[1].pinned);
	s.apply(&event(Event::Permissions(pb::PermissionsChanged {
		session_id: sid(1),
		permissions: Some(pb::SessionPermissions {
			allow_messages: false,
			allow_files: true,
		}),
		by_peer: true,
	})));
	assert!(!s.session(&key(1)).unwrap().perms.allow_messages);
	let mut ended = summary(1, "Ada", false);
	ended.message_count = 3;
	s.apply(&event(Event::Session(ended)));
	assert!(!s.session(&key(1)).unwrap().is_live());
	assert_eq!(s.live_sessions().count(), 0);
}

#[test]
fn clear_keeps_the_session_but_removal_and_eviction_drop_it() {
	let mut s = loaded();
	s.apply(&event(Event::Cleared(pb::SessionCleared {
		session_id: sid(1),
		removed: false,
		next_ordinal: 4,
	})));
	assert!(entries(&s).is_empty() && s.session(&key(1)).is_some());
	s.apply(&event(Event::Cleared(pb::SessionCleared {
		session_id: sid(1),
		removed: true,
		next_ordinal: 0,
	})));
	assert!(s.session(&key(1)).is_none() && s.transcript(&key(1)).is_none());
	let mut s = loaded();
	s.apply(&event(Event::Evicted(pb::HistoryEvicted {
		session_id: sid(1),
		reason: "budget".into(),
	})));
	assert!(s.sessions.is_empty());
}

#[test]
fn a_reload_replaces_transcripts_and_drops_vanished_sessions() {
	let mut s = loaded();
	s.load_list(&[summary(2, "Bo", true)], None);
	assert!(s.transcript(&key(1)).is_none());
	let before = s.transcripts.get(&key(2)).map(|t| t.generation);
	s.set_transcript(&key(2), &[text(1, false, "a")]);
	assert!(s.transcript(&key(2)).unwrap().generation > before.unwrap_or(0));
}

#[test]
fn history_held_here_is_bounded() {
	let mut s = MessagesState::default();
	s.load_list(&[summary(1, "Ada", true)], None);
	let many: Vec<_> = (1..=(MAX_LOCAL_ENTRIES as u64 + 10))
		.map(|n| text(n, false, "x"))
		.collect();
	s.set_transcript(&key(1), &many);
	let t = s.transcript(&key(1)).unwrap();
	assert_eq!(t.entries.len(), MAX_LOCAL_ENTRIES);
	assert!(t.truncated);
	s.apply(&new_entry(
		1,
		text(MAX_LOCAL_ENTRIES as u64 + 11, false, "y"),
	));
	assert_eq!(
		s.transcript(&key(1)).unwrap().entries.len(),
		MAX_LOCAL_ENTRIES
	);
}

#[test]
fn search_matches_text_and_file_names_case_insensitively() {
	let s = loaded();
	assert!(entries(&s)[0].matches("hi"));
	assert!(!entries(&s)[0].matches("zzz"));
	assert!(entries(&s)[2].matches("report"));
}
