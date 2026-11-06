//! Small drawing helpers.

use egui::{self, Color32, Response, RichText, Ui};

use crate::format::{Tone, format_dbfs};
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
	ui.add(bar)
		.on_hover_text(tr!("meter-hover", label = label, level = format_dbfs(dbfs)))
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
		let room = ui.spacing().interact_size.x + ui.spacing().item_spacing.x;
		ui.spacing_mut().slider_width = (ui.available_width() - room).max(60.0);
		slider(ui)
	})
	.inner
}

pub fn banner(ui: &mut Ui, text: &str, error: bool) {
	let color = if error { theme::ERROR } else { theme::WARN };
	egui::Frame::group(ui.style())
		.stroke(egui::Stroke::new(1.0, color))
		.show(ui, |ui| {
			ui.set_width(ui.available_width());
			ui.label(RichText::new(text).color(color));
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
