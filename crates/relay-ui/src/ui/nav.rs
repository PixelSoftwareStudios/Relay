//! Bottom navigation of the compact layout: one touch-sized tab per page. A
//! label is never broken mid-word: the font shrinks to fit, then the label is
//! cut with an ellipsis. Tapping a tab names it in full in the page heading,
//! and the full name is also the hover text.

use egui::{FontId, Sense, Ui, text::LayoutJob, vec2};

use crate::icons::page_icon;
use crate::layout::{MIN_TOUCH_TARGET, Page};

/// Height of a tab: the icon above a short label.
const TAB_HEIGHT: f32 = MIN_TOUCH_TARGET + 8.0;
/// Size of a tab icon (points).
const TAB_ICON: f32 = 20.0;

/// Tab label sizes tried largest first (points).
const TAB_FONT_MAX: f32 = 13.0;
const TAB_FONT_MIN: f32 = 10.0;

/// `ringing` puts a dot on the Calls tab: a call is waiting, whatever page is open.
pub fn bottom_bar(ui: &mut Ui, page: &mut Page, ringing: bool) {
	ui.columns(Page::ALL.len(), |cols| {
		for (col, p) in cols.iter_mut().zip(Page::ALL) {
			let size = vec2(col.available_width(), TAB_HEIGHT);
			let (rect, response) = col.allocate_exact_size(size, Sense::click());
			if response.clicked() {
				*page = p;
			}
			let selected = *page == p;
			let visuals = col.style().interact_selectable(&response, selected);
			col.painter().rect(
				rect,
				visuals.corner_radius,
				visuals.weak_bg_fill,
				visuals.bg_stroke,
				egui::StrokeKind::Inside,
			);
			let galley = fitted(col, p.label(), rect.width() - 4.0, visuals.text_color());
			let pos = egui::pos2(
				rect.center().x - galley.size().x / 2.0,
				rect.max.y - galley.size().y - 4.0,
			);
			col.painter().galley(pos, galley, visuals.text_color());
			let icon = page_icon(p);
			col.painter().text(
				egui::pos2(rect.center().x, rect.min.y + 4.0 + TAB_ICON / 2.0),
				egui::Align2::CENTER_CENTER,
				icon.codepoint(),
				icon.font(TAB_ICON),
				visuals.text_color(),
			);
			if ringing && p == Page::Calls {
				col.painter().circle_filled(
					rect.right_top() + vec2(-8.0, 8.0),
					4.0,
					crate::theme::LIVE,
				);
			}
			response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, p.label()));
			response.on_hover_text(p.label());
		}
	});
}

/// The label in the largest font that fits `width`, else cut with an ellipsis
/// at the smallest font.
fn fitted(ui: &Ui, text: &str, width: f32, color: egui::Color32) -> std::sync::Arc<egui::Galley> {
	let mut size = TAB_FONT_MAX;
	loop {
		let font = FontId::proportional(size);
		let whole = ui
			.painter()
			.layout_no_wrap(text.to_owned(), font.clone(), color);
		if whole.size().x <= width || size <= TAB_FONT_MIN {
			if whole.size().x <= width {
				return whole;
			}
			let mut job = LayoutJob::simple_singleline(text.to_owned(), font, color);
			job.wrap.max_width = width;
			job.wrap.max_rows = 1;
			job.wrap.break_anywhere = true;
			job.wrap.overflow_character = Some('…');
			return ui.painter().layout_job(job);
		}
		size -= 1.0;
	}
}
