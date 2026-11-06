//! Nearby: devices on the network that the user has not trusted yet. Each
//! card shows the local address next to the name. Trusting one is a two-step
//! action that tells the user to compare the fingerprint with the one the
//! other device shows. A trusted device lives on the Contacts page.

use egui::{RichText, Ui};

use crate::icons::{Icon, icon_button, icon_label, icon_label_enabled, icon_label_primary};
use crate::layout::Page;
use crate::tr;
use crate::viewmodel::PeerRow;

use super::kit::{self, Cx};
use super::widgets::{avatar, button_row};
use crate::theme;

pub fn section(ui: &mut Ui, rows: &[&PeerRow], cx: &mut Cx) {
	kit::section_title(ui, tr!("home-nearby"));
	if rows.is_empty() {
		kit::empty_state(
			ui,
			tr!("people-none-nearby"),
			tr!("people-none-nearby-help"),
		);
		return;
	}
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
				if !row.address.is_empty() {
					ui.label(RichText::new(&row.address).weak().small().monospace());
				}
			});
		});
		trust_row(ui, row, cx);
		super::contacts::block_ask(ui, cx, row);
		kit::details(ui, ("found-details", &row.peer_id), |ui| {
			ui.label(
				RichText::new(tr!(
					"peer-key",
					fingerprint = grouped(&row.peer_id).as_str()
				))
				.monospace(),
			);
			kit::help(ui, tr!("peer-key-help"));
			if !row.addresses.is_empty() {
				ui.label(tr!("peer-addresses", addresses = row.addresses.join(", ")));
			}
			if row.port > 0 {
				ui.label(tr!("peer-port", port = row.port));
			}
		});
	});
}

/// A nearby device can be called at once; answering makes it a contact (unverified). The
/// careful option is to compare its fingerprint first and trust it. Two-step trust: the
/// first click explains, the second one acts.
fn trust_row(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	if cx.local.trust_confirm.as_deref() == Some(row.peer_id.as_str()) {
		kit::help(ui, &tr!("found-trust-ask", name = row.name.as_str()));
		ui.label(RichText::new(grouped(&row.peer_id)).monospace());
		button_row(ui, |ui| {
			if icon_label_primary(ui, Icon::Check, tr!("found-trust-yes")).clicked() {
				let id = row.peer_id.clone();
				cx.actions.run(|c| async move { c.trust_peer(&id).await });
				cx.local.trust_confirm = None;
			}
			if ui.button(tr!("found-trust-cancel")).clicked() {
				cx.local.trust_confirm = None;
			}
		});
	} else {
		button_row(ui, |ui| {
			if icon_label_enabled(ui, row.can_call, Icon::Call, tr!("peer-call")).clicked() {
				let id = row.peer_id.clone();
				cx.local.goto = Some(Page::Calls);
				cx.actions.run(|c| async move { c.start_call(&id).await });
			}
			if icon_label(ui, Icon::Trust, tr!("found-trust")).clicked() {
				cx.local.trust_confirm = Some(row.peer_id.clone());
			}
			let more = icon_button(ui, Icon::More, tr!("contact-more"));
			egui::Popup::menu(&more).show(|ui| super::contacts::block_entry(ui, cx, row));
		});
		kit::help(ui, tr!("found-call-help"));
	}
}
