//! Own profile: display name and a profile image chosen with the native file
//! dialog, previewed before it is uploaded.

use std::mem;

use egui::{self, RichText, Ui};

use crate::actions::Actions;
use crate::model::AppState;
use crate::viewmodel::local_avatar;

use super::local::Local;
use super::textures::Textures;
use super::widgets::avatar;

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
	ui.heading("Your profile");
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		ui.horizontal(|ui| {
			match &local.picked {
				Some(p) => avatar(ui, &p.texture, 64.0),
				None => {
					let tex = textures.texture(&local_avatar(state), actions);
					avatar(ui, &tex, 64.0);
				}
			}
			ui.vertical(|ui| {
				ui.horizontal(|ui| {
					ui.add(
						egui::TextEdit::singleline(&mut local.name_input)
							.hint_text("Display name")
							.char_limit(64),
					);
					let unchanged = state
						.settings
						.as_ref()
						.is_some_and(|s| s.display_name == local.name_input.trim());
					let ready = !local.name_input.trim().is_empty() && !unchanged;
					if ui
						.add_enabled(ready, egui::Button::new("Save name"))
						.clicked()
					{
						let name = local.name_input.clone();
						actions.run(|c| async move { c.set_profile(&name).await });
					}
				});
				image_buttons(ui, actions, local);
			});
		});
		ui.label(
			RichText::new(
				"JPEG or PNG up to 1 MiB. The engine scales it and shares it with peers. \
				 Removing a stored image needs an engine request that does not exist yet.",
			)
			.weak(),
		);
	});
}

fn image_buttons(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	ui.horizontal(|ui| {
		if ui.button("Choose image…").clicked() {
			actions.pick_avatar();
		}
		if local.picked.is_some() {
			if ui.button("Upload").clicked()
				&& let Some(picked) = local.picked.take()
			{
				let (bytes, media) = (picked.bytes, picked.media_type);
				actions.run(|c| async move { c.upload_avatar(bytes, media).await });
			}
			if ui.button("Discard").clicked() {
				mem::take(&mut local.picked);
			}
		}
	});
}
