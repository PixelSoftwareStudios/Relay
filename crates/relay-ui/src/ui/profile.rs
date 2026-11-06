//! Own profile: display name and a profile image chosen with the native file
//! dialog, previewed before it is uploaded.

use std::mem;

use egui::{self, RichText, Ui};

use crate::actions::Actions;
use crate::avatar::Avatar;
use crate::model::AppState;
use crate::viewmodel::local_avatar;

use super::local::Local;
use super::textures::Textures;
use super::widgets::{avatar, field_width};
use crate::tr;

pub fn profile(
	ui: &mut Ui,
	state: &AppState,
	actions: &Actions,
	local: &mut Local,
	textures: &mut Textures,
) {
	if !local.name_seeded
		&& let Some(settings) = &state.settings
	{
		local.name_input = settings.display_name.clone();
		local.name_seeded = true;
	}
	ui.heading(tr!("profile-heading"));
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		ui.horizontal_wrapped(|ui| {
			match &local.picked {
				Some(p) => avatar(ui, &p.texture, 64.0),
				None => {
					let tex = textures.texture(&local_avatar(state), actions);
					avatar(ui, &tex, 64.0);
				}
			}
			ui.vertical(|ui| {
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
				let stored = local_avatar(state) != Avatar::Placeholder;
				image_buttons(ui, actions, local, stored);
			});
		});
		ui.label(RichText::new(tr!("profile-image-note")).weak());
	});
}

fn image_buttons(ui: &mut Ui, actions: &Actions, local: &mut Local, stored: bool) {
	ui.horizontal_wrapped(|ui| {
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
		} else if stored && ui.button(tr!("profile-remove")).clicked() {
			actions.run(|c| async move { c.clear_avatar().await });
		}
	});
}
