//! Colours of the icon buttons, derived from the theme tokens and the active
//! visuals. New colours for icons live here, not in `theme`.

use egui::{Color32, Response, Stroke, Ui};

use crate::theme;

/// Fill of a danger control (hang up, delete); the same red as `kit`'s button.
pub const DANGER_FILL: Color32 = Color32::from_rgb(178, 48, 42);
/// Fill of a primary affirmative control (accept, trust).
pub const PRIMARY_FILL: Color32 = Color32::from_rgb(34, 128, 56);

/// How a control reads at a glance.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tone {
	Plain,
	Danger,
}

/// What to paint for one frame of an icon button.
pub struct Paint {
	pub fill: Color32,
	pub stroke: Stroke,
	pub glyph: Color32,
}

/// Fill, outline and glyph colour from the button's state. A pressed toggle
/// gets the live green as an accent; danger buttons are red and white.
pub fn paint(ui: &Ui, response: &Response, tone: Tone, on: bool) -> Paint {
	let visuals = ui.style().interact(response);
	let enabled = ui.is_enabled();
	if tone == Tone::Danger {
		let fill = if response.hovered() {
			DANGER_FILL.gamma_multiply(1.15)
		} else {
			DANGER_FILL
		};
		return Paint {
			fill,
			stroke: Stroke::NONE,
			glyph: Color32::WHITE,
		};
	}
	let resting =
		!(response.hovered() || response.has_focus() || response.is_pointer_button_down_on());
	let fill = if on {
		ui.visuals().selection.bg_fill
	} else if resting {
		Color32::TRANSPARENT
	} else {
		visuals.weak_bg_fill
	};
	let stroke = if response.has_focus() {
		ui.visuals().selection.stroke
	} else {
		Stroke::NONE
	};
	let glyph = if !enabled {
		ui.visuals().weak_text_color()
	} else if on {
		ui.visuals().selection.stroke.color
	} else {
		visuals.text_color()
	};
	Paint {
		fill,
		stroke,
		glyph,
	}
}

/// Colour of an informational line's icon.
pub fn info_color(ui: &Ui) -> Color32 {
	ui.visuals().hyperlink_color
}

/// Colour of a warning line's icon.
pub fn warn_color() -> Color32 {
	theme::WARN
}

/// Colour of an error line's icon.
pub fn error_color() -> Color32 {
	theme::ERROR
}

/// Colour of a good-news line's icon.
pub fn ok_color() -> Color32 {
	theme::LIVE
}
