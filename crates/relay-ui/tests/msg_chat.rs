//! The chat as bubbles: alignment, grouping, delivery state, the three-dot
//! menu, the jump pill, the empty state and the sizes it fits.

mod chatkit;
mod phone;

use chatkit::*;
use phone::{Drawn, LANGUAGES, Phone};
use relay_ui::i18n::Language;

fn rect_of(drawn: &Drawn, text: &str) -> egui::Rect {
	drawn
		.named
		.iter()
		.find(|(t, _)| t == text)
		.unwrap_or_else(|| panic!("no {text:?} in {:?}", drawn.labels))
		.1
}

/// A phone tall enough for the whole short conversation.
fn tall() -> (Phone, Drawn) {
	let mut phone = Phone::new(360.0, 2600.0, Language::En);
	phone.show_state(&state(false));
	live_messages(&phone);
	talk(&phone);
	phone
		.shell
		.now_ms
		.store(NOW_MS, std::sync::atomic::Ordering::SeqCst);
	phone.app.open_chat(&ada_call());
	let drawn = phone.settle();
	(phone, drawn)
}

#[tokio::test]
async fn own_bubbles_sit_right_and_theirs_left_within_eighty_percent() {
	let (_, d) = tall();
	let own = rect_of(&d, "Sure, give me a minute.");
	let theirs = rect_of(&d, "Thanks!");
	assert!(
		own.max.x > 360.0 - 40.0,
		"own text ends at the right: {own:?}"
	);
	assert!(
		theirs.min.x < 360.0 / 3.0,
		"theirs starts at the left: {theirs:?}"
	);
	assert!(theirs.max.x < 360.0 / 2.0);
	// Body text is at least the compact size.
	assert!(theirs.height() >= 12.0, "{theirs:?}");
	for (t, r) in &d.named {
		assert!(
			r.min.x >= -0.5 && r.max.x <= 360.5,
			"{t:?} overflows: {r:?}"
		);
	}
	let long = rect_of(&d, "Can you also send the build?");
	assert!(long.width() <= 360.0 * 0.8, "{long:?}");
}

#[tokio::test]
async fn consecutive_messages_group_with_one_name_and_one_time() {
	let (_, d) = tall();
	let count = |text: &str| d.named.iter().filter(|(t, _)| t == text).count();
	// The heading plus the starts of three groups of the other side.
	assert_eq!(count("Ada Lovelace"), 4, "{:?}", d.labels);
	// Messages 3 and 4, 5 and 6, and 8 and 9 share a group: one time under each.
	let times: Vec<_> = d
		.labels
		.iter()
		.filter(|l| l.ends_with(" AM") || l.ends_with(" AM · Delivered"))
		.collect();
	assert_eq!(times.len(), 6, "{times:?}");
	// Inside a group the bubbles are closer than between groups.
	let (a, b) = (
		rect_of(&d, "Sure, give me a minute."),
		rect_of(&d, "Sending it over now"),
	);
	let c = rect_of(&d, "Thanks!");
	assert!(b.min.y - a.max.y < c.min.y - b.max.y);
}

#[tokio::test]
async fn days_are_separated_and_times_use_the_three_formats() {
	let (_, d) = tall();
	let has = |t: &str| d.labels.iter().any(|l| l == t);
	assert!(has("10/07/25"), "{:?}", d.labels);
	assert!(has("Yesterday") && has("Today"));
	assert!(has("10/07/25, 9:13 AM"), "{:?}", d.labels);
	assert!(has("Yesterday at 9:13 AM · Delivered"), "{:?}", d.labels);
	assert!(has("9:12 AM"));
}

#[tokio::test]
async fn delivery_state_is_written_under_own_messages() {
	let (_, d) = tall();
	assert!(d.labels.iter().any(|l| l == "Sending…"), "{:?}", d.labels);
	assert!(has(&d, "Not delivered: the other side did not answer."));
	assert!(has_exact(&d, "Retry"));
	assert!(d.labels.iter().any(|l| l.ends_with("· Delivered")));
	assert!(
		phone::has_icon(&d, Icon::Send) && phone::has_icon(&d, Icon::Attach),
		"{:?}",
		d.labels
	);
	assert!(has(&d, "Message"));
}

#[tokio::test]
async fn the_dots_menu_opens_copies_and_closes() {
	let (mut phone, d) = tall();
	let dots = phone::icon_rects(&d, Icon::More);
	assert!(dots.len() >= 10, "a button per message: {}", dots.len());
	// The first square is the chat options in the header; "Thanks!" is message 7.
	click_at(&mut phone, dots[7].center());
	let open = phone.settle();
	assert!(has_exact(&open, "Copy text") && has_exact(&open, "Pin"));
	let retries = |d: &Drawn| d.labels.iter().filter(|l| *l == "Retry").count();
	assert_eq!(retries(&open), 1, "only the failed message's own button");
	let r = rect_of(&open, "Copy text");
	click_at(&mut phone, r.center());
	let after = phone.settle();
	assert_eq!(*phone.shell.copied.lock().unwrap(), ["Thanks!"]);
	assert!(
		!has_exact(&after, "Copy text"),
		"the menu closes on a choice"
	);
}

