//! Channels (create, join, list, leave, invite) and privacy settings.

use std::mem;

use egui::{self, RichText, Ui};
use relay_client::ChannelOptions;

use crate::actions::Actions;
use crate::model::{Advert, AppState, Channel, Inbound};

use super::local::{INVITE_TTLS, INVITE_USES, Local};

pub fn channels(ui: &mut Ui, state: &AppState, actions: &Actions, local: &mut Local) {
	ui.heading("Channels");
	ui.label(
		RichText::new(
			"A channel decides who can see and call you. It never turns your microphone on.",
		)
		.weak(),
	);
	if state.channels.is_empty() {
		ui.label(RichText::new("You are not in any channel.").weak());
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
	let mut parts = vec![if channel.listed { "listed" } else { "hidden" }];
	if channel.password {
		parts.push("password");
	}
	if channel.invite_only {
		parts.push("invite only");
	}
	parts.join(", ")
}

fn channel_row(ui: &mut Ui, channel: &Channel, actions: &Actions, local: &mut Local) {
	ui.horizontal(|ui| {
		ui.vertical(|ui| {
			ui.label(RichText::new(&channel.name).strong());
			ui.label(
				RichText::new(format!(
					"{} · {} · {} shared peer(s)",
					channel.role,
					flags(channel),
					channel.shared_peers
				))
				.weak(),
			);
		});
		ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
			leave_button(ui, channel, actions, local);
			if channel.can_admin() && ui.button("Invite…").clicked() {
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
		if ui.button("Yes, leave").clicked() {
			let id = channel.channel_id.clone();
			actions.run(|c| async move { c.leave_channel(&id).await });
			local.leave_confirm = None;
		}
		if ui.button("Stay").clicked() {
			local.leave_confirm = None;
		}
	} else if ui.button("Leave").clicked() {
		local.leave_confirm = Some(channel.channel_id.clone());
	}
}

fn invite_form(ui: &mut Ui, channel: &Channel, actions: &Actions, local: &mut Local) {
	ui.horizontal(|ui| {
		egui::ComboBox::from_id_salt("invite-ttl")
			.selected_text(INVITE_TTLS[local.invite_ttl].0)
			.show_ui(ui, |ui| {
				for (i, (label, _)) in INVITE_TTLS.iter().enumerate() {
					ui.selectable_value(&mut local.invite_ttl, i, *label);
				}
			});
		egui::ComboBox::from_id_salt("invite-uses")
			.selected_text(format!("{} use(s)", INVITE_USES[local.invite_uses]))
			.show_ui(ui, |ui| {
				for (i, n) in INVITE_USES.iter().enumerate() {
					ui.selectable_value(&mut local.invite_uses, i, format!("{n} use(s)"));
				}
			});
		if ui.button("Create invite").clicked() {
			actions.create_invite(
				channel.channel_id.clone(),
				INVITE_TTLS[local.invite_ttl].1,
				INVITE_USES[local.invite_uses],
			);
		}
		if ui.button("Close").clicked() {
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
		ui.label(
			"Invite token (secret). Anyone who has it can join until it expires or is used up:",
		);
		ui.add(
			egui::Label::new(RichText::new(&token).monospace())
				.selectable(true)
				.wrap(),
		);
		ui.horizontal(|ui| {
			if ui.button("Copy").clicked() {
				actions.copy_text(&token);
			}
			if ui.button("Hide").clicked() {
				local.invite = None;
			}
		});
	});
}

fn create_form(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	egui::CollapsingHeader::new("Create a channel").show(ui, |ui| {
		let f = &mut local.new_channel;
		ui.add(
			egui::TextEdit::singleline(&mut f.name)
				.hint_text("Channel name")
				.char_limit(32),
		);
		ui.checkbox(&mut f.listed, "Listed: show the name to members' devices");
		ui.checkbox(&mut f.password_required, "Require a password to join");
		ui.checkbox(&mut f.invite_only, "Invite only");
		if f.password_required {
			ui.add(
				egui::TextEdit::singleline(&mut f.password)
					.password(true)
					.hint_text("Password"),
			);
		}
		let ready = !f.name.trim().is_empty() && (!f.password_required || !f.password.is_empty());
		if ui.add_enabled(ready, egui::Button::new("Create")).clicked() {
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
	egui::CollapsingHeader::new("Join a channel").show(ui, |ui| {
		ui.label("Paste an invite token, or a channel id with its password.");
		ui.add(
			egui::TextEdit::singleline(&mut local.join_target).hint_text("RLYI1… or channel id"),
		);
		ui.add(
			egui::TextEdit::singleline(&mut local.join_password)
				.password(true)
				.hint_text("Password (not needed for an invite)"),
		);
		if ui
			.add_enabled(
				!local.join_target.trim().is_empty(),
				egui::Button::new("Join"),
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
	ui.heading("Privacy");
	let Some(settings) = &state.settings else {
		ui.label(RichText::new("Privacy settings appear once the daemon is connected.").weak());
		return;
	};
	egui::Frame::group(ui.style()).show(ui, |ui| {
		ui.set_width(ui.available_width());
		let inbound_label = |v: Inbound| match v {
			Inbound::Anyone => "Anyone on the network",
			Inbound::SharedChannelOnly => "Only members of my channels",
		};
		let mut inbound = settings.inbound;
		egui::ComboBox::from_label("Who can ring me")
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
			Advert::Visible => "Visible to nearby devices",
			Advert::Hidden => "Hidden (only reachable by connection string)",
		};
		let mut advert = settings.advert;
		egui::ComboBox::from_label("Network visibility")
			.selected_text(advert_label(advert))
			.show_ui(ui, |ui| {
				for v in [Advert::Visible, Advert::Hidden] {
					ui.selectable_value(&mut advert, v, advert_label(v));
				}
			});
		if advert != settings.advert {
			actions.run(|c| async move { c.set_privacy(None, Some(advert)).await });
		}
	});
}
