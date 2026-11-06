//! A call in the history has a Messages button. It is enabled when the daemon
//! still holds that call's transcript, disabled with an explanation when it
//! does not, and opens the transcript read only in the same chat panel.

mod chatkit;
mod phone;

use chatkit::{alan_call, has, has_exact, history_state, is_tappable, live_messages};
use egui::{Event, Key, Modifiers};
use phone::Phone;
use relay_ui::i18n::Language;

const GONE: &str = "Messages from this call are no longer kept";

fn desktop() -> Phone {
	let phone = Phone::desktop(720.0, 3200.0, Language::En);
	phone.show_state(&history_state());
	phone
}

#[tokio::test]
async fn the_button_follows_whether_the_daemon_holds_the_transcript() {
	let mut phone = desktop();
	live_messages(&phone);
	let drawn = phone.settle();
	assert!(has_exact(&drawn, "Messages: 3000"), "{:?}", drawn.labels);
	assert!(is_tappable(&phone, &drawn, "Messages: 3000"));
	assert!(
		!is_tappable(&phone, &drawn, "Messages"),
		"the other row is off"
	);
	assert!(
		has(&drawn, GONE),
		"inline text says why: {:?}",
		drawn.labels
	);
	// The disabled button does nothing.
	let (_, rect) = drawn.named.iter().find(|(t, _)| t == "Messages").unwrap();
	let pos = rect.center();
	let button = |pressed| Event::PointerButton {
		pos,
		button: egui::PointerButton::Primary,
		pressed,
		modifiers: Modifiers::NONE,
	};
	phone.frame_with(vec![Event::PointerMoved(pos)]);
	phone.frame_with(vec![button(true)]);
	phone.frame_with(vec![button(false)]);
	assert_eq!(phone.app.chat_call(), None);
}

#[tokio::test]
async fn the_button_is_on_while_messaging_is_not_connected_yet() {
	let mut phone = desktop();
	let drawn = phone.settle();
	assert!(
		!has(&drawn, GONE),
		"nothing is known yet: {:?}",
		drawn.labels
	);
	assert!(is_tappable(&phone, &drawn, "Messages"));
	// Opening it connects messaging, as opening any chat does.
	assert!(!phone.actions.shared.messages.enabled());
	phone.app.open_chat(&alan_call());
	phone.settle();
	assert!(phone.actions.shared.messages.enabled());
}

#[tokio::test]
async fn the_button_opens_the_transcript_read_only_below_the_row() {
	let mut phone = desktop();
	live_messages(&phone);
	phone.click_label("Messages: 3000");
	assert_eq!(phone.app.chat_call(), Some(alan_call().as_str()));
	let open = phone.settle();
	assert!(
		has(&open, "Read only: this call has ended."),
		"{:?}",
		open.labels
	);
	assert!(
		has(&open, "This call has ended"),
		"the composer is replaced"
	);
	assert!(has(&open, "Export JSON"));
	assert!(!has(&open, "Offer file") && !has(&open, "Message (Enter sends"));
	assert!(
		has(&open, "Message 3000"),
		"the newest message is at the bottom"
	);
	// The row stays; its button closes the chat again.
	assert!(has(&open, "Call back"));
	phone.click_label("Messages: 3000");
	assert_eq!(phone.app.chat_call(), None);
	// Esc closes it as well.
	phone.app.open_chat(&alan_call());
	phone.settle();
	phone.frame_with(vec![Event::Key {
		key: Key::Escape,
		physical_key: None,
		pressed: true,
		repeat: false,
		modifiers: Modifiers::NONE,
	}]);
	assert_eq!(phone.app.chat_call(), None);
}

#[tokio::test]
async fn on_a_phone_the_history_chat_is_a_screen_with_a_back() {
	// Tall, so the History section is on screen without scrolling.
	let mut phone = Phone::new(360.0, 5000.0, Language::En);
	phone.show_state(&history_state());
	live_messages(&phone);
	phone.click_label("Messages: 3000");
	let open = phone.settle();
	assert!(
		has(&open, "Read only: this call has ended."),
		"{:?}",
		open.labels
	);
	assert!(has_exact(&open, "\u{2039} Back"));
	assert!(has(&open, "Kept Caller"));
	assert!(!has_exact(&open, "Contacts"), "the tabs are out of the way");
	phone.click_label("\u{2039} Back");
	assert_eq!(phone.app.chat_call(), None);
	let back = phone.settle();
	assert!(has_exact(&back, "Contacts") && has(&back, "History"));
}

#[tokio::test]
async fn an_incoming_call_surfaces_over_a_history_chat() {
	let mut state = history_state();
	let mut ring = state.calls[0].clone();
	ring.call_id = "03".repeat(16);
	ring.peer_id = state.peers[1].peer_id.clone();
	ring.phase = relay_ui::model::Phase::IncomingRinging;
	state.calls.push(ring);
	let mut phone = Phone::new(360.0, 800.0, Language::En);
	phone.show_state(&state);
	live_messages(&phone);
	phone.app.open_chat(&alan_call());
	let drawn = phone.settle();
	assert!(has(&drawn, "Read only"), "{:?}", drawn.labels);
	assert!(has_exact(&drawn, "Accept") && has_exact(&drawn, "Decline"));
}
