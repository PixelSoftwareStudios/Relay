//! An inline image in a bubble: decoded off the UI thread, shown as a texture,
//! with a one-line summary under it.

use egui::{RichText, Ui};

use super::transcript::RowCx;
use crate::avatar::Lookup;
use crate::messaging::actions::Intent;
use crate::messaging::model::{EntryView, ImageView};
use crate::messaging::rules::{INLINE_IMAGE_MAX_BYTES, format_size};
use crate::theme;
use crate::tr;

pub fn show(ui: &mut Ui, rcx: &mut RowCx<'_>, entry: &EntryView, img: &ImageView) {
	let caption = format!(
		"{} · {}x{} · {}",
		img.media_type.trim_start_matches("image/"),
		img.width,
		img.height,
		format_size(img.data.len() as u64)
	);
	if img.data.len() > INLINE_IMAGE_MAX_BYTES {
		ui.label(RichText::new(tr!("msg-image-over-limit")).color(theme::WARN));
		return;
	}
	let key = format!("{}:{}", rcx.session.key, entry.ordinal);
	match rcx.textures.get(&key) {
		Lookup::Ready(tex) => {
			let max = ui.available_width().min(320.0);
			let size = tex.size_vec2();
			let scale = (max / size.x).min(1.0);
			ui.add(egui::Image::new(tex).fit_to_exact_size(size * scale));
		}
		Lookup::Missing => {
			ui.label(RichText::new(tr!("msg-image-unsafe")).color(theme::WARN));
		}
		Lookup::Pending => {
			ui.label(RichText::new(tr!("msg-image-loading")).weak());
		}
		Lookup::Unknown => {
			if rcx.textures.request(&key) {
				rcx.intents.push(Intent::DecodeImage {
					key,
					data: img.data.clone(),
				});
			}
		}
	}
	ui.label(RichText::new(caption).small().weak());
}
