//! Section drawing. Reads view models, emits [`Actions`]; holds no logic.

use std::sync::atomic::Ordering;

use egui::{self, RichText, Ui};

use crate::actions::Actions;
use crate::format::{gain_percent, link_label};
use crate::model::{AppState, Link, Mode};
use crate::viewmodel::{CallCard, PeerRow, mode_enabled, modes};

use super::local::Local;
use super::textures::Textures;
use super::widgets::{avatar, banner, gain_slider, meter, tone_color};
use crate::tr;

pub fn status(ui: &mut Ui, state: &AppState, error: Option<&str>, actions: &Actions) {
	if state.mock {
		banner(ui, tr!("banner-mock"), false);
	} else if state.link != Link::Connected {
		let mut text = format!("{}.", link_label(state.link));
		if let Some(e) = &state.link_error {
			text.push_str(&format!(" {e}"));
		}
		if state.link == Link::Offline {
			text.push(' ');
			text.push_str(
				if actions.shared.autostart.enabled() && !actions.shared.autostart.paused() {
					tr!("link-offline-auto")
				} else {
					tr!("link-offline-manual")
				},
			);
		}
		banner(ui, &text, state.link == Link::Offline);
		if state.link == Link::Offline && actions.shell.can_manage_daemon() {
			start_daemon_button(ui, actions);
		}
	}
	for w in &state.warnings {
		banner(ui, &w.message, false);
	}
	if let Some(e) = error {
		banner(ui, e, true);
	}
	if let Some(n) = &state.notice {
		banner(ui, n, false);
	}
}

pub(super) fn start_daemon_button(ui: &mut Ui, actions: &Actions) {
	let busy = actions.shared.daemon_busy.load(Ordering::Acquire);
	let label = if busy {
		tr!("daemon-starting-button")
	} else {
		tr!("daemon-start-button")
	};
	if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
		actions.start_daemon();
	}
}

pub fn calls(
	ui: &mut Ui,
	cards: &[CallCard],
	actions: &Actions,
	local: &mut Local,
	textures: &mut Textures,
) {
	ui.heading(tr!("calls-heading"));
	if cards.is_empty() {
		ui.label(RichText::new(tr!("calls-none")).weak());
	}
	for card in cards {
		egui::Frame::group(ui.style()).show(ui, |ui| {
			ui.set_width(ui.available_width());
			call_card(ui, card, actions, local, textures);
		});
	}
}

fn call_card(
	ui: &mut Ui,
	card: &CallCard,
	actions: &Actions,
	local: &mut Local,
	textures: &mut Textures,
) {
	let id = card.call_id.clone();
	ui.horizontal_wrapped(|ui| {
		let tex = textures.texture(&card.avatar, actions);
		avatar(ui, &tex, 36.0);
		ui.vertical(|ui| {
			ui.label(RichText::new(&card.name).strong());
			let sub = RichText::new(&card.subtitle);
			ui.label(if card.speaking {
				sub.color(egui::Color32::from_rgb(63, 185, 80))
			} else {
				sub.weak()
			});
		});
		if card.receive_dbfs.is_some() {
			meter(
				ui,
				card.receive_dbfs,
				&tr!("meter-incoming", name = card.name.as_str()),
			);
		}
	});
	if let Some(p) = &card.call.problem {
		banner(ui, &format!("{} {}", p.message, p.recovery_action), true);
	}
	if card.incoming {
		ui.label(RichText::new(trust_line(&card.trust, &card.fingerprint)).small());
		ui.horizontal_wrapped(|ui| {
			if ui.button(tr!("call-accept")).clicked() {
				actions.run(|c| async move { c.accept_call(&id).await });
			}
			if ui.button(tr!("call-decline")).clicked() {
				let id = card.call_id.clone();
				actions.run(|c| async move { c.decline_call(&id).await });
			}
			ui.label(RichText::new(tr!("call-accept-note")).weak());
		});
		return;
	}
	controls(ui, card, actions, local);
}

/// The line under an incoming call: trust and the short key digest, which only helps tell
/// devices apart (verification compares the full digest).
fn trust_line(trust: &str, fingerprint: &str) -> String {
	if fingerprint.is_empty() {
		tr!("trust-line", trust = trust)
	} else {
		tr!("trust-line-key", trust = trust, fingerprint = fingerprint)
	}
}

