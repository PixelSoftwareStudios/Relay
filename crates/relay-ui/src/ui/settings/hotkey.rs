//! Push-to-talk hotkey panel of the settings page: the global key and the
//! release delay, read with `GetSettings` and written with `SetSettings`. The
//! outcome of each key (applied now, saved until restart, rejected with a
//! reason) is shown exactly as the daemon answered.

use egui::{self, RichText, Ui};

use crate::actions::Actions;
use crate::controller::RELEASE_DELAY_MAX_MS;
use crate::engine::{SettingOutcome, SettingResult};
use crate::hotkey_settings::{ACCELERATOR_MAX_CHARS, outcome_text};
use crate::theme;

use super::local::Local;
use super::widgets::{banner, button_row, field_width, hint};
use crate::tr;

pub fn hotkey(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	let header = egui::CollapsingHeader::new(RichText::new(tr!("hotkey-heading")).heading())
		.id_salt("hotkey")
		.default_open(false)
		.show(ui, |ui| body(ui, actions, local));
	if header.body_returned.is_none() {
		local.hotkey.reset();
	}
}

fn body(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	let panel = &mut local.hotkey;
	if !panel.requested {
		panel.requested = true;
		actions.fetch_hotkey_settings();
	}
	let stored = match &panel.stored {
		None => {
			ui.label(RichText::new(tr!("asking-daemon")).weak());
			return;
		}
		Some(Err(message)) => {
			banner(ui, message, true);
			if ui.button(tr!("action-try-again")).clicked() {
				panel.requested = false;
				panel.stored = None;
			}
			return;
		}
		Some(Ok(values)) => values.clone(),
	};
	ui.label(tr!("hotkey-intro"));
	ui.horizontal_wrapped(|ui| {
		ui.label(tr!("hotkey-field"));
		ui.add(
			egui::TextEdit::singleline(&mut panel.draft.accelerator)
				.desired_width(field_width(ui))
				.char_limit(ACCELERATOR_MAX_CHARS)
				.hint_text(tr!("hotkey-hint")),
		);
	});
	ui.horizontal_wrapped(|ui| {
		ui.label(tr!("hotkey-delay"));
		let drag = ui.add(
			egui::DragValue::new(&mut panel.draft.release_delay_ms)
				.range(0..=RELEASE_DELAY_MAX_MS)
				.speed(10)
				.suffix(format!(" {}", tr!("unit-ms"))),
		);
		hint(ui, drag, tr!("hotkey-delay-hover"));
	});
	let problem = panel.draft.check().err();
	if let Some(text) = problem {
		ui.label(RichText::new(text).color(theme::WARN));
	}
	button_row(ui, |ui| {
		let ready = panel.dirty() && problem.is_none() && !panel.saving;
		if ui
			.add_enabled(ready, egui::Button::new(tr!("action-save")))
			.clicked()
		{
			panel.saving = true;
			panel.report = None;
			actions.save_hotkey_settings(stored, panel.draft.clone());
		}
		if ui
			.add_enabled(panel.dirty(), egui::Button::new(tr!("action-revert")))
			.clicked()
		{
			panel.revert();
		}
	});
	match &panel.report {
		None => {}
		Some(Err(message)) => banner(ui, message, true),
		Some(Ok(results)) if results.is_empty() => {
			ui.label(RichText::new(tr!("hotkey-nothing")).weak());
		}
		Some(Ok(results)) => {
			for result in results {
				outcome_row(ui, result);
			}
		}
	}
}

fn outcome_row(ui: &mut Ui, result: &SettingResult) {
	let text = RichText::new(outcome_text(result));
	ui.label(match result.outcome {
		SettingOutcome::AppliedLive => text.color(theme::LIVE),
		SettingOutcome::SavedNeedsRestart => text,
		SettingOutcome::Rejected => text.color(theme::WARN),
	});
}
