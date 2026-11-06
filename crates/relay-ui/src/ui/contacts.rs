//! The Contacts page: devices that have been trusted. Each card shows the
//! name, whether it is online (with its local address then) and how it is
//! trusted, with Call and a menu holding "Forget device". Keys and shared
//! channels sit behind "Details". Devices that are only found on the network
//! are not here: they wait under Connect until the user trusts them.

use egui::{self, RichText, Ui};

use crate::layout::Page;
use crate::theme;
use crate::viewmodel::PeerRow;

use super::kit::{self, Cx};
use super::widgets::{avatar, button_row};
use crate::tr;

pub fn show(ui: &mut Ui, rows: &[PeerRow], cx: &mut Cx) {
	kit::page_title(ui, tr!("nav-contacts"));
	let mut contacts: Vec<&PeerRow> = rows.iter().filter(|r| r.known).collect();
	if contacts.is_empty() {
		kit::card(ui, |ui| {
			ui.label(RichText::new(tr!("contacts-empty-title")).size(theme::TEXT_HEADING));
			kit::help(ui, tr!("contacts-empty-help"));
			if ui.button(tr!("contacts-go-connect")).clicked() {
				cx.local.goto = Some(Page::Connect);
			}
		});
		return;
	}
	// Online devices first; the order within each group is the engine's.
	contacts.sort_by_key(|r| !r.reachable);
	for row in contacts {
		self::row(ui, row, cx, true);
		kit::gap(ui);
	}
}

/// One device: picture, name, state and a Call button. `full` adds the menu
/// with Forget and the technical detail behind an expander.
pub fn row(ui: &mut Ui, row: &PeerRow, cx: &mut Cx, full: bool) {
	let card = ui.scope(|ui| {
		kit::card(ui, |ui| {
			ui.horizontal_wrapped(|ui| {
				let tex = cx.textures.texture(&row.avatar, cx.actions);
				avatar(ui, &tex, theme::AVATAR_ROW);
				ui.vertical(|ui| {
					ui.label(RichText::new(&row.name).strong());
					ui.label(RichText::new(&row.subtitle).weak());
					if row.reachable && !row.address.is_empty() {
						ui.label(RichText::new(&row.address).weak().small().monospace());
					}
				});
			});
			button_row(ui, |ui| {
				let call = ui.add_enabled(row.can_call, egui::Button::new(tr!("peer-call")));
				if call.clicked() {
					let id = row.peer_id.clone();
					cx.actions.run(|c| async move { c.start_call(&id).await });
				}
				if full && row.known {
					ui.menu_button(tr!("contact-more"), |ui| {
						if ui.button(tr!("forget-device")).clicked() {
							cx.local.forget_confirm = Some(row.peer_id.clone());
							ui.close();
						}
					});
				}
			});
			if !row.can_call && row.reachable {
				kit::help(ui, tr!("peer-call-blocked"));
			}
			if full {
				forget_ask(ui, cx, row);
				kit::details(ui, ("peer-details", &row.peer_id), |ui| {
					peer_details(ui, row, cx);
				});
			}
		});
	});
	if cx.local.focus_contact.as_deref() == Some(row.peer_id.as_str()) {
		ui.scroll_to_rect(card.response.rect, Some(egui::Align::Center));
		cx.local.focus_contact = None;
	}
}

fn peer_details(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	if row.fingerprint.is_empty() {
		kit::help(ui, tr!("peer-no-key"));
	} else {
		ui.label(
			RichText::new(tr!("peer-key", fingerprint = row.fingerprint.as_str())).monospace(),
		);
		kit::help(ui, tr!("peer-key-help"));
	}
	if !row.addresses.is_empty() {
		ui.label(tr!("peer-addresses", addresses = row.addresses.join(", ")));
	}
	if !row.badges.is_empty() {
		ui.label(tr!("peer-shared-channels", names = row.badges.join(", ")));
	}
	for (channel_id, channel_name) in &row.revocable {
		revoke_row(ui, cx, row, channel_id, channel_name);
	}
}

/// Two-step revoke: the first click asks, the second one acts.
fn revoke_row(ui: &mut Ui, cx: &mut Cx, row: &PeerRow, channel_id: &str, channel_name: &str) {
	let key = (channel_id.to_owned(), row.peer_id.clone());
	button_row(ui, |ui| {
		if cx.local.revoke_confirm.as_ref() == Some(&key) {
			ui.label(tr!(
				"revoke-ask",
				name = row.name.as_str(),
				channel = channel_name
			));
			if kit::danger_button(ui, tr!("revoke-yes")).clicked() {
				let (c, p) = key.clone();
				cx.actions
					.run(|ctl| async move { ctl.revoke_member(&c, &p).await });
				cx.local.revoke_confirm = None;
			}
			if ui.button(tr!("revoke-keep")).clicked() {
				cx.local.revoke_confirm = None;
			}
		} else if ui
			.button(tr!("revoke-from", channel = channel_name))
			.clicked()
		{
			cx.local.revoke_confirm = Some(key);
		}
	});
}

/// The confirmation under a contact whose "Forget device" was chosen. The
/// first step is the menu entry, the second one acts.
fn forget_ask(ui: &mut Ui, cx: &mut Cx, row: &PeerRow) {
	if cx.local.forget_confirm.as_deref() != Some(row.peer_id.as_str()) {
		return;
	}
	ui.label(tr!("forget-ask", name = row.name.as_str()));
	button_row(ui, |ui| {
		if kit::danger_button(ui, tr!("forget-yes")).clicked() {
			let id = row.peer_id.clone();
			cx.actions.run(|c| async move { c.forget_peer(&id).await });
			cx.local.forget_confirm = None;
		}
		if ui.button(tr!("revoke-keep")).clicked() {
			cx.local.forget_confirm = None;
		}
	});
}

/// A trusted device in Settings: name, how it is trusted, and Forget.
pub fn trusted_row(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	ui.label(RichText::new(&row.name).strong());
	ui.label(RichText::new(&row.subtitle).weak());
	if cx.local.forget_confirm.as_deref() == Some(row.peer_id.as_str()) {
		forget_ask(ui, cx, row);
	} else if ui.button(tr!("forget-device")).clicked() {
		cx.local.forget_confirm = Some(row.peer_id.clone());
	}
}
