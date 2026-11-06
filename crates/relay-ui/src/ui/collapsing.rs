//! A collapsible section whose chevron can sit on either side of the title.
//! Closed shows a right chevron, open a down chevron; with the chevron on the
//! right it keeps a `SPACE_M` inset from the edge so it never touches the border.

use egui::collapsing_header::CollapsingState;
use egui::{Align2, Rect, Role, Sense, Ui, WidgetInfo, vec2};

use crate::icons::Icon;
use crate::theme;

/// Where the chevron sits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChevronSide {
	Left,
	Right,
}

/// A section header (full-width, one tap target) over a body shown when open.
/// Returns what the body returned while it was shown.
pub fn collapsing_header<R>(
	ui: &mut Ui,
	id: impl std::hash::Hash + std::fmt::Debug,
	title: &str,
	default_open: bool,
	chevron_side: ChevronSide,
	body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
	header(ui, id, title, Some(default_open), None, chevron_side, body).0
}

/// Like [`collapsing_header`], but the caller owns the open state (`open`) and
/// is told whether the header was pressed this frame (to store the choice).
pub fn collapsing_header_controlled<R>(
	ui: &mut Ui,
	id: impl std::hash::Hash + std::fmt::Debug,
	title: &str,
	open: bool,
	chevron_side: ChevronSide,
	body: impl FnOnce(&mut Ui) -> R,
) -> (Option<R>, bool) {
	header(ui, id, title, None, Some(open), chevron_side, body)
}

fn header<R>(
	ui: &mut Ui,
	id: impl std::hash::Hash + std::fmt::Debug,
	title: &str,
	default_open: Option<bool>,
	forced: Option<bool>,
	chevron_side: ChevronSide,
	body: impl FnOnce(&mut Ui) -> R,
) -> (Option<R>, bool) {
	let id = ui.make_persistent_id(id);
	let mut state =
		CollapsingState::load_with_default_open(ui.ctx(), id, default_open.unwrap_or(false));
	if let Some(open) = forced {
		state.set_open(open);
	}
	let height = ui.spacing().interact_size.y.max(32.0);
	let (rect, response) =
		ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
	response.widget_info(|| {
		let mut info = WidgetInfo::labeled(Role::DisclosureTriangle, ui.is_enabled(), title);
		info.selected = Some(state.is_open());
		info
	});
	let pressed = response.clicked();
	if pressed && forced.is_none() {
		state.toggle(ui);
	}
	if ui.is_rect_visible(rect) {
		let visuals = ui.style().interact(&response);
		if response.hovered() || response.has_focus() {
			ui.painter()
				.rect_filled(rect, theme::CARD_RADIUS, visuals.weak_bg_fill);
		}
		let icon = if state.is_open() {
			Icon::ChevronDown
		} else {
			Icon::ChevronRight
		};
		let glyph = 16.0;
		let slot = glyph + theme::SPACE_M;
		let (chevron_x, text_x, text_align) = match chevron_side {
			ChevronSide::Left => (
				rect.min.x + theme::SPACE_M + glyph / 2.0,
				rect.min.x + theme::SPACE_M + slot,
				Align2::LEFT_CENTER,
			),
			ChevronSide::Right => (
				rect.max.x - theme::SPACE_M - glyph / 2.0,
				rect.min.x + theme::SPACE_M,
				Align2::LEFT_CENTER,
			),
		};
		let text_room = match chevron_side {
			ChevronSide::Left => rect.max.x - text_x - theme::SPACE_M,
			ChevronSide::Right => rect.max.x - text_x - slot - theme::SPACE_M,
		};
		let font = egui::TextStyle::Button.resolve(ui.style());
		let galley = ui.painter().layout(
			title.to_owned(),
			font,
			visuals.text_color(),
			text_room.max(1.0),
		);
		let pos = text_align.align_size_within_rect(
			galley.size(),
			Rect::from_min_size(egui::pos2(text_x, rect.min.y), vec2(text_room, height)),
		);
		ui.painter().galley(pos.min, galley, visuals.text_color());
		ui.painter().text(
			egui::pos2(chevron_x, rect.center().y),
			Align2::CENTER_CENTER,
			icon.codepoint(),
			icon.font(glyph),
			visuals.text_color(),
		);
	}
	state.store(ui.ctx());
	(
		state.show_body_unindented(ui, body).map(|r| r.inner),
		pressed,
	)
}
