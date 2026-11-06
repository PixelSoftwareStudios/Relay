//! The chat of a call is part of the call: the call card's Messages button
//! shows it below the card (desktop) or as a full screen with a Back to the
//! call (phone), Esc closes it, unread messages show as a badge and never open
//! it, and an incoming call still surfaces while it is open.

mod chatkit;
mod phone;

use chatkit::{ada_call, has, has_exact, incoming, live_messages, state};
use std::time::Duration;

use egui::{Event, Key, Modifiers};
use phone::Phone;
use relay_ui::i18n::Language;

fn esc() -> Event {
	Event::Key {
		key: Key::Escape,
		physical_key: None,
		pressed: true,
		repeat: false,
		modifiers: Modifiers::NONE,
	}
}

fn desktop(language: Language, ringing: bool) -> Phone {
	let phone = Phone::desktop(720.0, 2600.0, language);
	phone.show_state(&state(ringing));
	live_messages(&phone);
	phone
}

fn handset(language: Language, ringing: bool) -> Phone {
	let phone = Phone::new(360.0, 800.0, language);
	phone.show_state(&state(ringing));
	live_messages(&phone);
	phone
}

#[tokio::test]
async fn the_call_card_shows_the_chat_below_it_and_toggles_it_on_desktop() {
	let mut phone = desktop(Language::En, false);
	let before = phone.settle();
	assert!(has_exact(&before, "Hang up") || has(&before, "End call"));
	assert!(!phone::has_icon(&before, relay_ui::icons::Icon::Send), "closed by default");
	phone.click_label(&chatkit::badge(&phone));
	assert_eq!(phone.app.chat_call(), Some(ada_call().as_str()));
	let open = phone.settle();
	assert!(phone::has_icon(&open, relay_ui::icons::Icon::Send), "{:?}", open.labels);
	assert!(has(&open, "Message (Enter sends"), "the composer is there");
	// The card is still on the page: the chat is part of the call, not another page.
	assert!(has(&open, "End call"));
	assert!(!has(&open, "Back to the call"), "no full screen on desktop");
	phone.click_label("Messages");
	assert_eq!(phone.app.chat_call(), None);
	assert!(!phone::has_icon(&phone.settle(), relay_ui::icons::Icon::Send));
}

#[tokio::test]
async fn the_call_card_opens_a_full_screen_chat_with_a_way_back_on_a_phone() {
	let mut phone = handset(Language::En, false);
	let calls = phone.settle();
	assert_eq!(phone.tabs(&calls).len(), 5, "five tabs, none for messages");
	assert!(has_exact(&calls, "Contacts") && !has_exact(&calls, "Messages"));
	phone.click_label(&chatkit::badge(&phone));
	let chat = phone.settle();
	assert!(phone::has_icon(&chat, relay_ui::icons::Icon::Send), "{:?}", chat.labels);
	assert!(
		phone::has_icon(&chat, relay_ui::icons::Icon::Back),
		"a way back: {:?}",
		chat.labels
	);
	assert!(!has(&chat, "End call"), "the chat replaces the page");
	assert!(
		!has_exact(&chat, "Contacts"),
		"the tabs make room for the chat"
	);
	phone.click_icon(relay_ui::icons::Icon::Back);
	assert_eq!(phone.app.chat_call(), None);
	let back = phone.settle();
	assert!(has(&back, "End call"));
	assert_eq!(phone.tabs(&back).len(), 5);
}

#[tokio::test]
async fn escape_closes_the_chat_in_both_layouts() {
	for mut phone in [desktop(Language::En, false), handset(Language::En, false)] {
		phone.app.open_chat(&ada_call());
		phone.settle();
		assert!(phone.app.chat_call().is_some());
		phone.frame_with(vec![esc()]);
		assert_eq!(phone.app.chat_call(), None);
	}
}

#[tokio::test]
async fn unread_messages_show_as_a_badge_and_never_open_the_chat() {
	let mut phone = desktop(Language::En, false);
	let first = phone.settle();
	assert!(
		has_exact(&first, &chatkit::badge(&phone)),
		"{:?}",
		first.labels
	);
	// Opening the chat reads everything.
	phone.click_label(&chatkit::badge(&phone));
	phone.settle();
	phone.click_label("Messages");
	let read = phone.settle();
	assert!(has_exact(&read, "Messages"), "{:?}", read.labels);
	assert!(!has(&read, "Messages ("));
	// A new message while the chat is closed: a badge, and nothing opens.
	incoming(&phone, 500);
	let badge = phone.settle();
	assert!(has_exact(&badge, "Messages (1)"), "{:?}", badge.labels);
	assert_eq!(phone.app.chat_call(), None);
	assert!(!phone::has_icon(&badge, relay_ui::icons::Icon::Send));
	// A message while the chat is open is read at once.
	phone.click_label("Messages (1)");
	incoming(&phone, 501);
	let open = phone.settle();
	assert!(phone::has_icon(&open, relay_ui::icons::Icon::Send));
	phone.click_label(&chat_glyph());
	assert!(has_exact(&phone.settle(), &chat_glyph()));
}

#[tokio::test]
async fn an_incoming_call_still_surfaces_while_the_chat_is_open() {
	for mut phone in [desktop(Language::En, true), handset(Language::En, true)] {
		phone.app.open_chat(&ada_call());
		let drawn = phone.settle();
		assert!(phone::has_icon(&drawn, relay_ui::icons::Icon::Send), "{:?}", drawn.labels);
		assert!(
			has_exact(&drawn, "Accept") && has_exact(&drawn, "Decline"),
			"{:?}",
			drawn.labels
		);
	}
}

/// How long the last frame asked to wait before the next repaint.
fn repaint_delay(phone: &mut Phone) -> Duration {
	let input = egui::RawInput {
		screen_rect: Some(phone.screen()),
		..Default::default()
	};
	let app = &mut phone.app;
	let out = phone.ctx.run_ui(input, |ui| {
		app.begin_frame(ui.ctx());
		app.ui(ui);
	});
	out.viewport_output
		.get(&egui::ViewportId::ROOT)
		.map_or(Duration::MAX, |v| v.repaint_delay)
}

#[tokio::test]
async fn an_open_chat_adds_no_repaints_to_the_call_screen() {
	for mut phone in [desktop(Language::En, false), handset(Language::En, false)] {
		for _ in 0..8 {
			phone.frame_with(vec![]);
		}
		let closed = repaint_delay(&mut phone);
		phone.app.open_chat(&ada_call());
		for _ in 0..12 {
			phone.frame_with(vec![]);
		}
		assert!(repaint_delay(&mut phone) >= closed, "{:?}", phone.size);
	}
}
