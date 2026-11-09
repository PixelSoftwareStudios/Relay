//! Startup and tray: whether the app opens at login and what closing the
//! window does. Only what this device can really do is shown.

use egui::{RichText, Ui};

use crate::capabilities::Capabilities;
use crate::format::{close_note, own_live_calls, quit_prompt};
use crate::host_options::{LoginStatus, login_text};

use super::super::kit::{self, Cx};
use super::super::widgets::button_row;
use crate::tr;

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	let caps = Capabilities::of(&*cx.actions.shell);
	if let Some(options) = cx.actions.shell.host_options() {
		kit::card(ui, |ui| {
			let status = options.login_status();
			let supported = !matches!(status, LoginStatus::Unsupported(_));
			let mut at_login = status == LoginStatus::On;
			let check = egui::Checkbox::new(&mut at_login, tr!("login-label"));
			if ui.add_enabled(supported, check).changed()
				&& let Err(reason) = options.set_launch_at_login(at_login)
			{
				cx.actions
					.shared
					.set_error(tr!("login-error", reason = reason));
			}
			kit::help(ui, &login_text(&status));
		});
		kit::gap(ui);
	}
	if caps.self_update {
		super::super::updates::settings_group(ui, cx);
		kit::gap(ui);
	}
	if caps.window_controls() {
		kit::card(ui, |ui| window(ui, cx, caps));
	} else {
		kit::help(ui, &close_note(false, true, 0));
	}
}

/// Close behaviour, Hide window and Quit this app (confirmed while calls run).
fn window(ui: &mut Ui, cx: &mut Cx, caps: Capabilities) {
	let (actions, local) = (cx.actions, &mut *cx.local);
	let live = own_live_calls(&cx.state.calls).len();
	kit::help(
		ui,
		&close_note(caps.hides_to_tray, caps.runs_in_background, live),
	);
	button_row(ui, |ui| {
		if caps.hides_to_tray && ui.button(tr!("app-hide")).clicked() {
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
			if kit::danger_button(ui, tr!("app-hang-up-quit")).clicked() {
				actions.quit_app();
			}
			if ui.button(tr!("action-cancel")).clicked() {
				local.confirm_quit_app = false;
			}
		});
	}
}
