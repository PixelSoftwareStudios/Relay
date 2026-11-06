//! The "Connect a device" section of the People page: open while nobody is
//! known, collapsed once there are people, the user's choice kept, and Copy
//! address one click away either way.

mod phone;

use phone::{Drawn, Phone};
use relay_ui::i18n::Language;
use relay_ui::layout::Page;
use relay_ui::model::ConnectionView;

const ADDRESS: &str =
	"relay://192.168.1.40:7000#abababababababababababababababababababababababababababababababab";

fn has(drawn: &Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l.contains(text))
}

/// A rig on the People page whose daemon answered with `ADDRESS`; `peers`
/// keeps or drops the seeded devices.
fn people(desktop: bool, language: Language, peers: bool) -> Phone {
	let phone = if desktop {
		Phone::desktop(720.0, 1400.0, language)
	} else {
		Phone::new(360.0, 1800.0, language)
	};
	let mut state = phone::busy_state();
	if !peers {
		state.peers.clear();
		state.calls.clear();
	}
	phone.show_state(&state);
	phone
		.actions
		.shared
		.inbox
		.set_connection(Ok(ConnectionView {
			addresses: vec!["192.168.1.40".into()],
			port: 7000,
			fingerprint: "ab".repeat(32),
			connection_string: ADDRESS.into(),
		}));
	phone
}

fn answer(phone: &Phone) {
	phone
		.actions
		.shared
		.inbox
		.set_connection(Ok(ConnectionView {
			addresses: vec![],
			port: 7000,
			fingerprint: "ab".repeat(32),
			connection_string: ADDRESS.into(),
		}));
}

fn open_body(drawn: &Drawn) -> bool {
	has(drawn, "Share this string")
}

#[tokio::test]
async fn the_section_is_open_with_nobody_known() {
	for desktop in [true, false] {
		let mut phone = people(desktop, Language::En, false);
		let drawn = phone.page(Page::People);
		assert!(has(&drawn, "Connect a device"), "{:?}", drawn.labels);
		assert!(open_body(&drawn), "desktop={desktop}: {:?}", drawn.labels);
		assert!(has(&drawn, "Scan this code"), "the QR code is shown");
		assert!(
			has(&drawn, "Add a device by address"),
			"paste field beside it"
		);
	}
}

#[tokio::test]
async fn the_section_is_collapsed_with_people_but_copy_stays() {
	for desktop in [true, false] {
		let mut phone = people(desktop, Language::En, true);
		let drawn = phone.page(Page::People);
		assert!(has(&drawn, "Connect a device"));
		assert!(!open_body(&drawn), "desktop={desktop}: {:?}", drawn.labels);
		assert!(!has(&drawn, "Scan this code"));
		assert!(has(&drawn, "Copy address"), "{:?}", drawn.labels);
		assert!(has(&drawn, "Add a device by address"));
	}
}

#[tokio::test]
async fn copy_address_copies_the_connection_string_in_both_states() {
	for peers in [false, true] {
		let mut phone = people(true, Language::En, peers);
		phone.page(Page::People);
		phone.click_label("Copy address");
		assert_eq!(
			*phone.shell.copied.lock().unwrap(),
			vec![ADDRESS.to_owned()]
		);
	}
}

#[tokio::test]
async fn the_users_choice_is_kept_across_pages() {
	let mut phone = people(true, Language::En, true);
	phone.page(Page::People);
	phone.click_label("Connect a device");
	let drawn = phone.settle();
	assert!(open_body(&drawn) && has(&drawn, "Scan this code"));
	phone.page(Page::Calls);
	// Leaving the page dropped the string; the daemon answers again on return.
	phone.app.set_page(Page::People);
	answer(&phone);
	let back = phone.settle();
	assert!(open_body(&back), "still open after leaving the page");
}