fn controls(ui: &mut Ui, card: &CallCard, actions: &Actions, local: &mut Local) {
	let call = &card.call;
	let cid = card.call_id.clone();
	ui.horizontal_wrapped(|ui| {
		let end = if card.phase == crate::model::Phase::OutgoingRinging {
			tr!("action-cancel")
		} else {
			tr!("call-end")
		};
		if ui.button(end).clicked() {
			let id = cid.clone();
			actions.run(|c| async move { c.end_call(&id).await });
		}
		if ui
			.button(tr!("nav-messages"))
			.on_hover_text(tr!("call-messages-hover"))
			.clicked()
		{
			local.open_messages = Some(cid.clone());
		}
		let (mute, deaf) = (call.user_muted, call.user_deafened);
		if ui
			.selectable_label(
				mute,
				if mute {
					tr!("call-unmute")
				} else {
					tr!("call-mute")
				},
			)
			.clicked()
		{
			let id = cid.clone();
			actions.run(|c| async move { c.set_call_controls(&id, Some(!mute), None, None).await });
		}
		if ui
			.selectable_label(
				deaf,
				if deaf {
					tr!("call-undeafen")
				} else {
					tr!("call-deafen")
				},
			)
			.clicked()
		{
			let id = cid.clone();
			actions.run(|c| async move { c.set_call_controls(&id, None, Some(!deaf), None).await });
		}
	});
	let edit = local.gain_edits.entry(format!("gain-{cid}")).or_default();
	let label = tr!(
		"call-their-volume",
		value = gain_percent(edit.unwrap_or(call.receive_gain))
	);
	if let Some(g) = gain_slider(ui, edit, call.receive_gain, &label) {
		let id = cid.clone();
		actions.run(|c| async move { c.set_call_controls(&id, None, None, Some(g)).await });
	}

	ui.separator();
	ui.label(RichText::new(tr!("call-your-mic")).strong());
	let consent = local
		.consent
		.entry(cid.clone())
		.or_insert(call.transmit.consent_granted);
	ui.add_enabled(
		call.transmit.mode == Mode::ReceiveOnly,
		egui::Checkbox::new(consent, tr!("call-consent", name = card.name.as_str())),
	);
	let consent = *consent;
	ui.horizontal_wrapped(|ui| {
		for (label, mode) in modes() {
			let selected = call.transmit.mode == mode;
			let enabled = mode_enabled(mode, consent);
			if ui
				.add_enabled(enabled, egui::Button::selectable(selected, label))
				.clicked() && !selected
			{
				let id = cid.clone();
				actions.run(|c| async move { c.set_transmit(&id, mode, consent).await });
			}
		}
	});
	let t = &card.transmit;
	ui.label(RichText::new(format!("{}. {}", t.label, t.detail)).color(tone_color(t.tone, ui)));
}

pub fn peers(
	ui: &mut Ui,
	rows: &[PeerRow],
	actions: &Actions,
	local: &mut Local,
	textures: &mut Textures,
) {
	ui.heading(tr!("peers-heading"));
	if rows.is_empty() {
		ui.label(RichText::new(tr!("peers-none")).weak());
	}
	for row in rows {
		egui::Frame::group(ui.style()).show(ui, |ui| {
			ui.set_width(ui.available_width());
			ui.horizontal_wrapped(|ui| {
				let tex = textures.texture(&row.avatar, actions);
				avatar(ui, &tex, 36.0);
				ui.vertical(|ui| {
					ui.label(&row.name);
					ui.label(RichText::new(&row.subtitle).weak());
					if !row.fingerprint.is_empty() {
						ui.label(
							RichText::new(tr!("peer-key", fingerprint = row.fingerprint.as_str()))
								.small()
								.weak(),
						);
					}
					if !row.badges.is_empty() {
						ui.label(
							RichText::new(tr!(
								"peer-shared-channels",
								names = row.badges.join(", ")
							))
							.small(),
						);
					}
				});
			});
			if ui
				.add_enabled(row.can_call, egui::Button::new(tr!("peer-call")))
				.clicked()
			{
				let id = row.peer_id.clone();
				actions.run(|c| async move { c.start_call(&id).await });
			}
			for (channel_id, channel_name) in &row.revocable {
				revoke_row(ui, actions, local, row, channel_id, channel_name);
			}
		});
	}
}

/// Two-step revoke: the first click asks, the second one acts.
fn revoke_row(
	ui: &mut Ui,
	actions: &Actions,
	local: &mut Local,
	row: &PeerRow,
	channel_id: &str,
	channel_name: &str,
) {
	let key = (channel_id.to_owned(), row.peer_id.clone());
	ui.horizontal_wrapped(|ui| {
		if local.revoke_confirm.as_ref() == Some(&key) {
			ui.label(tr!(
				"revoke-ask",
				name = row.name.as_str(),
				channel = channel_name
			));
			if ui.button(tr!("revoke-yes")).clicked() {
				let (c, p) = key.clone();
				actions.run(|ctl| async move { ctl.revoke_member(&c, &p).await });
				local.revoke_confirm = None;
			}
			if ui.button(tr!("revoke-keep")).clicked() {
				local.revoke_confirm = None;
			}
		} else if ui
			.small_button(tr!("revoke-from", channel = channel_name))
			.clicked()
		{
			local.revoke_confirm = Some(key);
		}
	});
}
