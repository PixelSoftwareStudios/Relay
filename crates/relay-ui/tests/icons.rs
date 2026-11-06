//! The icon set: every codepoint is in the bundled icon font, icon buttons
//! meet the touch target size, and the helpers draw without tofu.

use egui::{Context, RawInput, Rect, vec2};
use relay_ui::fonts::{self, PHOSPHOR_ICONS, PHOSPHOR_ICONS_FILL};
use relay_ui::i18n::Language;
use relay_ui::icons::{self, Icon};
use relay_ui::layout::{LayoutClass, MIN_TOUCH_TARGET};
use skrifa::{FontRef, MetadataProvider};

fn charmap_has(data: &'static [u8], c: char) -> bool {
	FontRef::new(data)
		.expect("icon font parses")
		.charmap()
		.map(c)
		.is_some()
}

#[test]
fn every_icon_codepoint_is_in_the_icon_font() {
	for icon in Icon::ALL {
		let face = if icon == Icon::StarFilled {
			PHOSPHOR_ICONS_FILL
		} else {
			PHOSPHOR_ICONS
		};
		assert!(
			charmap_has(face.data, icon.codepoint()),
			"{icon:?} U+{:04X} missing",
			icon.codepoint() as u32
		);
	}
}

#[test]
fn icon_codepoints_are_unique_per_family() {
	let mut seen = std::collections::HashSet::new();
	for icon in Icon::ALL {
		if icon == Icon::StarFilled {
			continue;
		}
		assert!(
			seen.insert(icon.codepoint()),
			"{icon:?} repeats a codepoint"
		);
	}
}

#[test]
fn icons_use_their_own_families() {
	let defs = fonts::definitions(Language::En);
	for icon in Icon::ALL {
		let faces = defs.families.get(&icon.family()).expect("family exists");
		assert_eq!(faces.len(), 1, "icons never fall back through text fonts");
	}
}

/// A context with the fonts and the style of a layout class installed.
fn context(class: LayoutClass) -> Context {
	let ctx = Context::default();
	fonts::install(&ctx, Language::En);
	relay_ui::theme::install(&ctx, class);
	ctx
}

fn run_button(class: LayoutClass, add: impl Fn(&mut egui::Ui) -> egui::Response) -> Rect {
	let ctx = context(class);
	let mut rect = Rect::NOTHING;
	for _ in 0..2 {
		let input = RawInput {
			screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(360.0, 800.0))),
			..Default::default()
		};
		let mut out = ctx.run_ui(input, |ui| {
			rect = add(ui).rect;
		});
		out.textures_delta.clear();
	}
	rect
}

#[test]
fn icon_buttons_are_44_points_on_touch_and_32_with_a_pointer() {
	for icon in Icon::ALL {
		let touch = run_button(LayoutClass::Compact, |ui| {
			icons::icon_button(ui, icon, "Tip")
		});
		assert!(touch.width() >= MIN_TOUCH_TARGET && touch.height() >= MIN_TOUCH_TARGET);
		let desktop = run_button(LayoutClass::Regular, |ui| {
			icons::icon_button(ui, icon, "Tip")
		});
		assert_eq!(desktop.size(), vec2(icons::DESKTOP_HIT, icons::DESKTOP_HIT));
	}
	let toggle = run_button(LayoutClass::Compact, |ui| {
		icons::icon_toggle(ui, true, Icon::Star, "Tip")
	});
	assert!(toggle.width() >= MIN_TOUCH_TARGET);
	let danger = run_button(LayoutClass::Compact, |ui| {
		icons::icon_button_danger(ui, Icon::HangUp, "Tip")
	});
	assert!(danger.height() >= MIN_TOUCH_TARGET);
}

#[test]
fn icon_label_buttons_fit_touch_rules() {
	let rect = run_button(LayoutClass::Compact, |ui| {
		icons::icon_label(ui, Icon::Call, "Call")
	});
	assert!(rect.height() >= MIN_TOUCH_TARGET);
}

#[test]
#[should_panic(expected = "needs a tooltip")]
fn an_empty_tooltip_is_a_bug() {
	run_button(LayoutClass::Regular, |ui| {
		icons::icon_button(ui, Icon::Call, "")
	});
}

#[test]
fn glyphs_are_laid_out_without_the_missing_glyph_box() {
	// A glyph missing from its family would be drawn from no font at all and
	// lay out with zero width.
	let ctx = context(LayoutClass::Regular);
	let mut out = ctx.run_ui(RawInput::default(), |ui| {
		for icon in Icon::ALL {
			let galley = ui.painter().layout_no_wrap(
				icon.codepoint().to_string(),
				icon.font(16.0),
				egui::Color32::WHITE,
			);
			assert!(galley.size().x > 4.0, "{icon:?} has no advance");
		}
	});
	out.textures_delta.clear();
}
