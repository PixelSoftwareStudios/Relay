//! Settings > Advanced: Reset above About (About is last), a short reset card with a red
//! button, no internal counters, and a Back button that leaves Settings.

mod phone;

use phone::{Drawn, Phone};
use relay_ui::capabilities::SettingsCategory;
use relay_ui::i18n::Language;
use std::sync::atomic::Ordering;

fn advanced() -> (Phone, Drawn) {
	let mut phone = Phone::desktop(900.0, 1400.0, Language::En);
	phone.shell.manage_daemon.store(true, Ordering::SeqCst);
	phone.show_state(&phone::busy_state());
	phone.app.open_settings(Some(SettingsCategory::Advanced));
	let drawn = phone.settle();
	(phone, drawn)
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

#[tokio::test]
async fn about_is_the_last_card() {
	let (_phone, drawn) = advanced();
	assert!(top(&drawn, "Reset Relay data") < top(&drawn, "About"));
	assert!(top(&drawn, "Relay daemon") < top(&drawn, "Reset Relay data"));
}

#[tokio::test]
async fn the_reset_card_is_short_and_has_no_revision_counter() {
	let (_phone, drawn) = advanced();
	assert!(
		!drawn
			.labels
			.iter()
			.any(|l| l.contains("Deletes Relay's database"))
	);
	assert!(!drawn.labels.iter().any(|l| l.starts_with("Revision")));
	assert!(drawn.labels.iter().any(|l| l == "Reset Relay data…"));
}

#[tokio::test]
async fn back_leaves_settings() {
	let (mut phone, drawn) = advanced();
	assert!(drawn.labels.iter().any(|l| l == "Back"));
	phone.click_label("Back");
	let after = phone.settle();
	assert!(
		!after.labels.iter().any(|l| l == "Storage and limits"),
		"{:?}",
		after.labels
	);
}
