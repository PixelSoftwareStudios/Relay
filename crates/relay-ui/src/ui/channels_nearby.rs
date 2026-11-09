//! "Channels on your network": the listed channels other devices advertise, one calm card.
//!
//! A row joins by name. A password channel (lock icon) first asks for the password in the row; an
//! open one joins at once. Any other refused join opens the join form with the name filled in
//! (the engine answers every refusal alike).

use std::mem;

use egui::{RichText, Ui};

use relay_io::LockExt;

use crate::icons::{Icon, icon_inline, icon_label, icon_label_enabled};
use crate::model::{Link, NearbyChannel};
use crate::tr;

use super::kit::{self, Cx};
use super::widgets::{button_row, field_width};

pub fn section(ui: &mut Ui, cx: &mut Cx) {
	kit::section_title(ui, tr!("nearby-heading"));
	kit::card(ui, |ui| {
		if cx.state.nearby_channels.is_empty() {
			empty(ui, cx.state.link);
			return;
		}
		if let Some(name) = &cx.local.nearby_prompt
			&& !cx.state.nearby_channels.iter().any(|n| &n.name == name)
		{
			cx.local.nearby_prompt = None;
			cx.local.nearby_focused = false;
		}
		let last = cx.state.nearby_channels.len() - 1;
		for (i, channel) in cx.state.nearby_channels.iter().enumerate() {
			row(ui, channel, cx);
			if i < last {
				ui.separator();
			}
		}
	});
	kit::gap(ui);
}

/// One muted line; offline adds why nothing can be found.
fn empty(ui: &mut Ui, link: Link) {
	ui.label(RichText::new(tr!("nearby-none")).color(ui.visuals().weak_text_color()));
	if link != Link::Connected {
		kit::help(ui, tr!("nearby-none-offline"));
	}
}

fn row(ui: &mut Ui, channel: &NearbyChannel, cx: &mut Cx) {
	ui.horizontal_top(|ui| {
		ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
			if !channel.ambiguous && icon_label(ui, Icon::Channels, tr!("action-join")).clicked() {
				if channel.password {
					cx.local.nearby_prompt = Some(channel.name.clone());
					cx.local.nearby_password.clear();
				} else {
					join(channel, cx);
				}
			}
			ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
				ui.horizontal(|ui| {
					ui.label(RichText::new(&channel.name).strong());
					if channel.password {
						icon_inline(ui, Icon::Lock, ui.visuals().weak_text_color())
							.on_hover_text(tr!("nearby-needs-password"));
					}
				});
				let note = if channel.ambiguous {
					tr!("nearby-ambiguous").to_owned()
				} else {
					tr!("nearby-devices", count = channel.devices)
				};
				ui.label(RichText::new(note).weak());
			});
		});
	});
	if cx.local.nearby_prompt.as_deref() == Some(&channel.name) {
		prompt(ui, channel, cx);
	}
}

/// The password of a protected channel, asked in the row. A wrong password keeps it open: the
/// engine's refusal shows in the usual strip.
fn prompt(ui: &mut Ui, channel: &NearbyChannel, cx: &mut Cx) {
	let local = &mut *cx.local;
	let field = ui.add(
		egui::TextEdit::singleline(&mut local.nearby_password)
			.desired_width(field_width(ui))
			.password(true)
			.hint_text(tr!("password-hint")),
	);
	if !local.nearby_focused {
		field.request_focus();
		local.nearby_focused = true;
	}
	let entered = field.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
	let mut submit = entered;
	// Esc and the system Back cancel through `RelayApp::step_out`.
	let mut cancel = false;
	button_row(ui, |ui| {
		let ready = !local.nearby_password.is_empty();
		submit |= icon_label_enabled(ui, ready, Icon::Channels, tr!("action-join")).clicked();
		cancel |= ui.button(tr!("action-cancel")).clicked();
	});
	if cancel {
		local.nearby_prompt = None;
		local.nearby_focused = false;
		local.nearby_password.clear();
	} else if submit && !local.nearby_password.is_empty() {
		let (name, password) = (channel.name.clone(), mem::take(&mut local.nearby_password));
		cx.actions
			.run(|c| async move { c.join_by_name(&name, &password).await });
		if entered {
			field.request_focus();
		}
	}
}

fn join(channel: &NearbyChannel, cx: &mut Cx) {
	let (name, again) = (channel.name.clone(), channel.name.clone());
	let failed = cx.local.join_failed.clone();
	cx.actions.run_then(
		|c| async move { c.join_by_name(&name, "").await },
		move || {
			*failed.lock_unpoisoned() = Some(again);
		},
	);
}
