//! The People page: add someone by address, the devices nearby and the other
//! devices this one knows. Keys and trust sit behind "Details".

use egui::{self, RichText, Ui};
use relay_client::connection::{self, ConnectionStringProblem};

use crate::theme;
use crate::viewmodel::PeerRow;

use super::kit::{self, Cx};
use super::widgets::{avatar, button_row, field_width};
use super::{connect, found, visibility};
use crate::tr;

pub fn show(ui: &mut Ui, rows: &[PeerRow], cx: &mut Cx) {
	kit::page_title(ui, tr!("nav-people"));
	visibility::card(ui, cx);
	kit::gap(ui);
	connect::show(ui, cx, rows.is_empty());
	kit::gap(ui);
	add_by_address(ui, cx);
	let (near, rest): (Vec<_>, Vec<_>) = rows.iter().partition(|r| r.reachable);
	let (far, met): (Vec<_>, Vec<_>) = rest.into_iter().partition(|r| r.known);
	kit::section_title(ui, tr!("home-nearby"));
	if near.is_empty() && met.is_empty() {
		kit::help(ui, tr!("people-none-nearby"));
		kit::help(ui, tr!("people-none-nearby-help"));
	}
	for row in near {
		self::row(ui, row, cx, true);
		kit::gap(ui);
	}
	if !met.is_empty() {
		found::section(ui, &met, cx);
	}
	if !far.is_empty() {
		kit::section_title(ui, tr!("people-known"));
		kit::help(ui, tr!("people-known-help"));
		for row in far {
			self::row(ui, row, cx, true);
			kit::gap(ui);
		}
	}
}

/// One person: picture, name, state and a Call button, with the technical
/// detail behind an expander when `with_details`.
pub fn row(ui: &mut Ui, row: &PeerRow, cx: &mut Cx, with_details: bool) {
	kit::card(ui, |ui| {
		ui.horizontal_wrapped(|ui| {
			let tex = cx.textures.texture(&row.avatar, cx.actions);
			avatar(ui, &tex, theme::AVATAR_ROW);
			ui.vertical(|ui| {
				ui.label(RichText::new(&row.name).strong());
				ui.label(RichText::new(&row.subtitle).weak());
			});
		});
		let call = ui.add_enabled(row.can_call, egui::Button::new(tr!("peer-call")));
		if call.clicked() {
			let id = row.peer_id.clone();
			cx.actions.run(|c| async move { c.start_call(&id).await });
		}
		if !row.can_call && row.reachable {
			kit::help(ui, tr!("peer-call-blocked"));
		}
		if with_details {
			kit::details(ui, ("peer-details", &row.peer_id), |ui| {
				peer_details(ui, row, cx);
			});
		}
	});
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
	if !row.badges.is_empty() {
		ui.label(tr!("peer-shared-channels", names = row.badges.join(", ")));
	}
	for (channel_id, channel_name) in &row.revocable {
		revoke_row(ui, cx, row, channel_id, channel_name);
	}
	if row.known {
		forget_row(ui, cx, row);
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

/// Two-step "forget this device": removes the stored trust decision.
fn forget_row(ui: &mut Ui, cx: &mut Cx, row: &PeerRow) {
	button_row(ui, |ui| {
		if cx.local.forget_confirm.as_deref() == Some(row.peer_id.as_str()) {
			ui.label(tr!("forget-ask", name = row.name.as_str()));
			if kit::danger_button(ui, tr!("forget-yes")).clicked() {
				let id = row.peer_id.clone();
				cx.actions.run(|c| async move { c.forget_peer(&id).await });
				cx.local.forget_confirm = None;
			}
			if ui.button(tr!("revoke-keep")).clicked() {
				cx.local.forget_confirm = None;
			}
		} else if ui.button(tr!("forget-device")).clicked() {
			cx.local.forget_confirm = Some(row.peer_id.clone());
		}
	});
}

/// A trusted device in Settings: name, how it is trusted, and Forget.
pub fn trusted_row(ui: &mut Ui, row: &PeerRow, cx: &mut Cx) {
	ui.label(RichText::new(&row.name).strong());
	ui.label(RichText::new(&row.subtitle).weak());
	forget_row(ui, cx, row);
}

/// Call a device that is not found automatically, by its connection string.
fn add_by_address(ui: &mut Ui, cx: &mut Cx) {
	kit::card(ui, |ui| {
		kit::card_title(ui, tr!("people-add-title"));
		kit::help(ui, tr!("people-add-help"));
		let local = &mut *cx.local;
		let problem = (!local.paste.trim().is_empty())
			.then(|| connection::check(&local.paste).err())
			.flatten();
		ui.horizontal_wrapped(|ui| {
			ui.add(
				egui::TextEdit::singleline(&mut local.paste)
					.desired_width(field_width(ui))
					.hint_text("relay://192.168.1.20:7000#…"),
			);
			let ready = !local.paste.trim().is_empty() && problem.is_none();
			if ui
				.add_enabled(ready, egui::Button::new(tr!("peer-call")))
				.clicked()
			{
				let text = local.paste.clone();
				cx.actions
					.run(|c| async move { c.start_call_to(&text).await });
				local.paste.clear();
			}
		});
		if let Some(p) = problem {
			ui.label(
				RichText::new(match p {
					ConnectionStringProblem::Scheme => tr!("conn-scheme"),
					ConnectionStringProblem::MissingFingerprint => tr!("conn-missing-fingerprint"),
					ConnectionStringProblem::BadFingerprint => tr!("conn-bad-fingerprint"),
					ConnectionStringProblem::BadAddress => tr!("conn-bad-address"),
				})
				.color(theme::WARN),
			);
		}
	});
}
