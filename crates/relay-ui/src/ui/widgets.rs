//! Small drawing helpers.

use egui::{self, Color32, Response, RichText, Ui};

use crate::format::{Tone, format_dbfs};
use crate::icons::{Icon, icon_inline};
use crate::theme;
use crate::tr;

pub fn tone_color(tone: Tone, ui: &Ui) -> Color32 {
	match tone {
		Tone::Live => theme::LIVE,
		Tone::Warn => theme::WARN,
		Tone::Idle => ui.visuals().text_color(),
	}
}

/// Round avatar from an already decoded texture.
pub fn avatar(ui: &mut Ui, texture: &egui::TextureHandle, size: f32) {
	let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
	egui::Image::new(texture)
		.fit_to_exact_size(rect.size())
		.corner_radius(size / 2.0)
		.paint_at(ui, rect);
}

/// Level bar. Not animated: it only changes when new data arrives.
pub fn meter(ui: &mut Ui, dbfs: Option<f32>, label: &str) -> Response {
	let bar = egui::ProgressBar::new(relay_client::meter::fill(dbfs))
		.desired_width(120.0)
		.animate(false);
	let text = tr!("meter-hover", label = label, level = format_dbfs(dbfs));
	let response = ui.add(bar);
	if crate::layout::is_touch(ui) {
		ui.label(RichText::new(format_dbfs(dbfs)).weak().small());
		response
	} else {
		response.on_hover_text(text)
	}
}

/// Integer slider for the push-to-talk release delay, 0 to 2000 ms. The drag
/// value lives in `edit` until released; returns the value to send.
pub fn release_delay_slider(ui: &mut Ui, edit: &mut Option<u32>, current: u32) -> Option<u32> {
	let mut value = edit.unwrap_or(current);
	let response = slider_row(ui, tr!("slider-release-delay"), |ui| {
		ui.add(
			egui::Slider::new(&mut value, 0..=crate::controller::RELEASE_DELAY_MAX_MS)
				.step_by(10.0)
				.suffix(format!(" {}", tr!("unit-ms"))),
		)
	});
	if response.dragged() || (response.changed() && edit.is_some()) {
		*edit = Some(value);
	}
	if response.drag_stopped() || (response.changed() && !response.dragged()) {
		*edit = None;
		return Some(value);
	}
	None
}

/// Width for a single-line text field: the row, up to a comfortable length, so
/// a field never makes its row wider than the screen.
pub fn field_width(ui: &Ui) -> f32 {
	ui.available_width().min(280.0)
}

/// A drop-down with its label on its own line above and the button as wide as
/// the row, so neither can run past the right edge. Long selected text is cut.
pub fn combo<'a>(ui: &mut Ui, label: &str, selected: &str) -> egui::ComboBox<'a> {
	ui.label(label);
	egui::ComboBox::from_id_salt(label)
		.width(ui.available_width())
		.selected_text(selected)
}

/// A slider with its label above and the track filling the row, leaving room
/// for the value box.
pub fn slider_row(ui: &mut Ui, label: &str, slider: impl FnOnce(&mut Ui) -> Response) -> Response {
	ui.label(label);
	ui.scope(|ui| {
		// The value box is about two touch widths wide, with its spacing.
		let spacing = ui.spacing();
		let room = 2.0 * (spacing.interact_size.x + spacing.item_spacing.x);
		ui.spacing_mut().slider_width = (ui.available_width() - room).max(60.0);
		slider(ui)
	})
	.inner
}

/// A row of buttons. A pointer gets them side by side, wrapping; touch gets
/// them stacked at the full row width, so a long translated label can never
/// push a button past the right edge.
pub fn button_row(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
	if crate::layout::is_touch(ui) {
		ui.with_layout(
			egui::Layout::top_down_justified(egui::Align::Center),
			add_contents,
		);
	} else {
		ui.horizontal_wrapped(add_contents);
	}
}

/// Explain a control. A pointer gets hover text; touch has no hover, so the
/// text is shown under the control as a weak line.
pub fn hint(ui: &mut Ui, response: Response, text: &str) -> Response {
	if crate::layout::is_touch(ui) {
		ui.label(RichText::new(text).weak().small());
		response
	} else {
		response.on_hover_text(text)
	}
}

/// A large press-and-hold button that works with a finger as well as a mouse:
/// it follows the pointer being down on it, never hover. Returns whether it is
/// held now.
pub fn hold_button(ui: &mut Ui, label: &str, held: bool) -> bool {
	let height = (crate::layout::MIN_TOUCH_TARGET * 1.75).max(72.0);
	let size = egui::vec2(ui.available_width(), height);
	let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click_and_drag());
	let down = response.is_pointer_button_down_on();
	let visuals = ui.visuals();
	let fill = if down || held {
		theme::LIVE
	} else {
		visuals.widgets.inactive.weak_bg_fill
	};
	let text_color = if down || held {
		Color32::BLACK
	} else {
		visuals.text_color()
	};
	let painter = ui.painter();
	painter.rect_filled(rect, 12.0, fill);
	let galley = painter.layout(
		label.to_owned(),
		egui::FontId::proportional(20.0),
		text_color,
		rect.width() - 16.0,
	);
	painter.galley(rect.center() - galley.size() / 2.0, galley, text_color);
	down
}

/// A boxed line of status: a warning or error icon, then the text.
pub fn banner(ui: &mut Ui, text: &str, error: bool) {
	let (color, icon) = if error {
		(theme::ERROR, Icon::Error)
	} else {
		(theme::WARN, Icon::Warning)
	};
	notice(ui, icon, color, text);
}

/// A boxed line with `icon` in `color` before the text.
pub fn notice(ui: &mut Ui, icon: Icon, color: Color32, text: &str) {
	egui::Frame::group(ui.style())
		.stroke(egui::Stroke::new(1.0, color))
		.inner_margin(theme::SPACE_M)
		.show(ui, |ui| {
			ui.set_width(ui.available_width());
			ui.horizontal_top(|ui| {
				icon_inline(ui, icon, color);
				ui.add(egui::Label::new(RichText::new(text).color(color)).wrap());
			});
		});
}

/// Slider whose drag value lives in `edit` until released, so state updates
/// arriving mid-drag do not fight the user. Returns the value to send.
pub fn gain_slider(ui: &mut Ui, edit: &mut Option<f32>, current: f32, label: &str) -> Option<f32> {
	let mut value = edit.unwrap_or(current);
	let response = slider_row(ui, label, |ui| {
		ui.add(egui::Slider::new(&mut value, 0.0..=2.0).fixed_decimals(2))
	});
	if response.dragged() || (response.changed() && edit.is_some()) {
		*edit = Some(value);
	}
	if response.drag_stopped() || (response.changed() && !response.dragged()) {
		*edit = None;
		return Some(value);
	}
	None
}
