//! The History section starts collapsed, opens and closes by its header, and its chevron sits to
//! the left of the title; the Settings list puts its chevrons on the right, away from the edge.

mod phone;

use phone::{Drawn, Phone};
use relay_ui::i18n::Language;
use relay_ui::icons::Icon;
use relay_ui::layout::Page;
use relay_ui::model::{HistoryDirection, HistoryEntry, HistoryOutcome};

fn entry() -> HistoryEntry {
	HistoryEntry {
		id: 1,
		call_id: hex::encode([1u8; 16]),
		peer_id: hex::encode([1u8; 32]),
		peer_name: "Caller One".into(),
		fingerprint_short: String::new(),
		direction: HistoryDirection::Incoming,
		outcome: HistoryOutcome::Completed,
		started_at_ms: 1_000,
		duration_ms: 5_000,
	}
}

fn rig() -> Phone {
	let mut phone = Phone::new(360.0, 2600.0, Language::En);
	let mut state = phone::busy_state();
	state.history = vec![entry()];
	phone.show_state(&state);
	phone.page(Page::Calls);
	phone
}

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l == text)
}

fn rect_of(drawn: &Drawn, text: &str) -> egui::Rect {
	drawn
		.named
		.iter()
		.find(|(t, _)| t == text)
		.unwrap_or_else(|| panic!("no {text:?} in {:?}", drawn.labels))
		.1
}

#[tokio::test]
async fn history_is_collapsed_until_its_header_is_pressed_and_remembers_the_choice() {
	let mut phone = rig();
	let closed = phone.settle();
	assert!(has(&closed, "History"));
	assert!(!has(&closed, "Caller One") && !has(&closed, "Clear history"));
	phone.click_label("History");
	let open = phone.settle();
	assert!(has(&open, "Caller One") && has(&open, "Clear history"));
	// Another page and back: still open.
	phone.page(Page::Contacts);
	assert!(has(&phone.page(Page::Calls), "Caller One"));
	phone.click_label("History");
	assert!(!has(&phone.settle(), "Caller One"));
}

#[tokio::test]
async fn the_chevron_is_left_of_the_title_and_turns_down_when_open() {
	let mut phone = rig();
	let right = Icon::ChevronRight.codepoint().to_string();
	let down = Icon::ChevronDown.codepoint().to_string();
	let closed = phone.settle();
	let title = rect_of(&closed, "History");
	let chevron = closed
		.named
		.iter()
		.filter(|(t, r)| *t == right && (r.center().y - title.center().y).abs() < 8.0)
		.map(|(_, r)| *r)
		.next()
		.expect("a chevron on the title's line");
	assert!(chevron.max.x <= title.min.x, "left of the text: {chevron:?} {title:?}");
	phone.click_label("History");
	let open = phone.settle();
	let title = rect_of(&open, "History");
	assert!(
		open.named
			.iter()
			.any(|(t, r)| *t == down && r.max.x <= title.min.x && (r.center().y - title.center().y).abs() < 8.0),
		"{:?}",
		open.labels
	);
}

#[tokio::test]
async fn the_settings_list_chevrons_are_on_the_right_with_room_to_the_border() {
	let mut phone = Phone::new(360.0, 900.0, Language::En);
	// The first frame decides the layout; only then does Settings open on its list.
	phone.settle();
	phone.app.set_page(Page::Settings);
	let drawn = phone.settle();
	let right = Icon::ChevronRight.codepoint().to_string();
	let chevrons: Vec<_> = drawn.named.iter().filter(|(t, _)| *t == right).collect();
	assert!(chevrons.len() >= 6, "{:?}", drawn.labels);
	for (_, r) in chevrons {
		assert!(r.center().x > 300.0, "on the right: {r:?}");
		assert!(360.0 - r.max.x >= 12.0, "room to the border: {r:?}");
	}
}
