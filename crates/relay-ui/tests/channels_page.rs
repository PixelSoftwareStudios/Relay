//! The Channels page: one card with the explanation and the two actions, forms revealed in
//! place, and the same actions above the list when channels exist.

mod phone;

use phone::{Drawn, Phone};
use relay_ui::i18n::Language;
use relay_ui::layout::Page;

const NONE: &str = "You are not in any channel.";
const CREATE: &str = "Create a channel";
const JOIN: &str = "Join a channel";
const LISTED: &str = "Listed: show the name to members' devices";
const NOTE: &str = "Paste an invite code, or enter a channel name or id with its password.";

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

fn top(drawn: &Drawn, text: &str) -> f32 {
	drawn
		.named
		.iter()
		.find(|(t, _)| t == text)
		.unwrap_or_else(|| panic!("{text:?} not drawn: {:?}", drawn.labels))
		.1
		.top()
}

fn empty_page(width: f32) -> Phone {
	let mut phone = Phone::new(width, 800.0, Language::En);
	let mut state = phone::busy_state();
	state.channels.clear();
	phone.show_state(&state);
	phone.app.set_page(Page::Channels);
	phone.settle();
	phone
}

#[tokio::test]
async fn empty_page_is_one_card_with_two_actions_and_hidden_forms() {
	let mut phone = empty_page(720.0);
	let drawn = phone.settle();
	assert!(has(&drawn, NONE) && has(&drawn, CREATE) && has(&drawn, JOIN));
	assert!(
		!has(&drawn, LISTED) && !has(&drawn, NOTE),
		"forms start closed"
	);
	assert!(
		drawn
			.labels
			.iter()
			.any(|l| l.contains("never turns your microphone on"))
	);
	assert!(top(&drawn, NONE) < top(&drawn, CREATE));

	phone.click_label(CREATE);
	let drawn = phone.settle();
	assert!(has(&drawn, LISTED) && !has(&drawn, NOTE));
	phone.click_label(JOIN);
	let drawn = phone.settle();
	assert!(
		has(&drawn, NOTE) && !has(&drawn, LISTED),
		"one form at a time"
	);
	phone.click_label(JOIN);
	let drawn = phone.settle();
	assert!(!has(&drawn, NOTE), "a second press closes it");
}

#[tokio::test]
async fn actions_keep_touch_size_on_the_phone() {
	let mut phone = empty_page(360.0);
	phone.click_label(CREATE);
	let drawn = phone.settle();
	let screen = phone.screen().expand(1.0);
	for r in drawn.tappable.iter().filter(|r| r.width() > 12.0) {
		assert!(
			r.height() >= 43.5 && r.width() >= 43.5,
			"small target {:?}",
			r.size()
		);
	}
	for r in &drawn.widgets {
		assert!(
			r.min.x >= screen.min.x && r.max.x <= screen.max.x,
			"overflow {r:?}"
		);
	}
}
