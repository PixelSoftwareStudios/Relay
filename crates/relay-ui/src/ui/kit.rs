//! Building blocks shared by every page, so spacing, type and colour come
//! from `theme` tokens and not from each page.

use egui::{self, Color32, Response, RichText, Ui};

use crate::actions::Actions;
use crate::model::{AppState, Meters};
use crate::theme;

use super::local::Local;
use super::textures::Textures;

/// What a page needs to draw and to act.
pub struct Cx<'a> {
	pub state: &'a AppState,
	pub meters: &'a Meters,
	pub actions: &'a Actions,
	pub local: &'a mut Local,
	pub textures: &'a mut Textures,
}

/// Fill of a danger button (hang up, forget, quit).
const DANGER_FILL: Color32 = Color32::from_rgb(178, 48, 42);
/// Fill of the accept button.
const ACCEPT_FILL: Color32 = Color32::from_rgb(34, 128, 56);

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
	egui::CollapsingHeader::new(RichText::new(crate::tr!("action-details")).small())
		.id_salt(id)
		.default_open(false)
		.show(ui, add)
		.body_returned
}

/// A button for an act that is hard to undo.
pub fn danger_button(ui: &mut Ui, text: &str) -> Response {
	ui.add(danger_button_widget(text))
}

/// The danger button as a widget, for `add_enabled`.
pub fn danger_button_widget(text: &str) -> egui::Button<'_> {
	egui::Button::new(RichText::new(text).color(Color32::WHITE)).fill(DANGER_FILL)
}

/// The main button of a prompt.
pub fn accept_button(ui: &mut Ui, text: &str) -> Response {
	ui.add(egui::Button::new(RichText::new(text).color(Color32::WHITE).strong()).fill(ACCEPT_FILL))
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
