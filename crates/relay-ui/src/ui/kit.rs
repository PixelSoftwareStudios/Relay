//! Building blocks shared by every page, so spacing, type and colour come
//! from `theme` tokens and not from each page.

use egui::{self, Color32, Response, RichText, Ui};

use crate::actions::Actions;
use crate::model::{AppState, Meters};
use crate::theme;

use super::local::Local;
use super::messages::MessagesUi;
use super::textures::Textures;

/// What a page needs to draw and to act.
pub struct Cx<'a> {
	pub state: &'a AppState,
	pub meters: &'a Meters,
	pub actions: &'a Actions,
	pub local: &'a mut Local,
	pub textures: &'a mut Textures,
	pub messages: &'a mut MessagesUi,
}

/// The room around the entries of a dropdown menu, so they do not touch its edge.
pub fn padded<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
	egui::Frame::new()
		.inner_margin(egui::Margin::same(theme::SPACE_M as i8))
		.show(ui, |ui| {
			ui.spacing_mut().item_spacing.y = theme::SPACE_M;
			add(ui)
		})
		.inner
}

/// A dropdown menu under `anchor`, with room around its entries.
pub fn menu(anchor: &Response, add: impl FnOnce(&mut Ui)) {
	egui::Popup::menu(anchor).show(|ui| padded(ui, add));
}

/// A rounded panel that groups related controls.
pub fn card<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
	egui::Frame::group(ui.style())
		.fill(ui.visuals().faint_bg_color)
		.corner_radius(theme::CARD_RADIUS)
		.inner_margin(theme::SPACE_L)
		.show(ui, |ui| {
			ui.set_width(ui.available_width());
			add(ui)
		})
		.inner
}

/// The big title at the top of a page.
pub fn page_title(ui: &mut Ui, text: &str) {
	ui.label(RichText::new(text).size(theme::TEXT_TITLE).strong());
	ui.add_space(theme::SPACE_S);
}

/// A title above a group of cards.
pub fn section_title(ui: &mut Ui, text: &str) {
	ui.add_space(theme::SPACE_M);
	ui.label(RichText::new(text).size(theme::TEXT_HEADING).strong());
	ui.add_space(theme::SPACE_S);
}

/// A title inside a card.
pub fn card_title(ui: &mut Ui, text: &str) {
	ui.label(RichText::new(text).strong());
}

/// Helper text under a control: weak and small, and it wraps.
pub fn help(ui: &mut Ui, text: &str) {
	ui.label(
		RichText::new(text)
			.size(theme::TEXT_SMALL + 1.0)
			.color(ui.visuals().weak_text_color()),
	);
}

/// A collapsed expander for technical detail (keys, ids, ports).
pub fn details<R>(
	ui: &mut Ui,
	id: impl std::hash::Hash + std::fmt::Debug,
	add: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
	super::collapsing::collapsing_header(
		ui,
		id,
		crate::tr!("action-details"),
		false,
		super::collapsing::ChevronSide::Left,
		add,
	)
}

/// A button for an act that is hard to undo.
pub fn danger_button(ui: &mut Ui, text: &str) -> Response {
	ui.add(danger_button_widget(text))
}

/// The danger button as a widget, for `add_enabled`.
pub fn danger_button_widget(text: &str) -> egui::Button<'_> {
	egui::Button::new(RichText::new(text).color(Color32::WHITE))
		.fill(super::icon_style::DANGER_FILL)
}

/// A friendly "nothing here yet" block: what is missing and what to do.
pub fn empty_state(ui: &mut Ui, title: &str, body: &str) {
	card(ui, |ui| {
		ui.label(RichText::new(title).size(theme::TEXT_HEADING));
		help(ui, body);
	});
}

/// Space between stacked cards.
pub fn gap(ui: &mut Ui) {
	ui.add_space(theme::SPACE_M);
}

/// A name that opens something when pressed (a contact): bold text with a touch-sized target
/// (44 points high under touch rules), underlined and tinted on hover, darker while pressed.
/// With `enabled` false it is plain bold text with nothing to press.
pub fn name_link(ui: &mut Ui, text: &str, color: Option<Color32>, enabled: bool) -> Response {
	let base = color.unwrap_or_else(|| ui.visuals().strong_text_color());
	let wrap = ui.available_width();
	if !enabled {
		return ui.label(RichText::new(text).strong().color(base));
	}
	let galley = ui.painter().layout(
		text.to_owned(),
		egui::TextStyle::Body.resolve(ui.style()),
		base,
		wrap,
	);
	let height =
		galley
			.size()
			.y
			.max(ui.spacing().interact_size.y)
			.max(if crate::layout::is_touch(ui) {
				crate::layout::MIN_TOUCH_TARGET
			} else {
				0.0
			});
	let size = egui::vec2(galley.size().x.max(crate::layout::MIN_TOUCH_TARGET), height);
	let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
	response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, text));
	if ui.is_rect_visible(rect) {
		let tint = if response.is_pointer_button_down_on() {
			ui.visuals().hyperlink_color.gamma_multiply(0.7)
		} else if response.hovered() || response.has_focus() {
			ui.visuals().hyperlink_color
		} else {
			base
		};
		let galley = ui.painter().layout(
			text.to_owned(),
			egui::TextStyle::Body.resolve(ui.style()),
			tint,
			wrap,
		);
		let pos = egui::pos2(rect.min.x, rect.center().y - galley.size().y / 2.0);
		let underline = response.hovered() || response.has_focus();
		let line = egui::Stroke::new(1.0, tint);
		let bottom = pos.y + galley.size().y;
		ui.painter().galley(pos, galley.clone(), tint);
		if underline {
			ui.painter().line_segment(
				[
					egui::pos2(pos.x, bottom),
					egui::pos2(pos.x + galley.size().x, bottom),
				],
				line,
			);
		}
	}
	response.on_hover_cursor(egui::CursorIcon::PointingHand)
}
