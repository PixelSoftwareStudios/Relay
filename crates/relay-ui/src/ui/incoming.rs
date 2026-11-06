//! The incoming-call prompt. It is drawn above every page, so a ringing call
//! cannot be missed whatever the user is looking at.

use egui::{self, RichText, Ui};

use crate::icons::{Icon, icon_label_danger, icon_label_primary};
use crate::model::Mode;
use crate::theme;
use crate::viewmodel::CallCard;

use super::kit::{self, Cx};
use super::widgets::{avatar, banner, button_row};
use crate::tr;

pub fn show(ui: &mut Ui, cards: &[&CallCard], cx: &mut Cx) {
	for card in cards {
		prompt(ui, card, cx);
	}
}

fn prompt(ui: &mut Ui, card: &CallCard, cx: &mut Cx) {
	let accent = ui.visuals().selection.stroke.color;
	egui::Frame::new()
		.fill(ui.visuals().faint_bg_color)
		.stroke(egui::Stroke::new(2.0, theme::LIVE))
		.corner_radius(theme::CARD_RADIUS)
		.inner_margin(theme::SPACE_L)
		.show(ui, |ui| {
			ui.set_width(ui.available_width());
			ui.horizontal_wrapped(|ui| {
				let tex = cx.textures.texture(&card.avatar, cx.actions);
				avatar(ui, &tex, theme::AVATAR_PROFILE);
				ui.vertical(|ui| {
					ui.label(
						RichText::new(tr!("incoming-title", name = card.name.as_str()))
							.size(theme::TEXT_TITLE)
							.strong()
							.color(accent),
					);
					ui.label(RichText::new(&card.trust).weak());
				});
			});
			if let Some(p) = &card.call.problem {
				banner(ui, &format!("{} {}", p.message, p.recovery_action), true);
			}
			if card.unknown {
				// Honest about what accepting does: a stranger becomes a contact, unverified.
				kit::help(
					ui,
					&tr!("incoming-unknown", fingerprint = card.fingerprint.as_str()),
				);
			}
			let id = card.call_id.clone();
			let actions = cx.actions;
			button_row(ui, |ui| {
				if icon_label_primary(ui, Icon::Call, tr!("call-accept")).clicked() {
					let id = id.clone();
					cx.local.goto = Some(crate::layout::Page::Calls);
					actions.run(|c| async move { c.accept_call(&id).await });
				}
				if icon_label_danger(ui, Icon::HangUp, tr!("call-decline")).clicked() {
					let id = id.clone();
					actions.run(|c| async move { c.decline_call(&id).await });
				}
				if ui.button(tr!("call-block")).clicked() {
					let (id, peer) = (id.clone(), card.peer_id.clone());
					actions.run(|c| async move {
						c.decline_call(&id).await?;
						c.block_peer(&peer).await
					});
				}
			});
			kit::help(ui, join_note(cx));
			kit::details(ui, ("incoming-details", &card.call_id), |ui| {
				ui.label(trust_line(&card.trust, &card.fingerprint));
			});
		});
	ui.add_space(theme::SPACE_M);
}

/// The line under an incoming call: trust and the short key digest, which only
/// helps tell devices apart (verification compares the full digest).
fn trust_line(trust: &str, fingerprint: &str) -> String {
	if fingerprint.is_empty() {
		tr!("trust-line", trust = trust)
	} else {
		tr!("trust-line-key", trust = trust, fingerprint = fingerprint)
	}
}

/// What accepting does to the microphone: the call starts in the mode that would really apply.
fn join_note(cx: &Cx) -> &'static str {
	let mode = cx
		.state
		.settings
		.as_ref()
		.map_or(Mode::PushToTalk, |s| s.effective_default_transmit_mode);
	match mode {
		Mode::Muted => tr!("incoming-join-muted"),
		Mode::Unmuted => tr!("incoming-join-unmuted"),
		Mode::PushToTalk => tr!("incoming-join-ptt"),
	}
}
