//! The microphone part of a call card: a three-way choice (Muted, Unmuted, Push to talk), what
//! the microphone is doing now, the level meter while it is live and, in push-to-talk, the big
//! hold-to-talk button.

use egui::{self, RichText, Ui};

use crate::actions::Actions;
use crate::capabilities::Capabilities;
use crate::icons::Icon;
use crate::model::Mode;
use crate::theme;
use crate::viewmodel::CallCard;

use super::kit::{self, Cx};
use super::local::Local;
use super::widgets::{hold_button, meter, tone_color};
use crate::tr;

pub fn show(ui: &mut Ui, card: &CallCard, cx: &mut Cx) {
	let ptt_on = cx.state.settings.as_ref().is_none_or(|s| s.ptt_enabled);
	choice(ui, card, cx.actions, ptt_on);
	let t = &card.transmit;
	ui.add_space(theme::SPACE_S);
	ui.label(RichText::new(t.label).strong().color(tone_color(t.tone, ui)));
	if !t.detail.is_empty() {
		kit::help(ui, &t.detail);
	}
	let mode = card.call.transmit.mode;
	if mode == Mode::PushToTalk {
		ui.add_space(theme::SPACE_S);
		talk_button(ui, card, cx.actions, cx.local);
		let caps = Capabilities::of(&*cx.actions.shell);
		if caps.global_hotkey {
			kit::help(ui, tr!("hotkey-note-global"));
		}
	}
	if card.call.transmit.effective_transmitting {
		meter(ui, cx.meters.microphone_dbfs, tr!("meter-yours"));
	}
}

/// The segmented control: one button per mode, the active one filled. Push to talk is left
/// out while the setting is off (unless the call is in it, so the state is never hidden).
fn choice(ui: &mut Ui, card: &CallCard, actions: &Actions, ptt_on: bool) {
	let current = card.call.transmit.mode;
	let modes: Vec<(Mode, Icon, &str)> = [
		(Mode::Muted, Icon::MicOff, tr!("mode-muted")),
		(Mode::Unmuted, Icon::Mic, tr!("mode-unmuted")),
		(Mode::PushToTalk, Icon::PushToTalk, tr!("mode-ptt")),
	]
	.into_iter()
	.filter(|(m, _, _)| *m != Mode::PushToTalk || ptt_on || current == Mode::PushToTalk)
	.collect();
	let gap = ui.spacing().item_spacing.x;
	let width = (ui.available_width() - gap * (modes.len() as f32 - 1.0)) / modes.len() as f32;
	ui.horizontal(|ui| {
		for (mode, icon, label) in modes {
			let selected = current == mode;
			let response = segment(ui, width, selected, icon, label);
			if response.clicked() && !selected {
				let id = card.call_id.clone();
				actions.run(|c| async move { c.set_transmit(&id, mode).await });
			}
		}
	});
}

/// One segment: icon over a short label, filled when it is the active mode.
fn segment(ui: &mut Ui, width: f32, selected: bool, icon: Icon, label: &str) -> egui::Response {
	let height = ui.spacing().interact_size.y.max(44.0);
	let (rect, response) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::click());
	response.widget_info(|| {
		let mut info = egui::WidgetInfo::labeled(egui::Role::Button, true, label);
		info.selected = Some(selected);
		info
	});
	let visuals = ui.style().interact_selectable(&response, selected);
	let painter = ui.painter();
	painter.rect(
		rect,
		theme::CARD_RADIUS,
		if selected {
			ui.visuals().selection.bg_fill
		} else {
			visuals.weak_bg_fill
		},
		if selected {
			egui::Stroke::new(2.0, ui.visuals().selection.stroke.color)
		} else {
			visuals.bg_stroke
		},
		egui::StrokeKind::Inside,
	);
	let color = if selected {
		ui.visuals().selection.stroke.color
	} else {
		visuals.text_color()
	};
	painter.text(
		rect.center_top() + egui::vec2(0.0, 5.0 + 9.0),
		egui::Align2::CENTER_CENTER,
		icon.codepoint(),
		icon.font(18.0),
		color,
	);
	let galley = painter.layout(
		label.to_owned(),
		egui::FontId::proportional(theme::TEXT_SMALL + 1.0),
		color,
		width - 6.0,
	);
	painter.galley(
		egui::pos2(rect.center().x - galley.size().x / 2.0, rect.max.y - galley.size().y - 4.0),
		galley,
		color,
	);
	response
}

/// The on-screen hold-to-talk button: pressing it tells the shell the key went down, letting go
/// (or the button vanishing) that it went up.
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
