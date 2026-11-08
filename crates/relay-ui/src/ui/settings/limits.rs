//! Storage and limits: how much this device keeps (avatar cache, chat memory, call history,
//! parallel file transfers). Read with `GetSettings`, edited as drafts, and saved with
//! `SetSettings`, which is sent only the keys that changed. The outcome of each key (applied
//! now, saved until restart, rejected with a reason) is shown as the daemon answered.

use egui::{self, RichText, Ui};

use crate::engine::SettingOutcome;
use crate::limits_settings::{DEFS, Def, LimitsPanel, outcome_text};
use crate::theme;

use super::super::kit::{self, Cx};
use super::super::widgets::{banner, button_row, slider_row};
use crate::tr;

/// The panel's state lives in egui's memory with the pass it was last drawn in: a gap of
/// passes means the page was closed, so reopening asks the daemon again.
type Remembered = (LimitsPanel, u64);

/// The limits grouped by what they bound: indexes into [`DEFS`].
const GROUPS: [&[usize]; 3] = [&[0], &[1, 6, 2, 5], &[3, 4]];

fn group_title(group: usize) -> &'static str {
	match group {
		0 => tr!("limits-group-pictures"),
		1 => tr!("limits-group-chats"),
		_ => tr!("limits-group-history"),
	}
}

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	let actions = cx.actions;
	let id = ui.make_persistent_id("relay-limits-panel");
	let pass = ui.ctx().cumulative_pass_nr();
	let (mut panel, last): Remembered = ui.data_mut(|d| d.get_temp(id)).unwrap_or_default();
	if pass > last + 1 {
		panel = LimitsPanel::default();
		let _ = actions.shared.inbox.take_limits_loaded();
		let _ = actions.shared.inbox.take_limits_saved();
	}
	if let Some(result) = actions.shared.inbox.take_limits_loaded() {
		panel.loaded(result);
	}
	if let Some(result) = actions.shared.inbox.take_limits_saved() {
		panel.saved(result);
	}
	if !panel.requested {
		panel.requested = true;
		actions.fetch_limits();
	}
	body(ui, cx, &mut panel);
	ui.data_mut(|d| d.insert_temp(id, (panel, pass)));
}

fn body(ui: &mut Ui, cx: &mut Cx, panel: &mut LimitsPanel) {
	let actions = cx.actions;
	let loaded = match &panel.stored {
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
		Some(Ok(loaded)) => loaded.clone(),
	};
	kit::help(ui, tr!("limits-intro"));
	kit::gap(ui);
	for (group, members) in GROUPS.into_iter().enumerate() {
		kit::card(ui, |ui| {
			kit::card_title(ui, group_title(group));
			for &index in members {
				limit(ui, panel, &DEFS[index], index, &loaded.usage);
			}
		});
		kit::gap(ui);
	}
	let problem = panel.draft.check().err();
	if let Some(text) = &problem {
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
			actions.save_limits(loaded.values, panel.draft);
		}
		if ui
			.add_enabled(panel.dirty(), egui::Button::new(tr!("action-revert")))
			.clicked()
		{
			panel.revert();
		}
		if ui
			.add_enabled(panel.resettable(), egui::Button::new(tr!("limits-reset")))
			.clicked()
		{
			panel.reset_to_defaults();
		}
	});
	match &panel.report {
		None => {}
		Some(Err(message)) => banner(ui, message, true),
		Some(Ok(results)) if results.is_empty() => {
			ui.label(RichText::new(tr!("limits-nothing")).weak());
		}
		Some(Ok(results)) => {
			for result in results {
				let text = RichText::new(outcome_text(result));
				ui.label(match result.outcome {
					SettingOutcome::AppliedLive => text.color(theme::LIVE),
					SettingOutcome::SavedNeedsRestart => text,
					SettingOutcome::Rejected => text.color(theme::WARN),
				});
			}
		}
	}
}

/// One limit: its slider, what is in use, and what it does when reached.
fn limit(
	ui: &mut Ui,
	panel: &mut LimitsPanel,
	def: &Def,
	index: usize,
	usage: &std::collections::BTreeMap<String, u64>,
) {
	ui.add_space(theme::SPACE_S);
	// The unit is in the label, not after the number: a long translated unit would
	// widen the value box past the row.
	let title = format!("{} ({})", def.label(), def.unit.text(def.max));
	slider_row(ui, &title, |ui| {
		// The value box of a short number must still be a full touch target wide.
		ui.spacing_mut().interact_size.x = crate::layout::MIN_TOUCH_TARGET;
		ui.add(egui::Slider::new(
			&mut panel.draft.0[index],
			def.min..=def.max,
		))
	});
	if let Some(text) = usage.get(def.key).and_then(|b| def.usage_text(*b)) {
		kit::help(ui, &text);
	}
	kit::help(ui, def.help());
}
