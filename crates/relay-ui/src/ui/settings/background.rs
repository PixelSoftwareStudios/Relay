//! "Stay available in the background", the call alert and the global auto-answer switch.
//! The engine stores them; the OS shells act on the first two.

use egui::{RichText, Ui};

use crate::background_settings::{KEY_AUTO_ANSWER, KEY_BACKGROUND, KEY_CALL_ALERT};
use crate::model::{CallAlert, Settings};
use crate::theme;

use super::super::kit::{self, Cx};
use super::super::widgets::{button_row, combo};
use super::saved;
use crate::tr;

/// A checkbox that saves `key` on its own and shows the engine's answer under it.
fn switch(ui: &mut Ui, cx: &mut Cx, key: &'static str, on: bool, label: &str) {
	let mut value = on;
	let busy = saved::busy(cx, key);
	if ui
		.add_enabled(!busy, egui::Checkbox::new(&mut value, label))
		.changed()
	{
		saved::save(cx, key, value.to_string());
	}
}

/// The background switch with its help, the warnings the shell raises and their buttons.
pub fn availability(ui: &mut Ui, settings: &Settings, cx: &mut Cx) {
	kit::card(ui, |ui| {
		switch(
			ui,
			cx,
			KEY_BACKGROUND,
			settings.background_available,
			tr!("background-label"),
		);
		kit::help(ui, tr!("background-help"));
		let shell = cx.actions.shell.clone();
		if shell.battery_optimization_exempt() == Some(false) {
			warning(ui, tr!("background-battery-warning"));
			button_row(ui, |ui| {
				if ui.button(tr!("background-battery-button")).clicked() {
					shell.request_battery_exemption();
				}
			});
		}
		if shell.notifications_allowed() == Some(false) {
			warning(ui, tr!("background-notifications-warning"));
			button_row(ui, |ui| {
				if ui.button(tr!("background-notifications-button")).clicked() {
					shell.request_notifications();
				}
			});
		}
		saved::answer(ui, cx, KEY_BACKGROUND);
	});
}

fn warning(ui: &mut Ui, text: &str) {
	ui.label(RichText::new(text).color(theme::WARN));
}

fn alert_label(alert: CallAlert, vibrates: bool) -> &'static str {
	match (alert, vibrates) {
		(CallAlert::System, true) => tr!("alert-system-device"),
		(CallAlert::System, false) => tr!("alert-system-desktop"),
		(CallAlert::RingAndVibrate, true) => tr!("alert-ring-vibrate"),
		(CallAlert::RingAndVibrate, false) => tr!("alert-ring"),
		(CallAlert::Vibrate, _) => tr!("alert-vibrate"),
		(CallAlert::Silent, _) => tr!("alert-silent"),
	}
}

/// The call alert selector. Devices that cannot vibrate do not offer the vibrate choices that
/// only differ by it ("Vibrate only"; "Ring and vibrate" reads "Ring").
pub fn alert(ui: &mut Ui, settings: &Settings, cx: &mut Cx) {
	kit::card(ui, |ui| {
		let vibrates = cx.actions.shell.can_vibrate();
		let busy = saved::busy(cx, KEY_CALL_ALERT);
		let mut chosen = settings.call_alert;
		ui.add_enabled_ui(!busy, |ui| {
			combo(ui, tr!("alert-title"), alert_label(chosen, vibrates)).show_ui(ui, |ui| {
				for option in CallAlert::ALL {
					if vibrates || option != CallAlert::Vibrate {
						ui.selectable_value(&mut chosen, option, alert_label(option, vibrates));
					}
				}
			});
		});
		kit::help(ui, tr!("alert-help"));
		if chosen != settings.call_alert {
			saved::save(cx, KEY_CALL_ALERT, chosen.key().to_owned());
		}
		saved::answer(ui, cx, KEY_CALL_ALERT);
	});
}

/// The global kill switch of automatic answering (Privacy).
pub fn auto_answer(ui: &mut Ui, settings: &Settings, cx: &mut Cx) {
	kit::card(ui, |ui| {
		switch(
			ui,
			cx,
			KEY_AUTO_ANSWER,
			settings.auto_answer_enabled,
			tr!("auto-answer-global"),
		);
		kit::help(ui, tr!("auto-answer-global-help"));
		saved::answer(ui, cx, KEY_AUTO_ANSWER);
	});
}
