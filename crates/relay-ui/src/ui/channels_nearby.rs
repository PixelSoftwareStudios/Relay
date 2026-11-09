//! "Channels on your network": the listed channels other devices advertise, one calm card.
//!
//! A row joins by name. The engine does not say whether a channel wants a password (it answers
//! every refusal alike), so a refused join opens the join form with the name filled in.

use egui::{RichText, Ui};

use relay_io::LockExt;

use crate::icons::{Icon, icon_label};
use crate::model::{Link, NearbyChannel};
use crate::tr;

use super::kit::{self, Cx};

pub fn section(ui: &mut Ui, cx: &mut Cx) {
	kit::section_title(ui, tr!("nearby-heading"));
	kit::card(ui, |ui| {
		if cx.state.nearby_channels.is_empty() {
			empty(ui, cx.state.link);
			return;
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
				join(channel, cx);
			}
			ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
				ui.label(RichText::new(&channel.name).strong());
				let note = if channel.ambiguous {
					tr!("nearby-ambiguous").to_owned()
				} else {
					tr!("nearby-devices", count = channel.devices)
				};
				ui.label(RichText::new(note).weak());
			});
		});
	});
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
