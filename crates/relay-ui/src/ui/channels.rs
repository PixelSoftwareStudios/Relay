//! Channels (create, join, list, leave, invite) and privacy settings.

use std::mem;

use egui::{self, RichText, Ui};
use relay_client::ChannelOptions;

use crate::actions::Actions;
use crate::model::{Advert, AppState, Channel, Inbound};

use super::local::{INVITE_USES, Local, invite_ttls};
use crate::tr;

pub fn channels(ui: &mut Ui, state: &AppState, actions: &Actions, local: &mut Local) {
	ui.heading(tr!("channels-heading"));
	ui.label(RichText::new(tr!("channels-note")).weak());
	if state.channels.is_empty() {
		ui.label(RichText::new(tr!("channels-none")).weak());
	}
	for channel in &state.channels {
		egui::Frame::group(ui.style()).show(ui, |ui| {
			ui.set_width(ui.available_width());
			channel_row(ui, channel, actions, local);
		});
	}
	invite_box(ui, actions, local);
	create_form(ui, actions, local);
	join_form(ui, actions, local);
}

fn flags(channel: &Channel) -> String {
	let mut parts = vec![if channel.listed {
		tr!("channel-listed")
	} else {
		tr!("channel-hidden")
	}];
	if channel.password {
		parts.push(tr!("channel-password"));
	}
	if channel.invite_only {
		parts.push(tr!("channel-invite-only"));
	}
	parts.join(", ")
}

fn channel_row(ui: &mut Ui, channel: &Channel, actions: &Actions, local: &mut Local) {
	ui.horizontal(|ui| {
		ui.vertical(|ui| {
			ui.label(RichText::new(&channel.name).strong());
			ui.label(
				RichText::new(tr!(
					"channel-summary",
					role = channel.role.as_str(),
					flags = flags(channel),
					peers = channel.shared_peers
				))
				.weak(),
			);
		});
		ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
			leave_button(ui, channel, actions, local);
			if channel.can_admin() && ui.button(tr!("channel-invite")).clicked() {
				local.invite_channel = Some(channel.channel_id.clone());
				local.invite = None;
			}
		});
	});
	if local.invite_channel.as_deref() == Some(&channel.channel_id) {
		invite_form(ui, channel, actions, local);
	}
}

fn leave_button(ui: &mut Ui, channel: &Channel, actions: &Actions, local: &mut Local) {
	if local.leave_confirm.as_deref() == Some(&channel.channel_id) {
		if ui.button(tr!("channel-leave-yes")).clicked() {
			let id = channel.channel_id.clone();
			actions.run(|c| async move { c.leave_channel(&id).await });
			local.leave_confirm = None;
		}
		if ui.button(tr!("channel-leave-stay")).clicked() {
			local.leave_confirm = None;
		}
	} else if ui.button(tr!("channel-leave")).clicked() {
		local.leave_confirm = Some(channel.channel_id.clone());
	}
}

fn invite_form(ui: &mut Ui, channel: &Channel, actions: &Actions, local: &mut Local) {
	ui.horizontal(|ui| {
		egui::ComboBox::from_id_salt("invite-ttl")
			.selected_text(invite_ttls()[local.invite_ttl].0)
			.show_ui(ui, |ui| {
				for (i, (label, _)) in invite_ttls().iter().enumerate() {
					ui.selectable_value(&mut local.invite_ttl, i, *label);
				}
			});
		egui::ComboBox::from_id_salt("invite-uses")
			.selected_text(tr!("invite-uses", count = INVITE_USES[local.invite_uses]))
			.show_ui(ui, |ui| {
				for (i, n) in INVITE_USES.iter().enumerate() {
					ui.selectable_value(&mut local.invite_uses, i, tr!("invite-uses", count = *n));
				}
			});
		if ui.button(tr!("invite-create")).clicked() {
			actions.create_invite(
				channel.channel_id.clone(),
				invite_ttls()[local.invite_ttl].1,
				INVITE_USES[local.invite_uses],
			);
		}
		if ui.button(tr!("action-close")).clicked() {
			local.invite_channel = None;
			local.invite = None;
		}
	});
}

