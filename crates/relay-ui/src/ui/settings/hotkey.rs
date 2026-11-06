//! Push-to-talk hotkey panel of the settings page: the global key, read with
//! `GetSettings` and written with `SetSettings`. The outcome of each key
//! (applied now, saved until restart, rejected with a reason) is shown
//! exactly as the daemon answered.

use egui::{self, RichText, Ui};

use crate::actions::Actions;
use crate::engine::{SettingOutcome, SettingResult};
use crate::hotkey_settings::{ACCELERATOR_MAX_CHARS, HotkeyState, KEY_ACCELERATOR, outcome_text};
use crate::theme;

use super::super::kit;
use super::super::local::Local;
use super::super::widgets::{banner, button_row, field_width};
use crate::tr;

pub fn show(ui: &mut Ui, actions: &Actions, local: &mut Local) {
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
	kit::card_title(ui, tr!("hotkey-heading"));
	kit::help(ui, tr!("hotkey-intro"));
	let state = actions.shell.hotkey_state();
	state_row(ui, &state);
	ui.horizontal_wrapped(|ui| {
		ui.label(tr!("hotkey-field"));
		ui.add(
			egui::TextEdit::singleline(&mut panel.draft.accelerator)
				.desired_width(field_width(ui))
				.char_limit(ACCELERATOR_MAX_CHARS)
				.hint_text(tr!("hotkey-hint")),
		);
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
			let live = state != HotkeyState::Unsupported;
			for result in results {
				outcome_row(ui, result, live);
			}
		}
	}
}

/// What became of the key this app registers: working, off or refused.
fn state_row(ui: &mut Ui, state: &HotkeyState) {
	match state {
		HotkeyState::Unsupported => {}
		HotkeyState::Off => kit::help(ui, tr!("hotkey-state-off")),
		HotkeyState::Registered(key) => {
			ui.label(RichText::new(tr!("hotkey-state-on", key = key.as_str())).color(theme::LIVE));
		}
		HotkeyState::Failed(reason) => {
			banner(
				ui,
				&tr!("hotkey-state-failed", reason = reason.as_str()),
				true,
			);
		}
	}
}

/// `live`: this app follows the stored key itself, so a saved key needs no restart.
fn outcome_row(ui: &mut Ui, result: &SettingResult, live: bool) {
	let followed = live
		&& result.key == KEY_ACCELERATOR
		&& result.outcome == SettingOutcome::SavedNeedsRestart;
	let text = if followed {
		RichText::new(tr!("hotkey-applied-here"))
	} else {
		RichText::new(outcome_text(result))
	};
	if followed {
		ui.label(text.color(theme::LIVE));
		return;
	}
	ui.label(match result.outcome {
		SettingOutcome::AppliedLive => text.color(theme::LIVE),
		SettingOutcome::SavedNeedsRestart => text,
		SettingOutcome::Rejected => text.color(theme::WARN),
	});
}
