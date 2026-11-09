//! The "Who can ring me" selector: the Nobody option, its helper line and its wire value.

mod common;
mod phone;

use phone::Phone;
use relay_api::pb;
use relay_ui::capabilities::SettingsCategory;
use relay_ui::i18n::Language;
use relay_ui::model::{AppState, Inbound};

// Rendering reads the process-wide active language; two render cases must not interleave.
static LANGUAGE: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn state_with(inbound: Inbound) -> AppState {
	let mut state = phone::busy_state();
	let settings = state.settings.as_mut().expect("seeded settings");
	settings.inbound = inbound;
	state
}

fn labels(inbound: Inbound, language: Language) -> Vec<String> {
	let _language = LANGUAGE.lock().expect("language test lock poisoned");
	let mut phone = Phone::new(360.0, 2400.0, language);
	phone.show_state(&state_with(inbound));
	phone.app.open_settings(Some(SettingsCategory::Calls));
	phone.settle().labels
}

#[tokio::test]
async fn nobody_shows_its_name_and_helper_line() {
	let nobody = labels(Inbound::Nobody, Language::En);
	assert!(nobody.iter().any(|l| l == "Nobody"), "{nobody:?}");
	assert!(
		nobody
			.iter()
			.any(|l| l == "Nobody can ring this device. You can still call others."),
		"{nobody:?}"
	);
	let anyone = labels(Inbound::Anyone, Language::En);
	assert!(!anyone.iter().any(|l| l == "Nobody"), "{anyone:?}");
	assert!(
		anyone
			.iter()
			.any(|l| l == "Decides who can start a call with you.")
	);
}

#[tokio::test]
async fn nobody_is_translated_in_every_language() {
	let english = labels(Inbound::Nobody, Language::En);
	for language in Language::ALL {
		if language == Language::En {
			continue;
		}
		let drawn = labels(Inbound::Nobody, language);
		assert!(
			!drawn
				.iter()
				.any(|l| l == "Nobody can ring this device. You can still call others."),
			"{language:?} fell back to English"
		);
		assert_eq!(drawn.len(), english.len(), "{language:?}");
	}
}

#[test]
fn nobody_maps_to_the_wire() {
	assert_eq!(
		relay_ui::convert::inbound_to_wire(Inbound::Nobody),
		pb::InboundPolicy::Nobody
	);
}
