//! Language: "System" first, then every language by its own name.

use egui::Ui;

use crate::i18n::picker;

use super::super::kit::{self, Cx};
use super::super::widgets::combo;
use crate::tr;

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	let Some(options) = cx.actions.shell.host_options() else {
		return;
	};
	kit::card(ui, |ui| {
		let choice = options.language_choice();
		let detected = options.system_language();
		combo(
			ui,
			tr!("language-label"),
			&picker::choice_label(choice, detected),
		)
		.show_ui(ui, |ui| {
			for (entry, label) in picker::entries(detected) {
				if ui.selectable_label(entry == choice, label).clicked() {
					options.set_language_choice(entry);
				}
			}
		});
		kit::help(ui, tr!("language-help"));
		if picker::EXPERIMENTAL.contains(&options.language()) {
			kit::help(ui, tr!("language-experimental"));
		}
	});
}
