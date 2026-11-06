//! Calls and notifications: who may ring this device, the ringtone, and the
//! notification shown for a call while the window is out of sight.

use egui::Ui;

use crate::host_options::notify_text;
use crate::model::Inbound;

use super::super::kit::{self, Cx};
use super::super::widgets::combo;
use super::ring;
use crate::tr;

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	let Some(settings) = &cx.state.settings else {
		kit::help(ui, tr!("privacy-none"));
		return;
	};
	let actions = cx.actions;
	kit::card(ui, |ui| {
		let label = |v: Inbound| match v {
			Inbound::Anyone => tr!("privacy-inbound-anyone"),
			Inbound::SharedChannelOnly => tr!("privacy-inbound-shared"),
		};
		let mut inbound = settings.inbound;
		combo(ui, tr!("privacy-who-rings"), label(inbound)).show_ui(ui, |ui| {
			for v in [Inbound::Anyone, Inbound::SharedChannelOnly] {
				ui.selectable_value(&mut inbound, v, label(v));
			}
		});
		if inbound != settings.inbound {
			actions.run(|c| async move { c.set_privacy(Some(inbound), None).await });
		}
		kit::help(ui, tr!("privacy-who-rings-help"));
	});
	kit::gap(ui);
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("calls-sounds"));
		ring::ring_controls(ui, settings, actions, cx.local);
	});
	if let Some(options) = actions.shell.host_options() {
		kit::gap(ui);
		kit::card(ui, |ui| {
			let mut notify = options.notify_incoming();
			if ui.checkbox(&mut notify, tr!("notify-label")).changed() {
				options.set_notify_incoming(notify);
			}
			kit::help(
				ui,
				&notify_text(notify, options.notify_failure().as_deref()),
			);
		});
	}
}
