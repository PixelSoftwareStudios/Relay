//! The desktop-only switches of "This app": call notifications and start at login. They
//! draw what the shell reports and change nothing locally, so the text is always the truth.

use egui::{RichText, Ui};

use crate::actions::Actions;
use crate::host_options::{HostOptions, login_text, notify_text};
use crate::i18n::picker;
use crate::tr;

pub fn draw(ui: &mut Ui, actions: &Actions) {
	let Some(options) = actions.shell.host_options() else {
		return;
	};
	language_picker(ui, options);
	let mut notify = options.notify_incoming();
	if ui.checkbox(&mut notify, tr!("notify-label")).changed() {
		options.set_notify_incoming(notify);
	}
	ui.label(RichText::new(notify_text(notify, options.notify_failure().as_deref())).weak());

	let status = options.login_status();
	let supported = !matches!(status, crate::host_options::LoginStatus::Unsupported(_));
	let mut at_login = status == crate::host_options::LoginStatus::On;
	if ui
		.add_enabled(
			supported,
			egui::Checkbox::new(&mut at_login, tr!("login-label")),
		)
		.changed()
		&& let Err(reason) = options.set_launch_at_login(at_login)
	{
		actions
			.shared
			.set_error(tr!("login-error", reason = reason));
	}
	ui.label(RichText::new(login_text(&status)).weak());
}

/// The language picker: "System (<detected>)" first, then every language by
/// its own native name, English first. The shell stores the choice (`System`
/// stores none); the next frame applies it.
fn language_picker(ui: &mut Ui, options: &dyn HostOptions) {
	let choice = options.language_choice();
	let detected = options.system_language();
	ui.horizontal(|ui| {
		ui.label(tr!("language-label"));
		egui::ComboBox::from_id_salt("ui-language")
			.selected_text(picker::choice_label(choice, detected))
			.show_ui(ui, |ui| {
				for (entry, label) in picker::entries(detected) {
					if ui.selectable_label(entry == choice, label).clicked() {
						options.set_language_choice(entry);
					}
				}
			});
	});
	if picker::EXPERIMENTAL.contains(&options.language()) {
		ui.label(RichText::new(tr!("language-experimental")).weak());
	}
}
