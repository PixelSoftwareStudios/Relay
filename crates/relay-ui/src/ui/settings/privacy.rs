//! Privacy and trust: whether nearby devices can find this one, session
//! messages on or off, and the devices that are trusted.

use egui::{RichText, Ui};

use crate::engine::SettingOutcome;
use crate::hotkey_settings::outcome_text;
use crate::model::Advert;
use crate::theme;
use crate::viewmodel::peer_rows;

use super::super::contacts;
use super::super::kit::{self, Cx};
use super::super::outdated;
use super::super::widgets::{banner, combo};
use crate::tr;

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	if let Some(settings) = &cx.state.settings {
		let actions = cx.actions;
		kit::card(ui, |ui| {
			let label = |v: Advert| match v {
				Advert::Visible => tr!("privacy-advert-visible"),
				Advert::Hidden => tr!("privacy-advert-hidden"),
			};
			let mut advert = settings.advert;
			combo(ui, tr!("privacy-visibility"), label(advert)).show_ui(ui, |ui| {
				for v in [Advert::Visible, Advert::Hidden] {
					ui.selectable_value(&mut advert, v, label(v));
				}
			});
			if advert != settings.advert {
				actions.run(|c| async move { c.set_privacy(None, Some(advert)).await });
			}
			kit::help(ui, tr!("privacy-visibility-help"));
		});
		kit::gap(ui);
	} else {
		kit::help(ui, tr!("privacy-none"));
	}
	if let Some(settings) = &cx.state.settings {
		super::background::auto_answer(ui, settings, cx);
		kit::gap(ui);
	}
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("privacy-messages"));
		super::super::messages::privacy_toggle(ui, cx.actions);
	});
	kit::gap(ui);
	history_switch(ui, cx);
	trusted(ui, cx);
	blocked(ui, cx);
}

/// Blocked devices: name and short fingerprint, each with Unblock (which only lets the
/// device be met again; it does not make it a contact).
fn blocked(ui: &mut Ui, cx: &mut Cx) {
	kit::section_title(ui, tr!("privacy-blocked"));
	let rows: Vec<_> = peer_rows(cx.state)
		.into_iter()
		.filter(|r| r.blocked)
		.collect();
	if rows.is_empty() {
		kit::help(ui, tr!("privacy-blocked-none"));
		return;
	}
	kit::help(ui, tr!("privacy-blocked-help"));
	for row in &rows {
		kit::card(ui, |ui| {
			ui.label(RichText::new(&row.name).strong());
			ui.label(RichText::new(&row.fingerprint).weak().monospace());
			if ui.button(tr!("unblock")).clicked() {
				let id = row.peer_id.clone();
				cx.actions.run(|c| async move { c.forget_peer(&id).await });
			}
		});
		kit::gap(ui);
	}
}

/// "Keep call history": the preference `history.enabled`, sent with `SetSettings`. The
/// daemon's answer for the key is shown as it gave it.
fn history_switch(ui: &mut Ui, cx: &mut Cx) {
	let Some(settings) = &cx.state.settings else {
		return;
	};
	kit::card(ui, |ui| {
		let mut keep = outdated::history_on(settings);
		let changed = ui
			.add_enabled(
				!cx.local.history_saving,
				egui::Checkbox::new(&mut keep, tr!("history-setting-label")),
			)
			.changed();
		if changed {
			cx.local.history_saving = true;
			cx.local.history_report = None;
			cx.actions.save_history_enabled(keep);
		}
		kit::help(ui, tr!("history-setting-help"));
		match &cx.local.history_report {
			None => {}
			Some(Err(message)) => banner(ui, message, true),
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
	});
	kit::gap(ui);
}

/// Devices a trust decision is stored for, each with a confirmed Forget.
fn trusted(ui: &mut Ui, cx: &mut Cx) {
	kit::section_title(ui, tr!("privacy-trusted"));
	let rows: Vec<_> = peer_rows(cx.state)
		.into_iter()
		.filter(|r| r.known)
		.collect();
	if rows.is_empty() {
		kit::help(ui, tr!("privacy-trusted-none"));
		return;
	}
	kit::help(ui, tr!("privacy-trusted-help"));
	for row in &rows {
		kit::card(ui, |ui| contacts::trusted_row(ui, row, cx));
		kit::gap(ui);
	}
}
