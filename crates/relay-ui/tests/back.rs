//! The system Back action: closes one thing per press, outermost first, and
//! reports `false` only on Calls with nothing open (the shell then sends the
//! app to the background).

mod phone;

use phone::Phone;
use relay_ui::capabilities::SettingsCategory;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;

fn has(drawn: &phone::Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l.contains(text))
}

#[tokio::test]
async fn calls_with_nothing_open_is_not_consumed() {
	let mut phone = Phone::new(360.0, 800.0, Language::En);
	phone.settle();
	assert!(!phone.app.handle_back());
	assert_eq!(phone.app.page(), Page::Calls);
}

#[tokio::test]
async fn other_pages_go_to_calls() {
	let mut phone = Phone::new(360.0, 800.0, Language::En);
	for page in [Page::Contacts, Page::Connect, Page::Channels] {
		phone.page(page);
		assert!(phone.app.handle_back(), "{page:?}");
		assert_eq!(phone.app.page(), Page::Calls);
		assert!(!phone.app.handle_back());
	}
}

#[tokio::test]
async fn a_category_goes_to_the_list_then_to_the_page_settings_came_from() {
	let mut phone = Phone::new(360.0, 800.0, Language::En);
	phone.page(Page::Channels);
	phone.app.open_settings(Some(SettingsCategory::Calls));
	let inside = phone.settle();
	assert!(has(&inside, "Who can ring me"), "{:?}", inside.labels);
	assert!(phone.app.handle_back());
	assert_eq!(
		(phone.app.page(), phone.app.settings_category()),
		(Page::Settings, None)
	);
	let list = phone.settle();
	assert!(has(&list, "Profile") && has(&list, "Advanced"));
	assert!(phone.app.handle_back());
	assert_eq!(phone.app.page(), Page::Channels);
	assert!(phone.app.handle_back());
	assert_eq!(phone.app.page(), Page::Calls);
	assert!(!phone.app.handle_back());
}

#[tokio::test]
async fn settings_on_a_wide_window_returns_to_where_it_was_opened() {
	let mut phone = Phone::desktop(720.0, 800.0, Language::En);
	phone.page(Page::Contacts);
	phone.app.open_settings(Some(SettingsCategory::Audio));
	phone.settle();
	assert!(phone.app.handle_back());
	assert_eq!(phone.app.page(), Page::Contacts);
}

#[tokio::test]
async fn an_open_chat_closes_first() {
	let mut phone = Phone::new(360.0, 800.0, Language::En);
	phone.settle();
	phone.app.open_chat("0a0b");
	phone.settle();
	assert_eq!(phone.app.chat_call(), Some("0a0b"));
	assert!(phone.app.handle_back());
	assert_eq!(phone.app.chat_call(), None);
	assert_eq!(phone.app.page(), Page::Calls);
	assert!(!phone.app.handle_back());
}

#[tokio::test]
async fn a_confirmation_is_dismissed_before_leaving_the_page() {
	let mut phone = Phone::new(360.0, 800.0, Language::En);
	phone.page(Page::Contacts);
	phone.click_icon(relay_ui::icons::Icon::More);
	phone.click_label("Forget this device");
	let asking = phone.settle();
	assert!(has(&asking, "Contacts"));
	assert!(has(&asking, "Yes, forget"), "{:?}", asking.labels);
	assert!(phone.app.handle_back(), "the confirmation closes first");
	assert_eq!(phone.app.page(), Page::Contacts);
	let after = phone.settle();
	assert!(!has(&after, "Yes, forget"), "{:?}", after.labels);
	assert!(phone.app.handle_back());
	assert_eq!(phone.app.page(), Page::Calls);
}
