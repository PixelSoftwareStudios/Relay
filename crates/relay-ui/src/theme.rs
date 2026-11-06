//! Colour, spacing and type tokens, and the light and dark styles. The
//! theme follows the OS hint: egui's default `ThemePreference::System` picks
//! the style that matches the platform, and both are installed here.

use egui::{Color32, Context, FontId, Style, TextStyle, Theme};

use crate::layout::LayoutClass;

/// Semantic colours. Status colours are shared by both themes.
pub const LIVE: Color32 = Color32::from_rgb(63, 185, 80);
pub const WARN: Color32 = Color32::from_rgb(210, 153, 34);
pub const ERROR: Color32 = Color32::from_rgb(248, 81, 73);

/// Spacing tokens (points).
pub const SPACE_S: f32 = 4.0;
pub const SPACE_M: f32 = 8.0;
pub const SPACE_L: f32 = 16.0;

/// Avatar sizes (points).
pub const AVATAR_ROW: f32 = 40.0;
pub const AVATAR_PROFILE: f32 = 64.0;

/// Type sizes (points) for body and heading text.
pub const TEXT_BODY: f32 = 14.0;
pub const TEXT_BODY_COMPACT: f32 = 16.0;
pub const TEXT_HEADING: f32 = 20.0;

/// Install the light and dark styles; egui then picks one from the OS hint.
/// Call again when the layout class changes.
pub fn install(ctx: &Context, class: LayoutClass) {
	for theme in [Theme::Light, Theme::Dark] {
		ctx.set_style_of(theme, style_for(theme, class));
	}
}

/// The style for a theme and layout class.
pub fn style_for(theme: Theme, class: LayoutClass) -> Style {
	let mut style = Style {
		visuals: match theme {
			Theme::Dark => egui::Visuals::dark(),
			Theme::Light => egui::Visuals::light(),
		},
		..Style::default()
	};
	let body = if class.is_compact() {
		TEXT_BODY_COMPACT
	} else {
		TEXT_BODY
	};
	let styles = &mut style.text_styles;
	styles.insert(TextStyle::Body, FontId::proportional(body));
	styles.insert(TextStyle::Button, FontId::proportional(body));
	styles.insert(TextStyle::Heading, FontId::proportional(TEXT_HEADING));
	if class.is_compact() {
		style.spacing.interact_size.y = class.min_touch();
		style.spacing.button_padding = egui::vec2(SPACE_L, SPACE_M);
		style.spacing.item_spacing = egui::vec2(SPACE_M, SPACE_M);
		style.spacing.slider_width = 160.0;
		// A touch screen has no text-selection drag; labels are not tap targets.
		style.interaction.selectable_labels = false;
	}
	style
}
