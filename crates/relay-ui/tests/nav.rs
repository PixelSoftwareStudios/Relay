//! Navigation: the sidebar and bottom tabs, Settings and Back, Esc and the
//! shortcuts, and an incoming call surfacing on every page.

mod phone;

use egui::{Event, Key, Modifiers};
use phone::{LANGUAGES, Phone};
use relay_ui::capabilities::SettingsCategory;
use relay_ui::i18n::Language;
use relay_ui::layout::Page;

fn key(key: Key, modifiers: Modifiers) -> Event {
	Event::Key {
		key,
		physical_key: None,
		pressed: true,
		repeat: false,
		modifiers,
	}
}

fn has(drawn: &phone::Drawn, text: &str) -> bool {
	drawn.labels.iter().any(|l| l.contains(text))
}

#[tokio::test]
async fn back_returns_to_the_page_settings_was_opened_from() {
	let mut phone = Phone::desktop(720.0, 800.0, Language::En);
	phone.page(Page::Channels);
	phone.app.open_settings(Some(SettingsCategory::Audio));
	phone.settle();
	assert_eq!(phone.app.page(), Page::Settings);
	assert_eq!(phone.app.settings_category(), Some(SettingsCategory::Audio));
	phone.app.close_settings();
	assert_eq!(phone.app.page(), Page::Channels);
	// The category is remembered for the next visit.
	phone.app.open_settings(None);
	assert_eq!(phone.app.settings_category(), Some(SettingsCategory::Audio));
}

#[tokio::test]
async fn escape_leaves_settings_on_a_wide_window() {
	let mut phone = Phone::desktop(720.0, 800.0, Language::En);
	phone.page(Page::People);
	phone.app.open_settings(None);
	phone.settle();
	phone.frame_with(vec![key(Key::Escape, Modifiers::NONE)]);
	assert_eq!(phone.app.page(), Page::People);
}

#[tokio::test]
async fn escape_steps_out_of_a_category_on_a_phone() {
	let mut phone = Phone::new(360.0, 800.0, Language::En);
	phone.app.open_settings(Some(SettingsCategory::Calls));
	phone.settle();
	let inside = phone.settle();
	assert!(has(&inside, "Who can ring me"), "{:?}", inside.labels);
	phone.frame_with(vec![key(Key::Escape, Modifiers::NONE)]);
	assert_eq!(phone.app.settings_category(), None);
	assert_eq!(phone.app.page(), Page::Settings);
	let list = phone.settle();
	assert!(has(&list, "Profile") && has(&list, "Advanced"));
}

#[tokio::test]
async fn shortcuts_switch_pages() {
	let mut phone = Phone::desktop(720.0, 800.0, Language::En);
	phone.frame_with(vec![key(Key::Num2, Modifiers::COMMAND)]);
	assert_eq!(phone.app.page(), Page::People);
	phone.frame_with(vec![key(Key::Num3, Modifiers::COMMAND)]);
	assert_eq!(phone.app.page(), Page::Channels);
	phone.frame_with(vec![key(Key::Num1, Modifiers::COMMAND)]);
	assert_eq!(phone.app.page(), Page::Calls);
	// There is no fourth page: Ctrl/Cmd+4 changes nothing.
	phone.frame_with(vec![key(Key::Num4, Modifiers::COMMAND)]);
	assert_eq!(phone.app.page(), Page::Calls);
	phone.frame_with(vec![key(Key::Num3, Modifiers::COMMAND)]);
	phone.frame_with(vec![key(Key::Comma, Modifiers::COMMAND)]);
	assert_eq!(phone.app.page(), Page::Settings);
	phone.frame_with(vec![key(Key::Escape, Modifiers::NONE)]);
	assert_eq!(phone.app.page(), Page::Channels);
}

#[tokio::test]
async fn an_incoming_call_shows_on_every_page_and_size() {
	for (desktop, w, h) in [(true, 720.0, 800.0), (false, 360.0, 800.0)] {
		let mut phone = if desktop {
			Phone::desktop(w, h, Language::En)
		} else {
			Phone::new(w, h, Language::En)
		};
		for page in Page::ALL {
			let drawn = phone.page(page);
			assert!(
				has(&drawn, "Accept") && has(&drawn, "Decline"),
				"{w}x{h} {page:?}: {:?}",
				drawn.labels
			);
		}
		phone.app.open_settings(Some(SettingsCategory::Privacy));
		let drawn = phone.settle();
		assert!(has(&drawn, "Accept"), "{w}x{h} inside Settings");
	}
}

#[tokio::test]
async fn the_wide_layout_has_a_sidebar_and_the_phone_has_tabs() {
	let mut wide = Phone::desktop(720.0, 800.0, Language::En);
	let drawn = wide.settle();
	assert_ne!(
		wide.tabs(&drawn).len(),
		Page::ALL.len(),
		"no bottom tabs when wide"
	);
	for page in Page::SIDEBAR {
		assert!(
			has(&drawn, page.label()),
			"{page:?} missing from the sidebar"
		);
	}
	let mut narrow = Phone::new(360.0, 800.0, Language::En);
	let drawn = narrow.settle();
	assert_eq!(narrow.tabs(&drawn).len(), Page::ALL.len());
}

#[tokio::test]
async fn every_category_draws_and_stays_inside_the_window() {
	let mut bad = Vec::new();
	for language in LANGUAGES {
		for (desktop, w, h) in [
			(true, 720.0, 800.0),
			(false, 360.0, 800.0),
			(false, 320.0, 640.0),
		] {
			let mut phone = if desktop {
				Phone::desktop(w, h, language)
			} else {
				Phone::new(w, h, language)
			};
			for category in SettingsCategory::ALL {
				phone.app.open_settings(Some(category));
				let drawn = phone.settle();
				let screen = phone.screen().expand(1.0);
				for r in drawn.widgets.iter().chain(&drawn.painted) {
					if r.min.x < screen.min.x || r.max.x > screen.max.x {
						bad.push(format!("{language:?} {w}x{h} {category:?}: {r:?}"));
					}
				}
				assert!(
					has(&drawn, category.label()) || !drawn.labels.is_empty(),
					"{language:?} {w}x{h} {category:?} drew nothing"
				);
			}
		}
	}
	bad.dedup();
	assert!(
		bad.is_empty(),
		"{} overflows: {:#?}",
		bad.len(),
		&bad[..bad.len().min(10)]
	);
}

#[tokio::test]
async fn the_phone_never_offers_desktop_only_actions() {
	let mut phone = Phone::new(360.0, 800.0, Language::En);
	for category in SettingsCategory::ALL {
		phone.app.open_settings(Some(category));
		let drawn = phone.settle();
		for label in &drawn.labels {
			assert!(!label.contains("keeps running"), "{category:?}: {label}");
			assert!(!label.contains("Quit this app"), "{category:?}: {label}");
			assert!(!label.contains("Hide window"), "{category:?}: {label}");
		}
	}
}

#[tokio::test]
async fn a_desktop_with_a_tray_offers_hide_and_quit_under_startup() {
	let mut phone = Phone::desktop(720.0, 800.0, Language::En);
	phone.app.open_settings(Some(SettingsCategory::Startup));
	let drawn = phone.settle();
	assert!(has(&drawn, "Hide window"), "{:?}", drawn.labels);
	assert!(has(&drawn, "Quit this app"), "{:?}", drawn.labels);
}
