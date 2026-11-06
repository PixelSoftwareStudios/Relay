//! A contact whose security key changed: a calm card with the three ways forward. Scan the
//! other device's QR code (the engine then repairs the contact), compare its fingerprint by
//! hand, or remove the contact. Nothing is trusted from here without the full fingerprint
//! of the new key, typed or scanned from the other device.

use egui::{self, RichText, Ui};

use crate::fingerprint;
use crate::icons::{Icon, icon_label, icon_label_enabled, icon_label_primary, icon_label_red};
use crate::theme;
use crate::tr;
use crate::viewmodel::PeerRow;

use super::contacts::{favorite_toggle, peer_details};
use super::kit::{self, Cx};
use super::widgets::{avatar, button_row, field_width};

/// The name as the title says it: a system host name loses its ".local".
fn short_name(name: &str) -> &str {
	let trimmed = name.trim();
	trimmed
		.strip_suffix(".local")
		.filter(|rest| !rest.is_empty())
		.unwrap_or(trimmed)
}

/// Whether `typed` is the full fingerprint of the new key of `row`.
pub fn matches(row: &PeerRow, typed: &str) -> bool {
	fingerprint::matches(&row.peer_id, typed)
}

pub fn card(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	let id = row.peer_id.as_str();
	let card = ui.scope(|ui| {
		kit::card(ui, |ui| {
			ui.horizontal_wrapped(|ui| {
				let tex = cx.textures.texture(&row.avatar, cx.actions);
				avatar(ui, &tex, theme::AVATAR_ROW);
				ui.vertical(|ui| {
					ui.label(
						RichText::new(tr!("keychange-title", name = short_name(&row.name)))
							.size(theme::TEXT_HEADING)
							.strong(),
					);
				});
			});
			kit::help(ui, tr!("keychange-body"));
			if cx.local.verify_key.as_deref() == Some(id) {
				compare(ui, row, cx);
			} else if cx.local.forget_confirm.as_deref() == Some(id) {
				remove_ask(ui, row, cx);
			} else {
				choices(ui, row, cx);
			}
			if cx.actions.shell.can_scan_qr() {
				super::connect::scan_status(ui, cx);
			}
			kit::details(ui, ("peer-details", id), |ui| {
				ui.label(
					RichText::new(tr!(
						"keychange-previous",
						fingerprint = row.previous_fingerprint.as_str()
					))
					.monospace(),
				);
				peer_details(ui, row, cx);
			});
		});
	});
	if cx.local.focus_contact.as_deref() == Some(id) {
		ui.scroll_to_rect(card.response.rect, Some(egui::Align::Center));
		cx.local.focus_contact = None;
	}
}

/// Scan, compare, remove; the star stays, Call is gone while the contact is blocked.
fn choices(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	let can_scan = cx.actions.shell.can_scan_qr();
	if can_scan {
		let idle =
			!cx.local.scanning && cx.local.scan_trust != Some(crate::qr_scan::ScanTrust::Working);
		button_row(ui, |ui| {
			if icon_label_primary_enabled(ui, idle, Icon::Scan, tr!("keychange-scan")) {
				cx.local.scanning = true;
				cx.local.scan_note = None;
				cx.local.scan_trust = None;
				cx.actions.scan_qr();
			}
		});
	}
	button_row(ui, |ui| {
		if icon_label(ui, Icon::Trust, tr!("keychange-compare")).clicked() {
			cx.local.verify_key = Some(row.peer_id.clone());
			cx.local.verify_text.clear();
		}
	});
	ui.horizontal_wrapped(|ui| {
		favorite_toggle(ui, row, cx);
		if icon_label_red(ui, Icon::Trash, tr!("keychange-remove")).clicked() {
			cx.local.forget_confirm = Some(row.peer_id.clone());
		}
	});
}

/// The primary button, disabled while a scan is open or its answer is awaited.
fn icon_label_primary_enabled(ui: &mut Ui, enabled: bool, icon: Icon, text: &str) -> bool {
	if enabled {
		icon_label_primary(ui, icon, text).clicked()
	} else {
		icon_label_enabled(ui, false, icon, text).clicked()
	}
}

/// The confirm step: the new key in groups, and a field for what the other device shows.
fn compare(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	kit::help(
		ui,
		&tr!("keychange-compare-intro", name = short_name(&row.name)),
	);
	kit::card_title(ui, tr!("keychange-new-key"));
	ui.label(RichText::new(&row.fingerprint).monospace());
	for line in fingerprint::lines(&row.peer_id) {
		ui.label(RichText::new(line).monospace());
	}
	ui.add(
		egui::TextEdit::singleline(&mut cx.local.verify_text)
			.desired_width(field_width(ui))
			.hint_text(tr!("keychange-field-hint")),
	);
	let typed = !cx.local.verify_text.trim().is_empty();
	let same = matches(row, &cx.local.verify_text);
	if same {
		ui.label(RichText::new(tr!("keychange-match")).color(theme::LIVE));
	} else if typed {
		ui.label(RichText::new(tr!("keychange-no-match")).color(theme::WARN));
	}
	button_row(ui, |ui| {
		if icon_label_enabled(ui, same, Icon::Check, tr!("keychange-confirm")).clicked() {
			let (id, typed) = (row.peer_id.clone(), cx.local.verify_text.clone());
			cx.actions
				.run(|c| async move { c.replace_pinned_key(&id, &typed).await });
			cx.local.verify_key = None;
			cx.local.verify_text.clear();
		}
		if ui.button(tr!("keychange-cancel")).clicked() {
			cx.local.verify_key = None;
			cx.local.verify_text.clear();
		}
	});
}

fn remove_ask(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	ui.label(tr!("keychange-remove-ask", name = short_name(&row.name)));
	button_row(ui, |ui| {
		if kit::danger_button(ui, tr!("keychange-remove-yes")).clicked() {
			let id = row.peer_id.clone();
			cx.actions.run(|c| async move { c.forget_peer(&id).await });
			cx.local.forget_confirm = None;
		}
		if ui.button(tr!("revoke-keep")).clicked() {
			cx.local.forget_confirm = None;
		}
	});
}
