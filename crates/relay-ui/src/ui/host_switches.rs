//! The desktop-only switches of "This app": call notifications and start at login. They
//! draw what the shell reports and change nothing locally, so the text is always the truth.

use egui::{RichText, Ui};

use crate::actions::Actions;
use crate::host_options::{login_text, notify_text};

pub fn draw(ui: &mut Ui, actions: &Actions) {
	let Some(options) = actions.shell.host_options() else {
		return;
	};
	let mut notify = options.notify_incoming();
	if ui
		.checkbox(&mut notify, "Notify on incoming calls")
		.changed()
	{
		options.set_notify_incoming(notify);
	}
	ui.label(RichText::new(notify_text(notify, options.notify_failure().as_deref())).weak());

	let status = options.login_status();
	let supported = !matches!(status, crate::host_options::LoginStatus::Unsupported(_));
	let mut at_login = status == crate::host_options::LoginStatus::On;
	if ui
		.add_enabled(
			supported,
			egui::Checkbox::new(&mut at_login, "Start this app at login"),
		)
		.changed()
		&& let Err(reason) = options.set_launch_at_login(at_login)
	{
		actions
			.shared
			.set_error(format!("Start at login: {reason}"));
	}
	ui.label(RichText::new(login_text(&status)).weak());
}