/// The minted token, shown until dismissed. It is a secret: it appears only
/// here and in the clipboard, never in logs or state.
fn invite_box(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	let Some(invite) = &local.invite else { return };
	let token = invite.token.expose().to_owned();
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		ui.label(tr!("invite-token-note"));
		ui.add(
			egui::Label::new(RichText::new(&token).monospace())
				.selectable(true)
				.wrap(),
		);
		ui.horizontal(|ui| {
			if ui.button(tr!("action-copy")).clicked() {
				actions.copy_text(&token);
			}
			if ui.button(tr!("action-hide")).clicked() {
				local.invite = None;
			}
		});
	});
}

fn create_form(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	egui::CollapsingHeader::new(tr!("channel-create-heading"))
		.id_salt("channel-create")
		.show(ui, |ui| {
			let f = &mut local.new_channel;
			ui.add(
				egui::TextEdit::singleline(&mut f.name)
					.hint_text(tr!("channel-name-hint"))
					.char_limit(32),
			);
			ui.checkbox(&mut f.listed, tr!("channel-listed-label"));
			ui.checkbox(&mut f.password_required, tr!("channel-password-label"));
			ui.checkbox(&mut f.invite_only, tr!("channel-invite-only-label"));
			if f.password_required {
				ui.add(
					egui::TextEdit::singleline(&mut f.password)
						.password(true)
						.hint_text(tr!("password-hint")),
				);
			}
			let ready =
				!f.name.trim().is_empty() && (!f.password_required || !f.password.is_empty());
			if ui
				.add_enabled(ready, egui::Button::new(tr!("action-create")))
				.clicked()
			{
				let options = ChannelOptions {
					listed: f.listed,
					password: f.password_required,
					invite_only: f.invite_only,
				};
				let (name, password) = (mem::take(&mut f.name), mem::take(&mut f.password));
				actions.run(|c| async move { c.create_channel(&name, options, &password).await });
			}
		});
}

fn join_form(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	egui::CollapsingHeader::new(tr!("channel-join-heading"))
		.id_salt("channel-join")
		.show(ui, |ui| {
			ui.label(tr!("channel-join-note"));
			ui.add(
				egui::TextEdit::singleline(&mut local.join_target)
					.hint_text(tr!("channel-join-hint")),
			);
			ui.add(
				egui::TextEdit::singleline(&mut local.join_password)
					.password(true)
					.hint_text(tr!("password-optional-hint")),
			);
			if ui
				.add_enabled(
					!local.join_target.trim().is_empty(),
					egui::Button::new(tr!("action-join")),
				)
				.clicked()
			{
				let target = mem::take(&mut local.join_target);
				let password = mem::take(&mut local.join_password);
				actions.run(|c| async move { c.join_channel(&target, &password).await });
			}
		});
}

pub fn privacy(ui: &mut Ui, state: &AppState, actions: &Actions) {
	ui.heading(tr!("privacy-heading"));
	let Some(settings) = &state.settings else {
		ui.label(RichText::new(tr!("privacy-none")).weak());
		return;
	};
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		let inbound_label = |v: Inbound| match v {
			Inbound::Anyone => tr!("privacy-inbound-anyone"),
			Inbound::SharedChannelOnly => tr!("privacy-inbound-shared"),
		};
		let mut inbound = settings.inbound;
		egui::ComboBox::from_label(tr!("privacy-who-rings"))
			.selected_text(inbound_label(inbound))
			.show_ui(ui, |ui| {
				for v in [Inbound::Anyone, Inbound::SharedChannelOnly] {
					ui.selectable_value(&mut inbound, v, inbound_label(v));
				}
			});
		if inbound != settings.inbound {
			actions.run(|c| async move { c.set_privacy(Some(inbound), None).await });
		}
		let advert_label = |v: Advert| match v {
			Advert::Visible => tr!("privacy-advert-visible"),
			Advert::Hidden => tr!("privacy-advert-hidden"),
		};
		let mut advert = settings.advert;
		egui::ComboBox::from_label(tr!("privacy-visibility"))
			.selected_text(advert_label(advert))
			.show_ui(ui, |ui| {
				for v in [Advert::Visible, Advert::Hidden] {
					ui.selectable_value(&mut advert, v, advert_label(v));
				}
			});
		if advert != settings.advert {
			actions.run(|c| async move { c.set_privacy(None, Some(advert)).await });
		}
		ui.separator();
		super::messages::privacy_toggle(ui, actions);
	});
}
