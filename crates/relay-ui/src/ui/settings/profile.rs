//! Profile: the name other people see and a profile picture chosen with the
//! native file dialog, previewed before it is uploaded.

use std::mem;

use egui::{self, Ui};

use crate::actions::Actions;
use crate::avatar::Avatar;
use crate::theme;
use crate::viewmodel::local_avatar;

use super::super::kit::{self, Cx};
use super::super::local::Local;
use super::super::widgets::{avatar, button_row, field_width};
use crate::tr;

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	let (state, actions) = (cx.state, cx.actions);
	if !cx.local.name_seeded
		&& let Some(settings) = &state.settings
	{
		cx.local.name_input = settings.display_name.clone();
		cx.local.name_seeded = true;
	}
	kit::card(ui, |ui| {
		ui.horizontal_wrapped(|ui| {
			match &cx.local.picked {
				Some(p) => avatar(ui, &p.texture, theme::AVATAR_PROFILE),
				None => {
					let tex = cx.textures.texture(&local_avatar(state), actions);
					avatar(ui, &tex, theme::AVATAR_PROFILE);
				}
			}
			let stored = local_avatar(state) != Avatar::Placeholder;
			ui.vertical(|ui| image_buttons(ui, actions, cx.local, stored));
		});
		kit::help(ui, tr!("profile-image-note"));
	});
	kit::gap(ui);
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("profile-name-label"));
		let local = &mut *cx.local;
		ui.horizontal_wrapped(|ui| {
			ui.add(
				egui::TextEdit::singleline(&mut local.name_input)
					.desired_width(field_width(ui))
					.hint_text(tr!("profile-name-hint"))
					.char_limit(64),
			);
			let unchanged = state
				.settings
				.as_ref()
				.is_some_and(|s| s.display_name == local.name_input.trim());
			let ready = !local.name_input.trim().is_empty() && !unchanged;
			if ui
				.add_enabled(ready, egui::Button::new(tr!("profile-save-name")))
				.clicked()
			{
				let name = local.name_input.clone();
				actions.run(|c| async move { c.set_profile(&name).await });
			}
		});
		kit::help(ui, tr!("profile-name-help"));
	});
}

fn image_buttons(ui: &mut Ui, actions: &Actions, local: &mut Local, stored: bool) {
	button_row(ui, |ui| {
		if ui.button(tr!("profile-choose-image")).clicked() {
			actions.pick_avatar();
		}
		if local.picked.is_some() {
			if ui.button(tr!("profile-upload")).clicked()
				&& let Some(picked) = local.picked.take()
			{
				let (bytes, media) = (picked.bytes, picked.media_type);
				actions.run(|c| async move { c.upload_avatar(bytes, media).await });
			}
			// Only drops the pending choice; the stored image is untouched.
			if ui.button(tr!("profile-discard")).clicked() {
				mem::take(&mut local.picked);
			}
		} else if stored {
			remove_picture(ui, actions, local);
		}
	});
}

/// Two-step: removing the picture also tells trusted peers to stop showing it.
fn remove_picture(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	if !local.confirm_remove_avatar {
		if ui.button(tr!("profile-remove")).clicked() {
			local.confirm_remove_avatar = true;
		}
		return;
	}
	ui.label(tr!("profile-remove-ask"));
	if kit::danger_button(ui, tr!("profile-remove-yes")).clicked() {
		local.confirm_remove_avatar = false;
		actions.run(|c| async move { c.clear_avatar().await });
	}
	if ui.button(tr!("action-cancel")).clicked() {
		local.confirm_remove_avatar = false;
	}
}
