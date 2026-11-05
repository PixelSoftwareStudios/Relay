//! Bottom navigation of the compact layout: one touch-sized tab per page.

use egui::{Ui, vec2};

use crate::layout::{MIN_TOUCH_TARGET, Page};

pub fn bottom_bar(ui: &mut Ui, page: &mut Page) {
	egui::Panel::bottom("relay-nav").show(ui, |ui| {
		ui.columns(Page::ALL.len(), |cols| {
			for (col, p) in cols.iter_mut().zip(Page::ALL) {
				let size = vec2(col.available_width(), MIN_TOUCH_TARGET);
				let button = egui::Button::selectable(*page == p, p.label()).min_size(size);
				if col.add(button).clicked() {
					*page = p;
				}
			}
		});
	});
}
