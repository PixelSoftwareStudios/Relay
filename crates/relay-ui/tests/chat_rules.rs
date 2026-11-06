//! Which session a call's chat shows, whether a past call's transcript is
//! still kept, and the unread count.

mod msgkit;

use msgkit::*;
use relay_ui::messaging::convert;
use relay_ui::messaging::rules::{Kept, session_of_call, transcript_kept, unread_count};
use relay_ui::messaging::state::{Feed, MessagesState};

fn entry_view(ordinal: u64, outgoing: bool) -> relay_ui::messaging::model::EntryView {
	convert::entry(&text(ordinal, outgoing, "hi")).unwrap()
}

#[test]
fn a_call_shows_its_live_session_else_its_latest_ended_one() {
	let live = convert::session(&summary(1, "A", true));
	let mut old = convert::session(&summary(1, "A", false));
	old.key = "old".into();
	old.ended_ms = 10;
	let mut newer = old.clone();
	newer.key = "newer".into();
	newer.ended_ms = 20;
	let other = convert::session(&summary(2, "B", true));
	let call = live.call_id.clone();
	let all = [old.clone(), other.clone(), live.clone(), newer.clone()];
	assert_eq!(session_of_call(&all, &call).unwrap().key, live.key);
	let ended = [old, other.clone(), newer];
	assert_eq!(session_of_call(&ended, &call).unwrap().key, "newer");
	assert!(session_of_call(&[other], &call).is_none());
	assert!(session_of_call(&ended, "").is_none());
}

#[test]
fn a_past_transcript_is_kept_only_while_the_daemon_has_its_session() {
	let mut state = MessagesState::default();
	assert_eq!(transcript_kept(&state, "x"), Kept::Unknown);
	state.feed = Feed::Live;
	let mut s = convert::session(&summary(3, "C", false));
	s.message_count = 4;
	let call = s.call_id.clone();
	state.sessions.push(s);
	assert_eq!(transcript_kept(&state, &call), Kept::Yes(4));
	assert_eq!(transcript_kept(&state, &hex::encode([9u8; 16])), Kept::No);
	assert_eq!(transcript_kept(&state, ""), Kept::No);
	state.feed = Feed::Offline("down".into());
	assert_eq!(transcript_kept(&state, &call), Kept::Unknown);
}

#[test]
fn unread_counts_received_messages_after_the_last_one_seen() {
	let entries = [
		entry_view(1, false),
		entry_view(2, true),
		entry_view(3, false),
		entry_view(4, false),
	];
	assert_eq!(unread_count(&entries, None), 3, "never opened");
	assert_eq!(unread_count(&entries, Some(1)), 2);
	assert_eq!(unread_count(&entries, Some(3)), 1);
	assert_eq!(unread_count(&entries, Some(4)), 0);
	assert_eq!(unread_count(&[], None), 0);
}
