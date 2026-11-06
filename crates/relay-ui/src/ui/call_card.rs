//! The card of a call in progress (or ringing out): who, how long, the microphone mode,
//! mute, deafen, messages and hang up. Secondary things hide behind the menu: the other
//! person's volume (also on a right click or long press of their name) and the connection
//! details.

use egui::{self, RichText, Sense, Ui};

use crate::format::call_duration;
use crate::icons::{Icon, icon_button, icon_label_danger, icon_toggle};
use crate::model::Phase;
use crate::theme;
use crate::viewmodel::CallCard;

use super::kit::{self, Cx};
use super::widgets::{avatar, banner, meter};
use super::{call_details, call_mode, call_volume, chat};
use crate::tr;

pub fn show(ui: &mut Ui, card: &CallCard, cx: &mut Cx) {
	kit::card(ui, |ui| {
		let connected = card.phase == Phase::Connected;
		if connected {
			// The time on screen changes once a second; nothing else needs a frame.
			ui.ctx()
				.request_repaint_after(std::time::Duration::from_secs(1));
		}
		header(ui, card, cx);
		if let Some(p) = &card.call.problem {
			banner(ui, &format!("{} {}", p.message, p.recovery_action), true);
		}
		if matches!(card.phase, Phase::Connected | Phase::Interrupted) {
			ui.add_space(theme::SPACE_S);
			call_mode::show(ui, card, cx);
			ui.add_space(theme::SPACE_S);
		}
		buttons(ui, card, cx);
		call_details::show(ui, card, cx);
	});
}

/// Seconds the call has been connected, by the engine's clock; `None` before it is.
pub fn connected_secs(card: &CallCard, now_ms: u64) -> Option<u64> {
	let at = card.call.connected_at_unix_ms;
	(card.phase == Phase::Connected && at > 0).then(|| now_ms.saturating_sub(at) / 1000)
}

fn header(ui: &mut Ui, card: &CallCard, cx: &mut Cx) {
	let now = cx.actions.shell.now_unix_ms();
	let mut sub = card.subtitle.clone();
	if let Some(secs) = connected_secs(card, now) {
		sub = format!("{sub} \u{b7} {}", call_duration(secs));
	}
	let id = ui.make_persistent_id(("call-head", &card.call_id));
	let width = ui.available_width();
	let mut kebab_left = f32::MAX;
	let who = ui.scope(|ui| {
		ui.horizontal(|ui| {
			let kebab = ui.spacing().interact_size.y.max(32.0) + theme::SPACE_M;
			let tex = cx.textures.texture(&card.avatar, cx.actions);
			avatar(ui, &tex, theme::AVATAR_ROW + 8.0);
			ui.vertical(|ui| {
				ui.set_max_width((width - kebab - theme::AVATAR_ROW - 24.0).max(60.0));
				ui.add(
					egui::Label::new(RichText::new(&card.name).size(theme::TEXT_HEADING).strong())
						.truncate(),
				);
				let sub = RichText::new(sub);
				ui.label(if card.speaking {
					sub.color(theme::LIVE)
				} else {
					sub.weak()
				});
			});
			ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
				kebab_left = menu(ui, card, cx);
			});
		});
	});
	// The name and picture take the right click; the menu button keeps its own clicks.
	let mut names = who.response.rect;
	names.max.x = names.max.x.min(kebab_left);
	let response = ui.interact(names, id, Sense::click());
	let hint = tr!("call-volume-hint");
	let response = response.on_hover_text(hint);
	call_volume::popup(&response, card, cx);
	if card.receive_dbfs.is_some() {
		meter(
			ui,
			card.receive_dbfs,
			&tr!("meter-incoming", name = card.name.as_str()),
		);
	}
}

/// The menu of the secondary items.
fn menu(ui: &mut Ui, card: &CallCard, cx: &mut Cx) -> f32 {
	let response = icon_button(ui, Icon::More, tr!("call-more"));
	let left = response.rect.min.x;
	egui::Popup::menu(&response).show(|ui| {
		if !matches!(card.phase, Phase::Connected | Phase::Interrupted) {
			ui.label(tr!("call-more-none"));
			return;
		}
		if ui.button(tr!("call-volume-menu")).clicked() {
			cx.local.volume_ask = Some(card.call_id.clone());
			ui.close();
		}
		let open = cx.local.details_call.as_deref() == Some(card.call_id.as_str());
		let label = if open {
			tr!("details-hide")
		} else {
			tr!("details-show")
		};
		if ui.button(label).clicked() {
			cx.local.details_call = (!open).then(|| card.call_id.clone());
			ui.close();
		}
	});
	left
}

fn buttons(ui: &mut Ui, card: &CallCard, cx: &mut Cx) {
	let actions = cx.actions;
	let call = &card.call;
	let cid = card.call_id.clone();
	let live = matches!(card.phase, Phase::Connected | Phase::Interrupted);
	ui.horizontal(|ui| {
		if live {
			let (mute, deaf) = (call.user_muted, call.user_deafened);
			let (icon, tip) = if mute {
				(Icon::MicOff, tr!("call-unmute"))
			} else {
				(Icon::Mic, tr!("call-mute"))
			};
			if icon_toggle(ui, mute, icon, tip).clicked() {
				let id = cid.clone();
				actions
					.run(|c| async move { c.set_call_controls(&id, Some(!mute), None, None).await });
			}
			let (icon, tip) = if deaf {
				(Icon::HeadphonesOff, tr!("call-undeafen"))
			} else {
				(Icon::Headphones, tr!("call-deafen"))
			};
			if icon_toggle(ui, deaf, icon, tip).clicked() {
				let id = cid.clone();
				actions
					.run(|c| async move { c.set_call_controls(&id, None, Some(!deaf), None).await });
			}
			messages_button(ui, card, cx);
		}
		ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
			let tip = if card.phase == Phase::OutgoingRinging {
				tr!("action-cancel")
			} else {
				tr!("call-end")
			};
			if icon_label_danger(ui, Icon::HangUp, tip).clicked() {
				let id = cid.clone();
				actions.run(|c| async move { c.end_call(&id).await });
			}
		});
	});
}

/// Shows or hides the chat of this call; a badge counts the messages not read yet.
fn messages_button(ui: &mut Ui, card: &CallCard, cx: &mut Cx) {
	let open = chat::is_open(cx.local, &card.call_id);
	let unread = if open {
		0
	} else {
		cx.messages.unread(cx.actions, &card.call_id)
	};
	let response = icon_toggle(ui, open, Icon::Chat, tr!("call-messages-hover"));
	if unread > 0 {
		badge(ui, response.rect, unread);
	}
	if response.clicked() {
		chat::toggle(cx.local, &card.call_id, &card.name, false);
	}
}

/// A small red count on the upper right of a button.
fn badge(ui: &Ui, rect: egui::Rect, count: usize) {
	let text = if count > 9 {
		"9+".to_owned()
	} else {
		count.to_string()
	};
	let center = rect.right_top() + egui::vec2(-6.0, 6.0);
	let painter = ui.painter();
	painter.circle_filled(center, 9.0, theme::ERROR);
	painter.text(
		center,
		egui::Align2::CENTER_CENTER,
		text,
		egui::FontId::proportional(theme::TEXT_SMALL),
		egui::Color32::WHITE,
	);
}
