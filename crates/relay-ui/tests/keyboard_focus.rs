//! A focused text field keeps focus when the soft keyboard comes and goes.
//! The tabs disappear while the keyboard is up; the fields must not be
//! renumbered by that (egui drops focus when the id changes, and the shell
//! then hides the keyboard again).

mod phone;

use std::sync::atomic::Ordering;

use phone::Phone;
use relay_ui::Insets;
use relay_ui::capabilities::SettingsCategory;
use relay_ui::i18n::Language;

/// A phone with a status bar and a gesture bar, as the real one has.
const INSETS: Insets = Insets {
	top: 28.0,
	bottom: 20.0,
	left: 0.0,
	right: 0.0,
};

fn keyboard(phone: &Phone, height: f32) {
	phone
		.shell
		.keyboard
		.store(height.to_bits(), Ordering::SeqCst);
}

fn focused(phone: &Phone) -> Option<egui::Id> {
	phone.ctx.memory(|m| m.focused())
}

#[tokio::test]
async fn a_focused_field_keeps_focus_when_the_keyboard_comes_and_goes() {
	let mut phone = Phone::new(360.0, 800.0, Language::En);
	*phone.shell.insets.lock().unwrap() = INSETS;
	phone.app.open_settings(Some(SettingsCategory::Profile));
	phone.click_label("Me");
	let field = focused(&phone).expect("the tap focuses the display name field");
	for height in [300.0, 0.0, 300.0, 0.0] {
		keyboard(&phone, height);
		phone.settle();
		assert_eq!(focused(&phone), Some(field), "keyboard {height}");
	}
}
