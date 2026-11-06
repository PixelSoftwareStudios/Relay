//! Calls and notifications: who may ring this device, the ringtone, and the
//! notification shown for a call while the window is out of sight.

use egui::Ui;

use crate::host_options::notify_text;
use crate::model::{Inbound, Mode, Settings};
use crate::setting_save::{KEY_DEFAULT_MODE, mode_text};

use super::super::kit::{self, Cx};
use super::super::widgets::combo;
use super::{ring, saved};
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
	default_mode(ui, settings, cx);
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

/// "Default when a call connects": muted, unmuted or push to talk, each with a line saying what
/// it means. While push-to-talk is off its choice is greyed and the real fallback is named.
fn default_mode(ui: &mut Ui, settings: &Settings, cx: &mut Cx) {
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("calls-default-mode"));
		kit::help(ui, tr!("calls-default-mode-help"));
		let busy = saved::busy(cx, KEY_DEFAULT_MODE);
		let chosen = settings.default_transmit_mode;
		let options = [
			(Mode::Muted, tr!("mode-muted"), tr!("calls-default-muted")),
			(
				Mode::Unmuted,
				tr!("mode-unmuted"),
				tr!("calls-default-unmuted"),
			),
			(Mode::PushToTalk, tr!("mode-ptt"), tr!("calls-default-ptt")),
		];
		for (mode, label, help) in options {
			let usable = mode != Mode::PushToTalk || settings.ptt_enabled;
			ui.add_enabled_ui(usable && !busy, |ui| {
				if ui.radio(chosen == mode, label).clicked() && chosen != mode {
					saved::save(cx, KEY_DEFAULT_MODE, mode_text(mode).to_owned());
				}
			});
			kit::help(ui, help);
		}
		if settings.effective_default_transmit_mode != chosen {
			let fallback = match settings.effective_default_transmit_mode {
				Mode::Muted => tr!("mode-muted"),
				Mode::Unmuted => tr!("mode-unmuted"),
				Mode::PushToTalk => tr!("mode-ptt"),
			};
			kit::help(ui, &tr!("calls-default-fallback", mode = fallback));
		}
		saved::answer(ui, cx, KEY_DEFAULT_MODE);
	});
}
