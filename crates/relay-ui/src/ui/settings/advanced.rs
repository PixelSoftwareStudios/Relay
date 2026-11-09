//! Advanced: the Relay daemon (status, start, stop), versions and the
//! numbers a bug report needs. Stopping the daemon is confirmed.

use std::sync::atomic::Ordering;

use egui::{RichText, Ui};

use crate::capabilities::Capabilities;
use crate::format::link_label;
use crate::model::Link;

use super::super::banners::start_daemon_button;
use super::super::kit::{self, Cx};
use super::super::outdated;
use super::super::widgets::button_row;
use crate::tr;

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	let caps = Capabilities::of(&*cx.actions.shell);
	outdated::card(ui, cx);
	kit::card(ui, |ui| daemon(ui, cx, caps));
	kit::gap(ui);
	let (actions, confirm) = (cx.actions, &mut cx.local.confirm_reset_data);
	kit::card(ui, |ui| super::super::reset::card(ui, actions, confirm));
	kit::gap(ui);
	kit::card(ui, |ui| about(ui, cx));
}

fn daemon(ui: &mut Ui, cx: &mut Cx, caps: Capabilities) {
	kit::card_title(ui, tr!("daemon-heading"));
	ui.label(link_label(cx.state.link));
	kit::help(ui, tr!("daemon-what"));
	if !caps.manage_daemon {
		return;
	}
	autostart_checkbox(ui, cx);
	let (actions, local) = (cx.actions, &mut *cx.local);
	let busy = actions.shared.daemon_busy.load(Ordering::Acquire);
	if cx.state.link != Link::Connected {
		ui.label(tr!("daemon-not-connected"));
		start_daemon_button(ui, actions);
		if actions.shared.autostart.paused() {
			kit::help(ui, tr!("daemon-paused"));
		}
		return;
	}
	kit::help(ui, tr!("daemon-keeps-running"));
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
			.add_enabled(!busy, kit::danger_button_widget(tr!("daemon-quit")))
			.clicked()
		{
			local.confirm_quit_daemon = false;
			actions.quit_daemon();
		}
		if ui.button(tr!("action-cancel")).clicked() {
			local.confirm_quit_daemon = false;
		}
	});
}

/// The stored "Start daemon automatically" preference.
fn autostart_checkbox(ui: &mut Ui, cx: &mut Cx) {
	let Some(settings) = &cx.state.settings else {
		return;
	};
	let mut on = settings.autostart_daemon;
	if ui.checkbox(&mut on, tr!("autostart-label")).changed() {
		cx.actions.shared.autostart.set_setting(on);
		cx.actions.run(|c| async move { c.set_autostart(on).await });
	}
	kit::help(ui, tr!("autostart-hover"));
}

fn about(ui: &mut Ui, cx: &mut Cx) {
	kit::card_title(ui, tr!("advanced-about"));
	ui.label(tr!(
		"advanced-app-version",
		version = env!("CARGO_PKG_VERSION")
	));
	if let Some(info) = &cx.state.engine {
		ui.label(tr!("header-engine", version = info.version.as_str()));
		kit::details(ui, "advanced-details", |ui| {
			ui.label(tr!(
				"advanced-api",
				major = info.api_major,
				minor = info.api_minor
			));
			ui.label(tr!(
				"advanced-counts",
				peers = cx.state.peers.len(),
				calls = cx.state.calls.len(),
				channels = cx.state.channels.len()
			));
		});
	}
}
