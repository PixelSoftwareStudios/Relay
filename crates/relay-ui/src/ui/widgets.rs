//! Small drawing helpers.

use egui::{self, Color32, Response, RichText, Ui};

use crate::format::{Tone, format_dbfs};
use crate::theme;

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
		.on_hover_text(format!("{label}: {}", format_dbfs(dbfs)))
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
	let response = ui.add(
		egui::Slider::new(&mut value, 0.0..=2.0)
			.text(label)
			.fixed_decimals(2),
	);
	if response.dragged() || (response.changed() && edit.is_some()) {
		*edit = Some(value);
	}
	if response.drag_stopped() || (response.changed() && !response.dragged()) {
		*edit = None;
		return Some(value);
	}
	None
}
