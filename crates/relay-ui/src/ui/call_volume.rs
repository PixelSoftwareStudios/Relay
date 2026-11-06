//! The volume of the other person in a call, like a chat app's per-user volume: a small popup
//! with a 0 to 200 % slider and a reset. It opens on a right click or long press of the name
//! on the call card, or from the card's menu.

use egui::{self, PopupCloseBehavior, Response, SetOpenCommand, Ui};

use crate::format::gain_percent;
use crate::viewmodel::CallCard;

use super::kit::Cx;
use crate::tr;

/// Width of the popup in points.
const WIDTH: f32 = 240.0;

/// Shows the popup anchored under `anchor` when it was asked for.
pub fn popup(anchor: &Response, card: &CallCard, cx: &mut Cx) {
	let asked = cx.local.volume_ask.as_deref() == Some(card.call_id.as_str());
	if asked {
		cx.local.volume_ask = None;
	}
	let command = (asked || anchor.secondary_clicked()).then_some(SetOpenCommand::Bool(true));
	egui::Popup::from_response(anchor)
		.open_memory(command)
		.close_behavior(PopupCloseBehavior::CloseOnClickOutside)
		.width(WIDTH)
		.show(|ui| content(ui, card, cx));
}

fn content(ui: &mut Ui, card: &CallCard, cx: &mut Cx) {
	ui.set_width(WIDTH);
	ui.label(egui::RichText::new(tr!("call-volume-title", name = card.name.as_str())).strong());
	let current = card.call.receive_gain;
	let edit = cx
		.local
		.gain_edits
		.entry(format!("gain-{}", card.call_id))
		.or_default();
	let mut value = edit.unwrap_or(current);
	let slider = ui.add(
		egui::Slider::new(&mut value, 0.0..=2.0)
			.custom_formatter(|v, _| gain_percent(v as f32))
			.custom_parser(|text| {
				text.trim()
					.trim_end_matches('%')
					.parse::<f64>()
					.ok()
					.map(|p| p / 100.0)
			}),
	);
	let mut send = None;
	if slider.dragged() || (slider.changed() && edit.is_some()) {
		*edit = Some(value);
	}
	if slider.drag_stopped() || (slider.changed() && !slider.dragged()) {
		*edit = None;
		send = Some(value);
	}
	if ui
		.add_enabled(
			(value - 1.0).abs() > 0.005,
			egui::Button::new(tr!("call-volume-reset")),
		)
		.clicked()
	{
		*edit = None;
		send = Some(1.0);
	}
	if let Some(gain) = send {
		let id = card.call_id.clone();
		cx.actions
			.run(|c| async move { c.set_call_controls(&id, None, None, Some(gain)).await });
	}
}
