//! The "background service is out of date" card: shown at the top of Calls and
//! in Settings, Advanced when the running daemon is older than this app. The
//! Restart button stops and starts it again through the shell's daemon
//! control, and is enabled only while no call is active (it would end it).

use std::sync::atomic::Ordering;

use egui::Ui;

use crate::model::Settings;
use crate::tr;

use super::kit::{self, Cx};
use super::widgets::{banner, button_row};

/// Whether the card is drawn.
pub fn shown(cx: &Cx) -> bool {
	cx.state.daemon_out_of_date()
}

pub fn card(ui: &mut Ui, cx: &mut Cx) {
	if !shown(cx) {
		return;
	}
	banner(ui, tr!("daemon-outdated"), false);
	if cx.actions.shell.can_manage_daemon() {
		let busy = cx.actions.shared.daemon_busy.load(Ordering::Acquire);
		let in_call = cx.state.calls.iter().any(|c| !c.phase.is_terminal());
		let label = if busy {
			tr!("daemon-restarting")
		} else {
			tr!("daemon-restart")
		};
		button_row(ui, |ui| {
			if ui
				.add_enabled(!busy && !in_call, egui::Button::new(label))
				.clicked()
			{
				cx.actions.restart_daemon();
			}
		});
		if in_call {
			kit::help(ui, tr!("daemon-restart-in-call"));
		}
	}
	kit::gap(ui);
}

/// The "Keep call history" switch as the UI shows it. A daemon from before the
/// revision this app was built for may not report the key at all; then the
/// default, on, is shown instead of an off the user never chose.
pub fn history_on(settings: &Settings) -> bool {
	settings.history_enabled || settings.daemon_api_revision < relay_api::API_REVISION
}
