//! The Settings page: a list of categories and the chosen category's page.
//! The regular layout shows the list beside the page; the compact layout
//! drills down (list, then one category with a Back button). Each category is
//! its own small module that draws into a `Ui` and holds no navigation.
//! Settings always fills the window: no notice or incoming-call strip is
//! drawn above it (see `RelayApp::ui`).

mod advanced;
mod audio;
mod calls;
mod hotkey;
mod language;
mod limits;
mod privacy;
mod profile;
mod ptt;
mod ring;
mod startup;

use egui::{self, RichText, Sense, Ui};

use crate::capabilities::{Capabilities, SettingsCategory};
use crate::theme;

use super::kit::{self, Cx};
use super::local::Local;
use crate::tr;

/// Minimum height of a category entry in the compact list (a touch target).
const ENTRY_HEIGHT: f32 = 56.0;

/// The category shown, given the user's choice: the regular layout always
/// shows one (the first when none was picked or it is not available).
pub fn current(choice: Option<SettingsCategory>, caps: Capabilities) -> SettingsCategory {
	let visible = SettingsCategory::visible(caps);
	choice
		.filter(|c| visible.contains(c))
		.unwrap_or(SettingsCategory::Profile)
}

/// The regular layout: Back button and title, the category list on the left,
/// the chosen category on the right. Returns true when Back was pressed.
pub fn show_regular(ui: &mut Ui, choice: &mut Option<SettingsCategory>, cx: &mut Cx) -> bool {
	let caps = Capabilities::of(&*cx.actions.shell);
	let selected = current(*choice, caps);
	*choice = Some(selected);
	let mut back = false;
	ui.horizontal(|ui| {
		if ui
			.button(format!("\u{2039} {}", tr!("action-back")))
			.clicked()
		{
			back = true;
		}
		ui.label(
			RichText::new(tr!("nav-settings"))
				.size(theme::TEXT_TITLE)
				.strong(),
		);
	});
	ui.add_space(theme::SPACE_M);
	egui::Panel::left("relay-settings-categories")
		.resizable(false)
		.exact_size(theme::CATEGORY_WIDTH)
		.frame(egui::Frame::NONE)
		.show_separator_line(false)
		.show(ui, |ui| {
			egui::ScrollArea::vertical()
				.id_salt("settings-categories")
				.auto_shrink([false, false])
				.show(ui, |ui| {
					for category in SettingsCategory::visible(caps) {
						if entry(ui, category, category == selected, false) {
							*choice = Some(category);
						}
					}
				});
		});
	egui::CentralPanel::default()
		.frame(egui::Frame::NONE.inner_margin(egui::Margin::symmetric(theme::SPACE_L as i8, 0)))
		.show(ui, |ui| {
			egui::ScrollArea::vertical()
				.id_salt(("settings-page", selected))
				.auto_shrink([false, false])
				.show(ui, |ui| {
					ui.set_max_width(ui.available_width().min(theme::CONTENT_MAX_WIDTH));
					category_page(ui, selected, cx);
				});
		});
	back
}

/// The compact layout: the list, or one category under a Back button.
pub fn show_compact(ui: &mut Ui, choice: &mut Option<SettingsCategory>, cx: &mut Cx) {
	let caps = Capabilities::of(&*cx.actions.shell);
	let Some(selected) = choice.filter(|c| c.available(caps)) else {
		*choice = None;
		kit::page_title(ui, tr!("nav-settings"));
		for category in SettingsCategory::visible(caps) {
			if entry(ui, category, false, true) {
				*choice = Some(category);
			}
		}
		return;
	};
	if ui
		.button(format!("\u{2039} {}", tr!("nav-settings")))
		.clicked()
	{
		*choice = None;
	}
	category_page(ui, selected, cx);
}

/// Drop what a category holds while it is not on screen (the hotkey
/// panel's draft).
pub fn release_unseen(local: &mut Local, shown: Option<SettingsCategory>) {
	if shown != Some(SettingsCategory::PushToTalk) {
		local.hotkey.reset();
	}
}

/// Esc: the compact layout steps back from a category to the list. Returns
/// true when the key was used.
pub fn step_back(choice: &mut Option<SettingsCategory>) -> bool {
	choice.take().is_some()
}

/// A category's title and content.
fn category_page(ui: &mut Ui, category: SettingsCategory, cx: &mut Cx) {
	kit::page_title(ui, category.label());
	match category {
		SettingsCategory::Profile => profile::show(ui, cx),
		SettingsCategory::Audio => audio::show(ui, cx),
		SettingsCategory::PushToTalk => ptt::show(ui, cx),
		SettingsCategory::Calls => calls::show(ui, cx),
		SettingsCategory::Privacy => privacy::show(ui, cx),
		SettingsCategory::Limits => limits::show(ui, cx),
		SettingsCategory::Language => language::show(ui, cx),
		SettingsCategory::Startup => startup::show(ui, cx),
		SettingsCategory::Advanced => advanced::show(ui, cx),
	}
}

/// One entry of the category list; `summary` adds the one-line description
/// (the compact list). Returns whether it was clicked.
fn entry(ui: &mut Ui, category: SettingsCategory, selected: bool, summary: bool) -> bool {
	let width = ui.available_width();
	let inner = width - 2.0 * theme::SPACE_M - 16.0;
	let title = ui.painter().layout(
		category.label().to_owned(),
		egui::TextStyle::Button.resolve(ui.style()),
		ui.visuals().text_color(),
		inner,
	);
	let note = summary.then(|| {
		ui.painter().layout(
			category.summary().to_owned(),
			egui::FontId::proportional(theme::TEXT_SMALL + 1.0),
			ui.visuals().weak_text_color(),
			inner,
		)
	});
	let total = title.size().y + note.as_ref().map_or(0.0, |n| n.size().y);
	let height = (total + 2.0 * theme::SPACE_M).max(if summary { ENTRY_HEIGHT } else { 36.0 });
	let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), Sense::click());
	response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, category.label()));
	let visuals = ui.style().interact_selectable(&response, selected);
	if selected || summary || response.hovered() || response.has_focus() {
		ui.painter().rect(
			rect,
			visuals.corner_radius,
			visuals.weak_bg_fill,
			visuals.bg_stroke,
			egui::StrokeKind::Inside,
		);
	}
	let x = rect.min.x + theme::SPACE_M;
	let mut y = rect.center().y - total / 2.0;
	let h = title.size().y;
	ui.painter()
		.galley(egui::pos2(x, y), title, visuals.text_color());
	y += h;
	if let Some(note) = note {
		let color = ui.visuals().weak_text_color();
		ui.painter().galley(egui::pos2(x, y), note, color);
		ui.painter().text(
			egui::pos2(rect.max.x - theme::SPACE_M, rect.center().y),
			egui::Align2::RIGHT_CENTER,
			"\u{203A}",
			egui::FontId::proportional(theme::TEXT_HEADING),
			visuals.text_color(),
		);
	}
	response.clicked()
}
