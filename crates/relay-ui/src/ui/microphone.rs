//! The microphone card on the Calls page: asks for the permission, or says
//! where to turn it on when the system will not ask again.

use egui::Ui;

use crate::microphone::{MicrophoneCard, MicrophoneChoice};
use crate::tr;

use super::kit::{self, Cx};

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	let access = cx.actions.shell.microphone_access();
	let dismissed = cx.local.microphone.is_dismissed(access);
	let Some(card) = crate::microphone::card(access, dismissed) else {
		return;
	};
	let mut choice = None;
	kit::card(ui, |ui| {
		let (title, body, button) = match card {
			MicrophoneCard::Ask => (
				tr!("mic-ask-title"),
				tr!("mic-ask-body"),
				(tr!("mic-allow"), MicrophoneChoice::Allow),
			),
			MicrophoneCard::TurnedOff => (
				tr!("mic-off-title"),
				tr!("mic-off-body"),
				(tr!("mic-open-settings"), MicrophoneChoice::OpenSettings),
			),
		};
		kit::card_title(ui, title);
		kit::help(ui, body);
		ui.horizontal(|ui| {
			if ui.button(button.0).clicked() {
				choice = Some(button.1);
			}
			if ui.button(tr!("action-dismiss")).clicked() {
				choice = Some(MicrophoneChoice::Dismiss);
			}
		});
	});
	kit::gap(ui);
	if let Some(choice) = choice {
		apply(choice, cx);
	}
}

/// Forward the user's choice to the shell or remember the dismissal.
pub fn apply(choice: MicrophoneChoice, cx: &mut Cx) {
	match choice {
		MicrophoneChoice::Allow => cx.actions.shell.request_microphone(),
		MicrophoneChoice::OpenSettings => cx.actions.shell.open_microphone_settings(),
		MicrophoneChoice::Dismiss => cx.local.microphone.dismiss(),
	}
}
