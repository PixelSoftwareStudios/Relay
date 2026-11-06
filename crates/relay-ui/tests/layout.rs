use relay_ui::layout::{COMPACT_MAX_WIDTH, LayoutClass, MIN_TOUCH_TARGET, Page};
use relay_ui::theme;

#[test]
fn phone_widths_are_compact() {
	for w in [320.0, 390.0, 412.0, COMPACT_MAX_WIDTH - 0.5] {
		assert_eq!(LayoutClass::for_width(w), LayoutClass::Compact, "{w}");
	}
}

#[test]
fn desktop_widths_are_regular() {
	for w in [COMPACT_MAX_WIDTH, 800.0, 1920.0] {
		assert_eq!(LayoutClass::for_width(w), LayoutClass::Regular, "{w}");
	}
}

#[test]
fn unlaid_out_windows_are_regular() {
	for w in [0.0, -1.0, f32::NAN, f32::INFINITY] {
		assert_eq!(LayoutClass::for_width(w), LayoutClass::Regular);
	}
}

#[test]
fn compact_style_has_touch_sized_targets() {
	for t in [egui::Theme::Light, egui::Theme::Dark] {
		let s = theme::style_for(t, LayoutClass::Compact);
		assert!(s.spacing.interact_size.y >= MIN_TOUCH_TARGET);
		let r = theme::style_for(t, LayoutClass::Regular);
		assert!(r.spacing.interact_size.y < MIN_TOUCH_TARGET);
	}
}

#[test]
fn themes_follow_their_visuals() {
	let dark = theme::style_for(egui::Theme::Dark, LayoutClass::Regular);
	let light = theme::style_for(egui::Theme::Light, LayoutClass::Regular);
	assert!(dark.visuals.dark_mode);
	assert!(!light.visuals.dark_mode);
}

#[test]
fn navigation_lists_each_page_once() {
	let labels: std::collections::HashSet<_> = Page::ALL.iter().map(|p| p.label()).collect();
	assert_eq!(labels.len(), Page::ALL.len());
	assert_eq!(Page::default(), Page::Calls);
}

#[test]
fn messages_are_not_a_page() {
	assert_eq!(
		Page::ALL,
		[
			Page::Calls,
			Page::Contacts,
			Page::Connect,
			Page::Channels,
			Page::Settings
		]
	);
	assert_eq!(
		Page::SIDEBAR,
		[Page::Calls, Page::Contacts, Page::Connect, Page::Channels]
	);
	assert!(!Page::ALL.iter().any(|p| p.label() == "Messages"));
}
