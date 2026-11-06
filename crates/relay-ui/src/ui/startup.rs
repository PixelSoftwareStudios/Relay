//! The "waiting for secure storage" card at the top of Calls.
//!
//! The background service answers before it has opened the system's secure storage, which can
//! wait for the user's password. That is not an error: while it waits the card is calm (a
//! spinner, no error colour). If opening failed the card says why in plain words and offers
//! Try again, or, after an explicit confirmation, the less secure file storage.

use egui::Ui;

use crate::model::Startup;
use crate::tr;

use super::kit::{self, Cx};
use super::widgets::button_row;

/// The plain reason a failed start-up gives.
pub fn reason(startup: &Startup) -> &'static str {
	if startup.unreadable {
		tr!("startup-reason-unreadable")
	} else {
		tr!("startup-reason-keychain")
	}
}

pub fn card(ui: &mut Ui, cx: &mut Cx) {
	let Some(startup) = cx.state.startup.as_ref() else {
		return;
	};
	egui::Frame::group(ui.style())
		.inner_margin(crate::theme::SPACE_M)
		.show(ui, |ui| {
			ui.set_width(ui.available_width());
			if startup.failed {
				failed(ui, cx, startup);
			} else {
				waiting(ui);
			}
		});
	kit::gap(ui);
}

fn waiting(ui: &mut Ui) {
	ui.horizontal(|ui| {
		ui.add(egui::Spinner::new());
		ui.label(egui::RichText::new(tr!("startup-waiting-title")).strong());
	});
	kit::help(ui, tr!("startup-waiting-body"));
}

fn failed(ui: &mut Ui, cx: &mut Cx, startup: &Startup) {
	ui.label(egui::RichText::new(tr!("startup-failed-title", reason = reason(startup))).strong());
	if !startup.unreadable {
		kit::help(ui, tr!("startup-failed-help"));
	}
	if !startup.detail.is_empty() {
		kit::help(ui, &startup.detail);
	}
	let asking = ui.make_persistent_id("startup-file-store-confirm");
	let confirming = ui.data(|d| d.get_temp::<bool>(asking)).unwrap_or(false);
	if confirming && !startup.unreadable {
		kit::help(ui, tr!("startup-file-warning"));
		button_row(ui, |ui| {
			if ui.button(tr!("startup-file-confirm")).clicked() {
				cx.actions.use_file_storage();
				ui.data_mut(|d| d.insert_temp(asking, false));
			}
			if ui.button(tr!("startup-cancel")).clicked() {
				ui.data_mut(|d| d.insert_temp(asking, false));
			}
		});
		return;
	}
	button_row(ui, |ui| {
		if ui.button(tr!("startup-retry")).clicked() {
			cx.actions.retry_secure_storage();
		}
		if !startup.unreadable && ui.button(tr!("startup-file-store")).clicked() {
			ui.data_mut(|d| d.insert_temp(asking, true));
		}
	});
}
