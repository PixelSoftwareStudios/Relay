//! Staying available in the background, outside Settings: the one-time question shown on the
//! Calls page, and the chip under the profile that says which way it is set.

use egui::{RichText, Sense, Ui};

use crate::background_settings::{KEY_BACKGROUND, QUESTION_BACKGROUND};
use crate::capabilities::SettingsCategory;
use crate::theme;

use super::kit::{self, Cx};
use super::widgets::button_row;
use crate::tr;

/// The question is asked once, where the shell can show notifications (a phone), and only when
/// the shell remembers answers. It is a card in the page, so a ringing call is never covered.
pub fn first_run(ui: &mut Ui, cx: &mut Cx) {
	let shell = cx.actions.shell.clone();
	if cx.local.first_run_closed
		|| cx.state.settings.is_none()
		|| shell.notifications_allowed().is_none()
		|| shell.question_answered(QUESTION_BACKGROUND)
	{
		return;
	}
	let mut answer = None;
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("background-ask-title"));
		kit::help(ui, tr!("background-ask-body"));
		button_row(ui, |ui| {
			if ui.button(tr!("background-ask-yes")).clicked() {
				answer = Some(true);
			}
			if ui.button(tr!("background-ask-no")).clicked() {
				answer = Some(false);
			}
		});
	});
	kit::gap(ui);
	let Some(stay) = answer else {
		return;
	};
	cx.actions.save_setting(KEY_BACKGROUND, stay.to_string());
	if stay {
		if shell.notifications_allowed() == Some(false) {
			shell.request_notifications();
		}
		if shell.battery_optimization_exempt() == Some(false) {
			shell.request_battery_exemption();
		}
	}
	shell.set_question_answered(QUESTION_BACKGROUND);
	cx.local.first_run_closed = true;
}

/// The chip in the sidebar (regular layout).
pub fn chip(ui: &mut Ui, cx: &mut Cx) {
	if !crate::layout::is_touch(ui) {
		draw(ui, cx, false);
	}
}

/// The same chip at the top of Calls where there is no sidebar (touch layout).
pub fn chip_compact(ui: &mut Ui, cx: &mut Cx) {
	if crate::layout::is_touch(ui) {
		draw(ui, cx, true);
	}
}

fn draw(ui: &mut Ui, cx: &mut Cx, touch: bool) {
	let Some(settings) = &cx.state.settings else {
		return;
	};
	let (color, text) = match settings.background_available {
		true => (theme::LIVE, tr!("background-chip-on")),
		false => (ui.visuals().weak_text_color(), tr!("background-chip-off")),
	};
	let height = if touch {
		crate::layout::MIN_TOUCH_TARGET
	} else {
		0.0
	};
	let width = ui.available_width();
	let (rect, response) =
		ui.allocate_exact_size(egui::vec2(width, height.max(16.0)), Sense::click());
	response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Link, true, text));
	let mut inner = ui.new_child(
		egui::UiBuilder::new()
			.max_rect(rect)
			.layout(egui::Layout::left_to_right(egui::Align::Center)),
	);
	inner.label(RichText::new("\u{25CF}").color(color).small());
	inner.add(
		egui::Label::new(RichText::new(text).small().color(color))
			.truncate()
			.selectable(false),
	);
	if response
		.on_hover_text(tr!("background-chip-hover"))
		.clicked()
	{
		cx.local.goto_settings = Some(SettingsCategory::Calls);
	}
	if touch {
		kit::gap(ui);
	}
}
