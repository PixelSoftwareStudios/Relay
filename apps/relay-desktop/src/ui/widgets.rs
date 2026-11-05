//! Small drawing helpers.

use eframe::egui::{self, Color32, Response, RichText, Ui};

use crate::avatar::{Avatar, DEFAULT_PNG};
use crate::format::{Tone, format_dbfs};

pub fn tone_color(tone: Tone, ui: &Ui) -> Color32 {
	match tone {
		Tone::Live => Color32::from_rgb(63, 185, 80),
		Tone::Warn => Color32::from_rgb(210, 153, 34),
		Tone::Idle => ui.visuals().text_color(),
	}
}

/// The default avatar texture, decoded once per context.
fn placeholder_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
	static KEY: std::sync::LazyLock<egui::Id> =
		std::sync::LazyLock::new(|| egui::Id::unique("relay-default-avatar"));
	let key = *KEY;
	if let Some(texture) = ctx.data(|d| d.get_temp::<egui::TextureHandle>(key)) {
		return Some(texture);
	}
	let rgba = image::load_from_memory(DEFAULT_PNG).ok()?.to_rgba8();
	let size = [rgba.width() as usize, rgba.height() as usize];
	let pixels = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
	let texture = ctx.load_texture("default-avatar", pixels, egui::TextureOptions::LINEAR);
	ctx.data_mut(|d| d.insert_temp(key, texture.clone()));
	Some(texture)
}

/// Round avatar. Until a profile image is available the default image is drawn.
pub fn avatar(ui: &mut Ui, _avatar: Avatar, size: f32) {
	let (rect, _) = ui.allocate_exact_size(egui::vec2(size, size), egui::Sense::hover());
	if let Some(texture) = placeholder_texture(ui.ctx()) {
		egui::Image::new(&texture)
			.fit_to_exact_size(rect.size())
			.corner_radius(size / 2.0)
			.paint_at(ui, rect);
	}
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
	let color = if error {
		Color32::from_rgb(248, 81, 73)
	} else {
		Color32::from_rgb(210, 153, 34)
	};
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
