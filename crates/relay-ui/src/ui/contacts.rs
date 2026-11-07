//! The Contacts page: devices that have been trusted. Each card shows the
//! name, whether it is online (with its local address then) and how it is
//! trusted, with Call and a menu holding "Forget device". Keys and shared
//! channels sit behind "Details". Devices that are only found on the network
//! are not here: they wait under Connect until the user trusts them.

use egui::{self, RichText, Ui};

use crate::icons::{Icon, icon_button, icon_label, icon_label_full, icon_label_red, icon_toggle};
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
			if icon_label(ui, Icon::Connect, tr!("contacts-go-connect")).clicked() {
				cx.local.goto = Some(Page::Connect);
			}
		});
		return;
	}
	// A device that needs a decision first, then online devices; the order within each
	// group is the engine's.
	contacts.sort_by_key(|r| (!r.key_changed, !r.reachable));
	for row in contacts {
		if row.key_changed {
			super::key_changed::card(ui, row, cx);
		} else {
			self::row(ui, row, cx, true);
		}
		kit::gap(ui);
	}
}

/// One device. Top row: picture, name and state, with the star and the menu at the top right
/// (View details first, then Block and Forget). Below: the details when opened from the menu,
/// then Call across the full width, with room above it. An offline contact is listed all the
/// same: Call is greyed and the line under it says why.
pub fn row(ui: &mut Ui, row: &PeerRow, cx: &mut Cx, full: bool) {
	let card = ui.scope(|ui| {
		kit::card(ui, |ui| {
			head(ui, row, cx, full);
			if full {
				forget_ask(ui, cx, row);
				block_ask(ui, cx, row);
				if cx.local.details_open.contains(&row.peer_id) {
					ui.add_space(theme::SPACE_S);
					peer_details(ui, row, cx);
				}
			}
			ui.add_space(theme::SPACE_XL);
			let call = icon_label_full(ui, row.can_call, Icon::Call, tr!("peer-call"));
			if call.clicked() {
				let id = row.peer_id.clone();
				cx.local.goto = Some(Page::Calls);
				cx.actions.run(|c| async move { c.start_call(&id).await });
			}
			if !row.can_call {
				let why = if row.reachable {
					tr!("peer-call-blocked")
				} else {
					tr!("peer-call-offline")
				};
				kit::help(ui, why);
			}
		});
	});
	if cx.local.focus_contact.as_deref() == Some(row.peer_id.as_str()) {
		ui.scroll_to_rect(card.response.rect, Some(egui::Align::Center));
		cx.local.focus_contact = None;
	}
}

/// Picture, name and state on the left; star and menu pinned to the top right corner.
fn head(ui: &mut Ui, row: &PeerRow, cx: &mut Cx, full: bool) {
	ui.horizontal_top(|ui| {
		let tex = cx.textures.texture(&row.avatar, cx.actions);
		avatar(ui, &tex, theme::AVATAR_ROW);
		ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
			if full && row.known {
				menu(ui, row, cx);
				favorite_toggle(ui, row, cx);
			}
			ui.with_layout(egui::Layout::top_down(egui::Align::Min), |ui| {
				ui.label(RichText::new(&row.name).strong());
				ui.label(RichText::new(&row.subtitle).weak());
				if row.reachable && !row.address.is_empty() {
					ui.label(RichText::new(&row.address).weak().small().monospace());
				} else if !row.reachable && row.last_seen_ms > 0 {
					ui.label(
						RichText::new(last_seen(cx, row.last_seen_ms))
							.weak()
							.small(),
					);
				}
			});
		});
	});
}

/// The kebab menu: View details (or Hide details) on top, then Block and Forget.
fn menu(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	let more = icon_button(ui, Icon::More, tr!("contact-more"));
	egui::Popup::menu(&more).show(|ui| {
		details_entry(ui, cx, &row.peer_id);
		if icon_label_red(ui, Icon::Trash, tr!("forget-device")).clicked() {
			cx.local.forget_confirm = Some(row.peer_id.clone());
			ui.close();
		}
		block_entry(ui, cx, row);
	});
}

/// The first menu entry of a card: opens or closes its details.
pub(super) fn details_entry(ui: &mut Ui, cx: &mut Cx, id: &str) {
	let open = cx.local.details_open.contains(id);
	let text = if open {
		tr!("action-hide-details")
	} else {
		tr!("action-view-details")
	};
	if icon_label(ui, Icon::Info, text).clicked() {
		if open {
			cx.local.details_open.remove(id);
		} else {
			cx.local.details_open.insert(id.to_owned());
		}
		ui.close();
	}
}

/// The star: filled when the device is a favorite. The tooltip names what a
/// press does. The engine refuses a seventh favorite; the error banner says so.
pub(super) fn favorite_toggle(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	let (icon, tip) = if row.favorite {
		(Icon::StarFilled, tr!("contact-favorite-remove"))
	} else {
		(Icon::Star, tr!("contact-favorite-add"))
	};
	if icon_toggle(ui, row.favorite, icon, tip).clicked() {
		let (id, on) = (row.peer_id.clone(), !row.favorite);
		cx.actions
			.run(|c| async move { c.set_favorite(&id, on).await });
	}
}

/// "Last seen 5 min ago" in the user's time zone.
fn last_seen(cx: &Cx, then_ms: u64) -> String {
	let zone = cx.actions.shell.time_zone();
	let when = crate::timefmt::format(&zone, cx.actions.shell.now_unix_ms(), then_ms);
	tr!("contact-last-seen", when = when.as_str())
}

pub(super) fn peer_details(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
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
	if row.port > 0 {
		ui.label(tr!("peer-port", port = row.port));
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

/// The menu entry "Block device": the first step, a confirmation follows under the card.
pub(super) fn block_entry(ui: &mut Ui, cx: &mut Cx, row: &PeerRow) {
	if icon_label_red(ui, Icon::Lock, tr!("block-device")).clicked() {
		cx.local.block_confirm = Some(row.peer_id.clone());
		ui.close();
	}
}

/// The confirmation under a device whose "Block device" was chosen.
pub(super) fn block_ask(ui: &mut Ui, cx: &mut Cx, row: &PeerRow) {
	if cx.local.block_confirm.as_deref() != Some(row.peer_id.as_str()) {
		return;
	}
	ui.label(tr!("block-ask", name = row.name.as_str()));
	button_row(ui, |ui| {
		if kit::danger_button(ui, tr!("block-yes")).clicked() {
			let id = row.peer_id.clone();
			cx.actions.run(|c| async move { c.block_peer(&id).await });
			cx.local.block_confirm = None;
		}
		if ui.button(tr!("revoke-keep")).clicked() {
			cx.local.block_confirm = None;
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
