//! The Channels page: one card with the create and join actions (and their forms), the list and invites,
//! then the listed channels other devices advertise (`channels_nearby`).

use std::mem;

use egui::{self, RichText, Ui};
use relay_client::ChannelOptions;
use relay_io::LockExt;

use crate::actions::Actions;
use crate::model::Channel;

use super::kit::{self, Cx};
use super::local::{ChannelForm, INVITE_USES, Local, invite_ttls};
use super::widgets::{button_row, field_width};
use crate::icons::{
	Icon, icon_button, icon_inline, icon_label, icon_label_enabled, icon_label_widget,
};
use crate::tr;

/// The page: with no channel, one card that explains channels and holds the two actions;
/// otherwise the same two actions in one card at the top, then the list.
pub fn show(ui: &mut Ui, cx: &mut Cx) {
	kit::page_title(ui, tr!("channels-heading"));
	let (actions, local) = (cx.actions, &mut *cx.local);
	open_join_for_failed_name(local);
	kit::card(ui, |ui| {
		if cx.state.channels.is_empty() {
			kit::help(ui, tr!("channels-none-help"));
		}
		action_buttons(ui, local);
		match local.channel_form {
			Some(ChannelForm::Create) => create_form(ui, actions, local),
			Some(ChannelForm::Join) => join_form(ui, actions, local),
			None => {}
		}
	});
	kit::gap(ui);
	invite_box(ui, actions, local);
	your_channels(ui, cx);
	super::channels_nearby::section(ui, cx);
}

/// "Your channels": the channels this device is in, one calm card with a row each.
fn your_channels(ui: &mut Ui, cx: &mut Cx) {
	kit::section_title(ui, tr!("channels-yours"));
	let (actions, local) = (cx.actions, &mut *cx.local);
	kit::card(ui, |ui| {
		let Some(last) = cx.state.channels.len().checked_sub(1) else {
			ui.label(RichText::new(tr!("channels-none")).color(ui.visuals().weak_text_color()));
			return;
		};
		for (i, channel) in cx.state.channels.iter().enumerate() {
			channel_row(ui, channel, actions, local);
			if i < last {
				ui.separator();
			}
		}
	});
	kit::gap(ui);
}

/// A join started from the nearby list was refused (most likely it needs a password): show the
/// join form with the name in place.
fn open_join_for_failed_name(local: &mut Local) {
	let failed = local.join_failed.lock_unpoisoned().take();
	if let Some(name) = failed {
		local.join_target.clone_from(&name);
		local.join_prefill = Some(name);
		local.join_password.clear();
		local.channel_form = Some(ChannelForm::Join);
	}
}

/// "Create a channel" and "Join a channel": each opens its form below, a second press closes it.
fn action_buttons(ui: &mut Ui, local: &mut Local) {
	button_row(ui, |ui| {
		for (form, icon, text) in [
			(
				ChannelForm::Create,
				Icon::Add,
				tr!("channel-create-heading"),
			),
			(
				ChannelForm::Join,
				Icon::Channels,
				tr!("channel-join-heading"),
			),
		] {
			let open = local.channel_form == Some(form);
			let button = icon_label_widget(ui, icon, text).selected(open);
			if ui.add(button).clicked() {
				local.channel_form = if open { None } else { Some(form) };
			}
		}
	});
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
		ui.label(RichText::new(&channel.name).strong());
		if channel.password || channel.invite_only {
			let why = if channel.password {
				tr!("channel-password")
			} else {
				tr!("channel-invite-only")
			};
			icon_inline(ui, Icon::Lock, ui.visuals().weak_text_color()).on_hover_text(why);
		}
	});
	ui.label(
		RichText::new(tr!(
			"channel-summary",
			role = channel.role.as_str(),
			flags = flags(channel),
			peers = channel.shared_peers
		))
		.weak(),
	);
	button_row(ui, |ui| {
		if channel.can_admin() && icon_label(ui, Icon::AddContact, tr!("channel-invite")).clicked()
		{
			local.invite_channel = Some(channel.channel_id.clone());
			local.invite = None;
		}
		leave_button(ui, channel, actions, local);
	});
	if local.invite_channel.as_deref() == Some(&channel.channel_id) {
		invite_form(ui, channel, actions, local);
	}
}

