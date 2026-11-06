//! The icon-bearing pages at phone and desktop sizes: no missing-glyph boxes,
//! icons where the design wants them, icon-only controls at touch size, and
//! nothing repainting while idle.

mod phone;

use phone::{LANGUAGES, Phone, has_icon, icon_rects};
use relay_ui::fonts::{FACES, PHOSPHOR_ICONS, PHOSPHOR_ICONS_FILL};
use relay_ui::icons::Icon;
use relay_ui::layout::{MIN_TOUCH_TARGET, Page};
use skrifa::{FontRef, MetadataProvider};

fn drawable(c: char) -> bool {
	[PHOSPHOR_ICONS, PHOSPHOR_ICONS_FILL]
		.iter()
		.chain(FACES.iter())
		.any(|f| FontRef::new(f.data).unwrap().charmap().map(c).is_some())
}

#[tokio::test]
async fn no_page_paints_a_character_without_a_glyph() {
	let mut bad = Vec::new();
	for language in LANGUAGES {
		for (w, h, desktop) in [
			(360.0, 800.0, false),
			(720.0, 800.0, true),
			(320.0, 640.0, false),
		] {
			let mut phone = if desktop {
				Phone::desktop(w, h, language)
			} else {
				Phone::new(w, h, language)
			};
			for page in Page::ALL {
				let drawn = phone.page(page);
				for label in &drawn.labels {
					for c in label
						.chars()
						.filter(|c| !c.is_whitespace() && !c.is_control())
					{
						if !drawable(c) {
							bad.push(format!("{language:?} {w}x{h} {page:?}: U+{:04X}", c as u32));
						}
					}
				}
			}
		}
	}
	bad.sort();
	bad.dedup();
	assert!(bad.is_empty(), "tofu: {bad:?}");
}

#[tokio::test]
async fn navigation_and_cards_carry_icons() {
	for desktop in [true, false] {
		let mut phone = if desktop {
			Phone::desktop(720.0, 800.0, relay_ui::i18n::Language::En)
		} else {
			Phone::new(360.0, 800.0, relay_ui::i18n::Language::En)
		};
		let contacts = phone.page(Page::Contacts);
		for icon in [
			Icon::Call,
			Icon::Contacts,
			Icon::Connect,
			Icon::Channels,
			Icon::Settings,
		] {
			assert!(has_icon(&contacts, icon), "desktop={desktop} nav {icon:?}");
		}
		assert!(
			has_icon(&contacts, Icon::More),
			"the kebab replaces the More text"
		);
		assert!(
			has_icon(&contacts, Icon::Star),
			"a favorite star on each contact"
		);
		let connect = phone.page(Page::Connect);
		assert!(has_icon(&connect, Icon::Trust) || has_icon(&connect, Icon::Eye));
		let channels = phone.page(Page::Channels);
		assert!(
			has_icon(&channels, Icon::ChevronRight),
			"the sections collapse to the right"
		);
	}
}

#[tokio::test]
async fn icon_only_controls_are_touch_sized_on_a_phone() {
	let mut phone = Phone::new(360.0, 800.0, relay_ui::i18n::Language::En);
	for page in [Page::Contacts, Page::Connect, Page::Channels] {
		let drawn = phone.page(page);
		for icon in [Icon::More, Icon::Star, Icon::Copy] {
			for glyph in icon_rects(&drawn, icon) {
				let hit = drawn
					.tappable
					.iter()
					.find(|r| r.contains_rect(glyph))
					.unwrap_or_else(|| panic!("{icon:?} on {page:?} is not tappable"));
				assert!(
					hit.width() >= MIN_TOUCH_TARGET - 0.5 && hit.height() >= MIN_TOUCH_TARGET - 0.5
				);
			}
		}
	}
}

#[tokio::test]
async fn chevrons_keep_their_distance_from_the_border() {
	let mut phone = Phone::new(360.0, 800.0, relay_ui::i18n::Language::En);
	let drawn = phone.page(Page::Channels);
	for glyph in icon_rects(&drawn, Icon::ChevronRight) {
		let card = drawn
			.widgets
			.iter()
			.filter(|w| w.contains_rect(glyph) && w.width() > 200.0)
			.min_by(|a, b| a.area().total_cmp(&b.area()))
			.expect("a card around the section");
		assert!(
			card.max.x - glyph.max.x >= relay_ui::theme::SPACE_M,
			"{glyph:?} in {card:?}"
		);
	}
}

#[tokio::test]
async fn an_idle_icon_page_asks_for_no_repaint() {
	let mut phone = Phone::new(360.0, 800.0, relay_ui::i18n::Language::En);
	// A ringing call repaints for its timer; idle means no call.
	let mut state = phone::busy_state();
	state.calls.clear();
	phone.show_state(&state);
	phone.page(Page::Contacts);
	for _ in 0..12 {
		phone.frame_with(Vec::new());
	}
	let input = egui::RawInput {
		screen_rect: Some(phone.screen()),
		..Default::default()
	};
	let app = &mut phone.app;
	let mut out = phone.ctx.run_ui(input, |ui| {
		app.begin_frame(ui.ctx());
		app.ui(ui);
	});
	out.textures_delta.clear();
	let delay = out
		.viewport_output
		.get(&egui::ViewportId::ROOT)
		.map_or(std::time::Duration::MAX, |v| v.repaint_delay);
	assert!(
		delay >= std::time::Duration::from_millis(500),
		"repaint after {delay:?}"
	);
}
