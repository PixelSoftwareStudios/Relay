//! The card of a call in progress (or ringing out): who, how long, mute,
//! deafen, hang up, and the microphone settings of this call.

use egui::{self, RichText, Ui};

use crate::actions::Actions;
use crate::format::{call_duration, gain_percent};
use crate::model::{Mode, Phase};
use crate::theme;
use crate::viewmodel::{CallCard, mode_enabled, modes};

use super::kit::{self, Cx};
use super::local::Local;
use super::widgets::{avatar, banner, button_row, gain_slider, hold_button, meter, tone_color};
use crate::tr;

pub fn show(ui: &mut Ui, card: &CallCard, cx: &mut Cx) {
	kit::card(ui, |ui| {
		header(ui, card, cx);
		if let Some(p) = &card.call.problem {
			banner(ui, &format!("{} {}", p.message, p.recovery_action), true);
		}
		buttons(ui, card, cx.actions, cx.local);
		ui.separator();
		volume(ui, card, cx.actions, cx.local);
		ui.separator();
		microphone(ui, card, cx.actions, cx.local);
	});
}

fn header(ui: &mut Ui, card: &CallCard, cx: &mut Cx) {
	let since = cx.local.call_since.get(&card.call_id).copied();
	ui.horizontal_wrapped(|ui| {
		let tex = cx.textures.texture(&card.avatar, cx.actions);
		avatar(ui, &tex, theme::AVATAR_ROW + 8.0);
		ui.vertical(|ui| {
			ui.label(RichText::new(&card.name).size(theme::TEXT_HEADING).strong());
			let mut sub = card.subtitle.clone();
			if let Some(start) = since.filter(|_| card.phase == Phase::Connected) {
				sub = format!("{sub} · {}", call_duration(start.elapsed().as_secs()));
			}
			let sub = RichText::new(sub);
			ui.label(if card.speaking {
				sub.color(theme::LIVE)
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
}

fn buttons(ui: &mut Ui, card: &CallCard, actions: &Actions, local: &mut Local) {
	let call = &card.call;
	let cid = card.call_id.clone();
	button_row(ui, |ui| {
		let (mute, deaf) = (call.user_muted, call.user_deafened);
		let mute_label = if mute {
			tr!("call-unmute")
		} else {
			tr!("call-mute")
		};
		if ui.selectable_label(mute, mute_label).clicked() {
			let id = cid.clone();
			actions.run(|c| async move { c.set_call_controls(&id, Some(!mute), None, None).await });
		}
		let deaf_label = if deaf {
			tr!("call-undeafen")
		} else {
			tr!("call-deafen")
		};
		if ui.selectable_label(deaf, deaf_label).clicked() {
			let id = cid.clone();
			actions.run(|c| async move { c.set_call_controls(&id, None, Some(!deaf), None).await });
		}
		if ui
			.button(tr!("nav-messages"))
			.on_hover_text(tr!("call-messages-hover"))
			.clicked()
		{
			local.open_messages = Some(cid.clone());
		}
		let end = if card.phase == Phase::OutgoingRinging {
			tr!("action-cancel")
		} else {
			tr!("call-end")
		};
		if kit::danger_button(ui, end).clicked() {
			let id = cid.clone();
			actions.run(|c| async move { c.end_call(&id).await });
		}
	});
}

fn volume(ui: &mut Ui, card: &CallCard, actions: &Actions, local: &mut Local) {
	let call = &card.call;
	let edit = local
		.gain_edits
		.entry(format!("gain-{}", card.call_id))
		.or_default();
	let label = tr!(
		"call-their-volume",
		value = gain_percent(edit.unwrap_or(call.receive_gain))
	);
	if let Some(g) = gain_slider(ui, edit, call.receive_gain, &label) {
		let id = card.call_id.clone();
		actions.run(|c| async move { c.set_call_controls(&id, None, None, Some(g)).await });
	}
}

/// Consent, transmit mode, the talk button and what the microphone is doing.
fn microphone(ui: &mut Ui, card: &CallCard, actions: &Actions, local: &mut Local) {
	let call = &card.call;
	let cid = card.call_id.clone();
	kit::card_title(ui, tr!("call-your-mic"));
	let consent = local
		.consent
		.entry(cid.clone())
		.or_insert(call.transmit.consent_granted);
	ui.add_enabled(
		call.transmit.mode == Mode::ReceiveOnly,
		egui::Checkbox::new(consent, tr!("call-consent", name = card.name.as_str())),
	);
	let consent = *consent;
	button_row(ui, |ui| {
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
	if !consent && call.transmit.mode == Mode::ReceiveOnly {
		kit::help(ui, tr!("call-mic-help"));
	}
	if actions.shell.has_talk_button() && call.transmit.mode == Mode::PushToTalk {
		talk_button(ui, card, actions, local);
	}
	let t = &card.transmit;
	ui.label(RichText::new(format!("{}. {}", t.label, t.detail)).color(tone_color(t.tone, ui)));
}

/// The on-screen hold-to-talk button: pressing it tells the shell the key went
/// down, letting go (or the button vanishing) that it went up.
fn talk_button(ui: &mut Ui, card: &CallCard, actions: &Actions, local: &mut Local) {
	let was = local.talking.as_deref() == Some(card.call_id.as_str());
	let down = hold_button(ui, tr!("call-talk-hold"), was);
	local.talk_drawn = true;
	if down && !was {
		if let Some(prev) = local.talking.replace(card.call_id.clone()) {
			actions.shell.push_to_talk(&prev, false);
		}
		actions.shell.push_to_talk(&card.call_id, true);
	} else if !down && was {
		local.talking = None;
		actions.shell.push_to_talk(&card.call_id, false);
	}
}