fn leave_button(ui: &mut Ui, channel: &Channel, actions: &Actions, local: &mut Local) {
	if local.leave_confirm.as_deref() == Some(&channel.channel_id) {
		if kit::danger_button(ui, tr!("channel-leave-yes")).clicked() {
			let id = channel.channel_id.clone();
			actions.run(|c| async move { c.leave_channel(&id).await });
			local.leave_confirm = None;
		}
		if ui.button(tr!("channel-leave-stay")).clicked() {
			local.leave_confirm = None;
		}
	} else if icon_label(ui, Icon::SignOut, tr!("channel-leave")).clicked() {
		local.leave_confirm = Some(channel.channel_id.clone());
	}
}

fn invite_form(ui: &mut Ui, channel: &Channel, actions: &Actions, local: &mut Local) {
	button_row(ui, |ui| {
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
		if icon_label(ui, Icon::Add, tr!("invite-create")).clicked() {
			actions.create_invite(
				channel.channel_id.clone(),
				invite_ttls()[local.invite_ttl].1,
				INVITE_USES[local.invite_uses],
			);
		}
		if icon_button(ui, Icon::Close, tr!("action-close")).clicked() {
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
	kit::card(ui, |ui| {
		ui.label(tr!("invite-token-note"));
		ui.add(
			egui::Label::new(RichText::new(&token).monospace())
				.selectable(true)
				.wrap(),
		);
		button_row(ui, |ui| {
			if icon_button(ui, Icon::Copy, tr!("action-copy")).clicked() {
				actions.copy_text(&token);
			}
			if icon_button(ui, Icon::EyeOff, tr!("action-hide")).clicked() {
				local.invite = None;
			}
		});
	});
	kit::gap(ui);
}

fn create_form(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	let f = &mut local.new_channel;
	ui.add(
		egui::TextEdit::singleline(&mut f.name)
			.desired_width(field_width(ui))
			.hint_text(tr!("channel-name-hint"))
			.char_limit(32),
	);
	ui.checkbox(&mut f.listed, tr!("channel-listed-label"));
	ui.checkbox(&mut f.password_required, tr!("channel-password-label"));
	ui.checkbox(&mut f.invite_only, tr!("channel-invite-only-label"));
	if f.password_required {
		ui.add(
			egui::TextEdit::singleline(&mut f.password)
				.desired_width(field_width(ui))
				.password(true)
				.hint_text(tr!("password-hint")),
		);
	}
	let ready = !f.name.trim().is_empty() && (!f.password_required || !f.password.is_empty());
	if icon_label_enabled(ui, ready, Icon::Add, tr!("action-create")).clicked() {
		let options = ChannelOptions {
			listed: f.listed,
			password: f.password_required,
			invite_only: f.invite_only,
		};
		let (name, password) = (mem::take(&mut f.name), mem::take(&mut f.password));
		actions.run(|c| async move { c.create_channel(&name, options, &password).await });
		local.channel_form = None;
	}
}

fn join_form(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	ui.label(tr!("channel-join-note"));
	ui.add(
		egui::TextEdit::singleline(&mut local.join_target)
			.desired_width(field_width(ui))
			.hint_text(tr!("channel-join-hint")),
	);
	if local.join_prefill.as_deref() != Some(local.join_target.trim()) {
		local.join_prefill = None;
	}
	ui.add(
		egui::TextEdit::singleline(&mut local.join_password)
			.desired_width(field_width(ui))
			.password(true)
			.hint_text(tr!("password-optional-hint")),
	);
	if icon_label_enabled(
		ui,
		!local.join_target.trim().is_empty(),
		Icon::Channels,
		tr!("action-join"),
	)
	.clicked()
	{
		let target = mem::take(&mut local.join_target);
		let password = mem::take(&mut local.join_password);
		if local.join_prefill.take().as_deref() == Some(target.trim()) {
			actions.run(|c| async move { c.join_by_name(&target, &password).await });
		} else {
			actions.run(|c| async move { c.join_channel(&target, &password).await });
		}
		local.channel_form = None;
	}
}
