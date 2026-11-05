//! The compose box: Enter sends, Shift+Enter adds a line, a live byte
//! counter against the 4 KiB limit, and an explanation instead of a box when
//! sending is not possible.

use egui::{RichText, Ui};

use super::dialogs::ClipFlow;
use crate::messaging::actions::Intent;
use crate::messaging::model::SessionView;
use crate::messaging::rules::{Blocked, Counter, files_blocked};
use crate::theme;
use crate::tr;

/// Remove an Enter press without Shift from this frame's input and say
/// whether there was one. egui's own shortcut matching ignores extra Shift,
/// which would send on Shift+Enter, so the events are inspected directly.
fn take_plain_enter(ui: &Ui) -> bool {
	ui.input_mut(|i| {
		let at = i
			.events
			.iter()
			.position(crate::messaging::rules::is_plain_enter);
		at.map(|n| i.events.remove(n)).is_some()
	})
}

pub fn blocked(
	ui: &mut Ui,
	why: &Blocked,
	session: &SessionView,
	intents: &mut Vec<Intent>,
	confirm_clear: &mut Option<String>,
) {
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		ui.label(RichText::new(&why.reason).color(theme::WARN));
		if why.export_clear {
			ui.horizontal(|ui| {
				if ui.button(tr!("msg-export-json")).clicked() {
					intents.push(export_intent(session));
				}
				if ui.button(tr!("msg-clear")).clicked() {
					*confirm_clear = Some(session.key.clone());
				}
			});
		}
	});
}

pub fn export_intent(session: &SessionView) -> Intent {
	let name: String = session
		.peer_name
		.chars()
		.map(|c| if c.is_alphanumeric() { c } else { '_' })
		.take(32)
		.collect();
	Intent::Export {
		session: session.key.clone(),
		suggested_name: format!("relay-session-{name}.json"),
	}
}

/// The text box and its buttons.
pub fn editor(
	ui: &mut Ui,
	draft: &mut String,
	session: &SessionView,
	clip: &mut ClipFlow,
	intents: &mut Vec<Intent>,
) {
	let id = ui.make_persistent_id(("relay-msg-compose", &session.key));
	let focused = ui.memory(|m| m.has_focus(id));
	let enter = focused && take_plain_enter(ui);
	ui.add(
		egui::TextEdit::multiline(draft)
			.id(id)
			.desired_rows(2)
			.desired_width(f32::INFINITY)
			.hint_text(tr!("msg-compose-hint")),
	);
	let counter = Counter::of(draft);
	ui.horizontal_wrapped(|ui| {
		let label = RichText::new(counter.label()).small();
		ui.label(if counter.over() {
			label.color(theme::ERROR)
		} else {
			label.weak()
		});
		if counter.over() {
			ui.label(
				RichText::new(tr!("msg-too-long"))
					.small()
					.color(theme::ERROR),
			);
		}
	});
	let send = enter
		|| ui
			.add_enabled(counter.sendable(draft), egui::Button::new(tr!("msg-send")))
			.clicked();
	if send && counter.sendable(draft) {
		intents.push(Intent::Send {
			session: session.key.clone(),
			text: std::mem::take(draft),
		});
	}
	ui.horizontal_wrapped(|ui| {
		if ui
			.button(tr!("msg-send-clipboard"))
			.on_hover_text(tr!("msg-send-clipboard-hover"))
			.clicked()
		{
			ui.memory_mut(|m| m.surrender_focus(id));
			ui.ctx()
				.send_viewport_cmd(egui::ViewportCommand::RequestPaste);
			*clip = ClipFlow::Awaiting;
		}
		let blocked = files_blocked(session);
		let offer = ui.add_enabled(blocked.is_none(), egui::Button::new(tr!("msg-offer-file")));
		if let Some(why) = blocked {
			offer.clone().on_disabled_hover_text(why);
		}
		if offer.clicked() {
			intents.push(Intent::OfferFile {
				session: session.key.clone(),
			});
		}
	});
}
