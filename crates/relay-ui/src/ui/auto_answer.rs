//! Auto-answer on a contact card: the menu entry, the confirmation under the card and the
//! "Answers automatically" line. Only a verified contact can be switched on; the engine
//! checks again.

use egui::{RichText, Ui};

use crate::icons::{Icon, icon_label};
use crate::theme;
use crate::viewmodel::PeerRow;

use super::kit::{self, Cx};
use super::widgets::button_row;
use crate::tr;

/// The menu entry, with a check mark while it is on. Switching on asks first (the card shows
/// the question); switching off acts at once. Greyed, with the reason, for a contact that
/// is not verified (one that already answers automatically can always be switched off).
pub fn menu_entry(ui: &mut Ui, cx: &mut Cx, row: &PeerRow) {
	let label = tr!("contact-auto-answer");
	if row.auto_answer {
		if icon_label(ui, Icon::Check, label).clicked() {
			let id = row.peer_id.clone();
			cx.actions
				.run(|c| async move { c.set_peer_auto_answer(&id, false).await });
			ui.close();
		}
		return;
	}
	if ui
		.add_enabled(row.verified, egui::Button::new(label))
		.clicked()
	{
		cx.local.auto_answer_confirm = Some(row.peer_id.clone());
		ui.close();
	}
	if !row.verified {
		kit::help(ui, tr!("contact-auto-answer-unverified"));
	}
}

/// The question under a contact whose entry was chosen: the first step is the menu entry,
/// Yes acts.
pub fn ask(ui: &mut Ui, cx: &mut Cx, row: &PeerRow) {
	if cx.local.auto_answer_confirm.as_deref() != Some(row.peer_id.as_str()) {
		return;
	}
	ui.label(tr!(
		"auto-answer-ask",
		name = row.name.as_str(),
		mode = tr!("calls-default-mode")
	));
	button_row(ui, |ui| {
		if ui.button(tr!("auto-answer-yes")).clicked() {
			let id = row.peer_id.clone();
			cx.actions
				.run(|c| async move { c.set_peer_auto_answer(&id, true).await });
			cx.local.auto_answer_confirm = None;
		}
		if ui.button(tr!("action-cancel")).clicked() {
			cx.local.auto_answer_confirm = None;
		}
	});
}

/// A small line in the card while the contact is answered automatically.
pub fn badge(ui: &mut Ui, row: &PeerRow) {
	if row.auto_answer {
		ui.label(
			RichText::new(tr!("contact-auto-answer-badge"))
				.small()
				.color(theme::LIVE),
		);
	}
}
