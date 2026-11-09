//! "Reset Relay data…": the way out when the daemon cannot start (an old database it refuses to
//! open). It needs no daemon, asks twice, and says plainly what is deleted and what is kept.
//! Shown in Settings > Advanced and on the "daemon not running" strip.

use std::sync::atomic::Ordering;

use egui::{RichText, Ui};

use crate::actions::Actions;
use crate::tr;

use super::kit;
use super::widgets::button_row;

/// The button, or after pressing it the confirmation. `confirming` is the user's pending
/// choice (kept by the caller so Esc can cancel it). Where the shell cannot reset, a line says
/// what to do instead.
pub fn controls(ui: &mut Ui, actions: &Actions, confirming: &mut bool) {
	if !actions.shell.can_reset_data() {
		kit::help(ui, tr!("reset-unsupported"));
		return;
	}
	let busy = actions.shared.daemon_busy.load(Ordering::Acquire);
	if !*confirming {
		if ui
			.add_enabled(!busy, kit::danger_button_widget(tr!("reset-ask")))
			.clicked()
		{
			*confirming = true;
		}
		return;
	}
	ui.label(RichText::new(tr!("reset-confirm-title")).strong());
	kit::help(ui, tr!("reset-deletes"));
	kit::help(ui, tr!("reset-keeps"));
	button_row(ui, |ui| {
		if ui
			.add_enabled(!busy, kit::danger_button_widget(tr!("reset-confirm")))
			.clicked()
		{
			*confirming = false;
			actions.reset_local_data();
		}
		if ui.button(tr!("action-cancel")).clicked() {
			*confirming = false;
		}
	});
}

/// Advanced: the card, a title and one short line above the controls (the confirmation
/// explains the rest).
pub fn card(ui: &mut Ui, actions: &Actions, confirming: &mut bool) {
	kit::card_title(ui, tr!("reset-heading"));
	kit::help(ui, tr!("reset-what"));
	controls(ui, actions, confirming);
}
