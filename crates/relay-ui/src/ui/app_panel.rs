//! The "This app" panel: daemon, autostart and the desktop switches.

use std::sync::atomic::Ordering;

use egui::{self, RichText, Ui};

use crate::actions::Actions;
use crate::format::{close_note, hotkey_note, own_live_calls, quit_prompt};
use crate::model::{AppState, Link};

use super::local::Local;
use super::sections::start_daemon_button;
use super::widgets::{button_row, hint};
use crate::tr;

pub fn app_panel(
	ui: &mut Ui,
	state: &AppState,
	actions: &Actions,
	local: &mut Local,
	has_tray: bool,
) {
	ui.heading(tr!("app-heading"));
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		let live = own_live_calls(&state.calls).len();
		ui.label(close_note(
			has_tray,
			actions.shell.runs_in_background(),
			live,
		));
		button_row(ui, |ui| {
			if has_tray && ui.button(tr!("app-hide")).clicked() {
				actions.hide_window();
			}
			if live == 0 {
				local.confirm_quit_app = false;
			}
			if ui.button(tr!("app-quit")).clicked() {
				if live == 0 {
					actions.quit_app();
				} else {
					local.confirm_quit_app = true;
				}
			}
		});
		if local.confirm_quit_app {
			ui.label(RichText::new(quit_prompt(live)).strong());
			button_row(ui, |ui| {
				if ui.button(tr!("app-hang-up-quit")).clicked() {
					actions.quit_app();
				}
				if ui.button(tr!("action-cancel")).clicked() {
					local.confirm_quit_app = false;
				}
			});
		}
		autostart_checkbox(ui, state, actions);
		super::host_switches::draw(ui, actions);
		ui.label(RichText::new(hotkey_note(actions.shell.has_talk_button())).weak());
	});
	if actions.shell.can_manage_daemon() {
		daemon_panel(ui, state, actions, local);
	}
}

/// The stored "Start daemon automatically" preference.
fn autostart_checkbox(ui: &mut Ui, state: &AppState, actions: &Actions) {
	let Some(settings) = &state.settings else {
		return;
	};
	let mut on = settings.autostart_daemon;
	let response = ui.checkbox(&mut on, tr!("autostart-label"));
	let changed = response.changed();
	hint(ui, response, tr!("autostart-hover"));
	if changed {
		actions.shared.autostart.set_setting(on);
		actions.run(|c| async move { c.set_autostart(on).await });
	}
}

/// The daemon is its own process; stopping it is a separate, confirmed act.
fn daemon_panel(ui: &mut Ui, state: &AppState, actions: &Actions, local: &mut Local) {
	ui.heading(tr!("daemon-heading"));
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		let busy = actions.shared.daemon_busy.load(Ordering::Acquire);
		if state.link != Link::Connected {
			ui.label(tr!("daemon-not-connected"));
			start_daemon_button(ui, actions);
			if actions.shared.autostart.paused() {
				ui.label(RichText::new(tr!("daemon-paused")).weak());
			}
			return;
		}
		ui.label(tr!("daemon-keeps-running"));
		if !local.confirm_quit_daemon {
			if ui
				.add_enabled(!busy, egui::Button::new(tr!("daemon-quit-ask")))
				.clicked()
			{
				local.confirm_quit_daemon = true;
			}
			return;
		}
		ui.label(RichText::new(tr!("daemon-quit-confirm")).strong());
		button_row(ui, |ui| {
			if ui
				.add_enabled(!busy, egui::Button::new(tr!("daemon-quit")))
				.clicked()
			{
				local.confirm_quit_daemon = false;
				actions.quit_daemon();
			}
			if ui.button(tr!("action-cancel")).clicked() {
				local.confirm_quit_daemon = false;
			}
		});
	});
}