#[tokio::test]
async fn the_menu_has_pin_link_and_retry_where_they_apply() {
	let (mut phone, d) = tall();
	let dots = phone::icon_rects(&d, Icon::More);
	// Message 3 has a link and is pinned; message 8 failed.
	click_at(&mut phone, dots[3].center());
	let open = phone.settle();
	assert!(
		has_exact(&open, "Copy link") && has_exact(&open, "Unpin"),
		"{:?}",
		open.labels
	);
	click_at(&mut phone, rect_of(&open, "Copy link").center());
	assert_eq!(
		*phone.shell.copied.lock().unwrap(),
		["https://relay.example/notes"]
	);
	phone.settle();
	click_at(&mut phone, dots[8].center());
	let open = phone.settle();
	let retries = open.labels.iter().filter(|l| *l == "Retry").count();
	assert_eq!(retries, 2, "the menu adds Retry for a failed message");
	assert!(has_exact(&open, "Pin"));
	// Pin goes to the engine; without a connection the app says so.
	click_at(&mut phone, rect_of(&open, "Pin").center());
	tokio::time::sleep(std::time::Duration::from_millis(100)).await;
	assert!(has_exact(&phone.settle(), "Dismiss"));
}

#[tokio::test]
async fn escape_and_a_click_outside_close_the_menu() {
	let (mut phone, d) = tall();
	let dots = phone::icon_rects(&d, Icon::More);
	click_at(&mut phone, dots[7].center());
	assert!(has_exact(&phone.settle(), "Copy text"));
	click_at(&mut phone, egui::pos2(20.0, 1500.0));
	assert!(!has_exact(&phone.settle(), "Copy text"));
	click_at(&mut phone, dots[7].center());
	assert!(has_exact(&phone.settle(), "Copy text"));
	phone.frame_with(vec![egui::Event::Key {
		key: egui::Key::Escape,
		physical_key: None,
		pressed: true,
		repeat: false,
		modifiers: Default::default(),
	}]);
	assert!(!has_exact(&phone.settle(), "Copy text"));
}

#[tokio::test]
async fn no_inline_copy_or_pin_buttons_remain() {
	let (_, d) = tall();
	assert!(!has_exact(&d, "Copy") && !has_exact(&d, "Pin") && !has_exact(&d, "Unpin"));
}

#[tokio::test]
async fn an_empty_chat_says_hello() {
	let mut phone = Phone::new(360.0, 800.0, Language::En);
	phone.show_state(&state(false));
	live_messages(&phone);
	phone.app.open_chat(&hex::encode([101u8; 16]));
	let d = phone.settle();
	assert!(
		has_exact(&d, "No messages yet") && has_exact(&d, "Say hello"),
		"{:?}",
		d.labels
	);
}

#[tokio::test]
async fn scrolled_up_readers_get_a_jump_pill_for_new_messages() {
	let mut phone = Phone::new(360.0, 600.0, Language::En);
	open_chat(&mut phone);
	talk(&phone);
	let d = phone.settle();
	assert!(!has_exact(&d, "New messages"));
	// At the bottom a new message follows without a pill.
	incoming(&phone, 500);
	assert!(!has_exact(&phone.settle(), "New messages"));
	// Scroll up, then a message arrives.
	let wheel = egui::Event::MouseWheel {
		unit: egui::MouseWheelUnit::Point,
		delta: egui::vec2(0.0, 3000.0),
		modifiers: Default::default(),
		phase: egui::TouchPhase::Move,
		source: egui::MouseWheelSource::Wheel,
	};
	phone.frame_with(vec![egui::Event::PointerMoved(egui::pos2(180.0, 300.0))]);
	phone.frame_with(vec![wheel]);
	phone.settle();
	incoming(&phone, 501);
	let d = phone.settle();
	assert!(has_exact(&d, "New messages"), "{:?}", d.labels);
	assert!(is_tappable(&phone, &d, "New messages"));
	click_at(&mut phone, rect_of(&d, "New messages").center());
	let d = phone.settle();
	assert!(!has_exact(&d, "New messages"));
	assert!(
		has_exact(&d, "new text"),
		"scrolled to the newest: {:?}",
		d.labels
	);
}

#[tokio::test]
async fn it_fits_phones_and_desktop_in_three_languages() {
	for language in LANGUAGES {
		for (w, h, desktop) in [
			(360.0, 800.0, false),
			(320.0, 640.0, false),
			(720.0, 800.0, true),
		] {
			let mut phone = if desktop {
				Phone::desktop(w, h, language)
			} else {
				Phone::new(w, h, language)
			};
			phone.show_state(&state(false));
			live_messages(&phone);
			talk(&phone);
			phone
				.shell
				.now_ms
				.store(NOW_MS, std::sync::atomic::Ordering::SeqCst);
			phone.app.open_chat(&ada_call());
			let d = phone.settle();
			for (t, r) in &d.named {
				assert!(
					r.min.x >= -0.5 && r.max.x <= w + 0.5,
					"{language:?} {w}: {t:?} {r:?}"
				);
			}
			if !desktop {
				// The composer's controls are full touch targets.
				let controls: Vec<_> = d
					.tappable
					.iter()
					.filter(|r| r.max.y > h - 80.0 && r.width() < w)
					.collect();
				assert!(controls.len() >= 2, "{language:?} {w}: {controls:?}");
				for r in controls {
					assert!(r.height() >= 43.9, "{language:?} {w}: {r:?}");
				}
				for r in squares(&d, 44.0) {
					assert!(r.max.x <= w);
				}
			}
		}
	}
}
