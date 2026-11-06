//! "Found nearby": devices on the network that the user has not trusted yet.
//! Trusting one is a two-step action that tells the user to compare the
//! fingerprint with the one the other device shows.

use egui::{RichText, Ui};

use crate::tr;
use crate::viewmodel::PeerRow;

use super::kit::{self, Cx};
use super::widgets::{avatar, button_row};
use crate::theme;

pub fn section(ui: &mut Ui, rows: &[&PeerRow], cx: &mut Cx) {
	kit::section_title(ui, tr!("people-found-title"));
	kit::help(ui, tr!("people-found-help"));
	for row in rows {
		card(ui, row, cx);
		kit::gap(ui);
	}
}

/// The key digest as groups of four, easy to read out and compare.
pub fn grouped(peer_id: &str) -> String {
	peer_id
		.to_uppercase()
		.as_bytes()
		.chunks(4)
		.map(|c| String::from_utf8_lossy(c).into_owned())
		.collect::<Vec<_>>()
		.join(" ")
}

fn card(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	kit::card(ui, |ui| {
		ui.horizontal_wrapped(|ui| {
			let tex = cx.textures.texture(&row.avatar, cx.actions);
			avatar(ui, &tex, theme::AVATAR_ROW);
			ui.vertical(|ui| {
				ui.label(RichText::new(&row.name).strong());
				ui.label(
					RichText::new(tr!(
						"found-subtitle",
						fingerprint = row.fingerprint.as_str()
					))
					.weak(),
				);
			});
		});
		trust_row(ui, row, cx);
		kit::details(ui, ("found-details", &row.peer_id), |ui| {
			ui.label(
				RichText::new(tr!(
					"peer-key",
					fingerprint = grouped(&row.peer_id).as_str()
				))
				.monospace(),
			);
			kit::help(ui, tr!("peer-key-help"));
		});
	});
}

/// Two-step trust: the first click explains, the second one acts.
fn trust_row(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	if cx.local.trust_confirm.as_deref() == Some(row.peer_id.as_str()) {
		kit::help(ui, &tr!("found-trust-ask", name = row.name.as_str()));
		ui.label(RichText::new(grouped(&row.peer_id)).monospace());
		button_row(ui, |ui| {
			if kit::accept_button(ui, tr!("found-trust-yes")).clicked() {
				let id = row.peer_id.clone();
				cx.actions.run(|c| async move { c.trust_peer(&id).await });
				cx.local.trust_confirm = None;
			}
			if ui.button(tr!("found-trust-cancel")).clicked() {
				cx.local.trust_confirm = None;
			}
		});
	} else if ui.button(tr!("found-trust")).clicked() {
		cx.local.trust_confirm = Some(row.peer_id.clone());
	}
}
