//! The "Ringtone" group of "Calls and notifications": volume, the chosen file, choose, reset and
//! a preview. The file is chosen with the shell's picker and prepared by the engine; the app
//! plays nothing itself (the preview rings on the engine's own output).

use std::sync::atomic::Ordering;

use egui::{RichText, Ui};

use super::super::kit;
use super::super::local::Local;
use super::super::widgets::{banner, button_row, hint, slider_row};
use crate::actions::Actions;
use crate::model::Settings;
use crate::ringtone_pick::KEY_RINGTONE_FILE;
use crate::theme;
use crate::tr;

/// Volume slider whose drag value lives in `edit` until released.
fn volume_slider(ui: &mut Ui, edit: &mut Option<f32>, current: f32) -> Option<f32> {
	let mut value = edit.unwrap_or(current);
	let response = slider_row(ui, tr!("ring-volume"), |ui| {
		ui.add(egui::Slider::new(&mut value, 0.0..=1.0).fixed_decimals(2))
	});
	if response.dragged() || (response.changed() && edit.is_some()) {
		*edit = Some(value);
	}
	if response.drag_stopped() || (response.changed() && !response.dragged()) {
		*edit = None;
		return Some(value);
	}
	None
}

/// The name shown for the current ringtone.
pub fn current_name(settings: &Settings) -> &str {
	if settings.ringtone_name.is_empty() {
		tr!("ringtone-default-name")
	} else {
		&settings.ringtone_name
	}
}

/// Title, volume slider, current file, buttons and help.
pub fn ringtone_group(ui: &mut Ui, settings: &Settings, actions: &Actions, local: &mut Local) {
	kit::card_title(ui, tr!("ringtone-title"));
	if let Some(volume) = volume_slider(ui, &mut local.ring_volume_edit, settings.ringtone_volume) {
		actions
			.run(|c| async move { c.set_ring_preferences(None, Some(volume), None, None).await });
	}
	let busy = local.ringtone_busy.load(Ordering::Acquire);
	if busy {
		ui.label(tr!("ringtone-busy"));
	} else {
		ui.label(tr!("ringtone-current", name = current_name(settings)));
	}
	button_row(ui, |ui| {
		let choose = ui.add_enabled(!busy, egui::Button::new(tr!("ringtone-choose")));
		let clicked = choose.clicked();
		hint(ui, choose, tr!("ringtone-choose-hover"));
		if clicked {
			local.setting_reports.remove(KEY_RINGTONE_FILE);
			actions.pick_ringtone(&local.ringtone_busy);
		}
		let custom = !settings.ringtone_name.is_empty();
		if ui
			.add_enabled(custom && !busy, egui::Button::new(tr!("ringtone-reset")))
			.clicked()
		{
			local.setting_reports.remove(KEY_RINGTONE_FILE);
			actions.reset_ringtone(&local.ringtone_busy);
		}
		let test = ui.button(tr!("ring-test"));
		let clicked = test.clicked();
		hint(ui, test, tr!("ring-test-hover"));
		if clicked {
			actions.run(|c| async move { c.preview_tone().await });
		}
	});
	answer(ui, local);
	kit::help(ui, tr!("ringtone-help"));
}

/// What became of the last choose or reset: "Saved." or the reason, in plain words.
fn answer(ui: &mut Ui, local: &Local) {
	match local.setting_reports.get(KEY_RINGTONE_FILE) {
		None => {}
		Some(Err(message)) => banner(ui, message, true),
		Some(Ok(_)) => {
			ui.label(RichText::new(tr!("setting-saved")).color(theme::LIVE));
		}
	}
}
