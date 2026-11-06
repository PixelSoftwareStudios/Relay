//! The Connect page: the devices found nearby (not trusted yet), then
//! "Connect a device": how others reach this device (the address, its QR
//! code, Scan on phones) and the field for another device's address. The
//! address section is open while there are no contacts and collapsed
//! afterwards, unless the user chose.

use egui::{self, RichText, Ui};
use relay_client::connection::{self, ConnectionStringProblem};

use crate::model::ConnectionView;
use crate::qr;
use crate::qr_scan::{ScanError, ScanNote};

use super::kit::{self, Cx};
use super::local::Local;
use super::nearby;
use super::textures::upload;
use super::visibility;
use super::widgets::field_width;
use super::widgets::{banner, button_row};
use crate::actions::Actions;
use crate::theme;
use crate::tr;
use crate::viewmodel::PeerRow;

/// Pixels per QR module.
const QR_SCALE: usize = 4;

/// Whether the section shows its body: the user's choice, else open exactly
/// when there are no contacts yet.
pub fn is_open(local: &Local, no_contacts: bool) -> bool {
	local.connect_open.unwrap_or(no_contacts)
}

pub fn show(ui: &mut Ui, rows: &[PeerRow], cx: &mut Cx) {
	kit::page_title(ui, tr!("nav-connect"));
	visibility::card(ui, cx);
	let found: Vec<&PeerRow> = rows.iter().filter(|r| !r.known).collect();
	nearby::section(ui, &found, cx);
	kit::gap(ui);
	card(ui, cx, !rows.iter().any(|r| r.known));
	kit::gap(ui);
	add_by_address(ui, cx);
}

/// The "Connect a device" card.
fn card(ui: &mut Ui, cx: &mut Cx, no_contacts: bool) {
	if !cx.local.connection_requested {
		cx.local.connection_requested = true;
		cx.actions.fetch_connection();
	}
	let open = is_open(cx.local, no_contacts);
	kit::card(ui, |ui| {
		let header = egui::CollapsingHeader::new(
			RichText::new(tr!("connect-title"))
				.size(theme::TEXT_HEADING)
				.strong(),
		)
		.id_salt("connect-section")
		.open(Some(open))
		.show(ui, |ui| body(ui, cx));
		if header.header_response.clicked() {
			cx.local.connect_open = Some(!open);
		}
		if !open {
			collapsed(ui, cx);
		}
	});
	if !open {
		cx.local.qr = None;
	}
}

/// The collapsed section: Copy address stays one click away.
fn collapsed(ui: &mut Ui, cx: &mut Cx) {
	if let Some(Ok(info)) = &cx.local.connection
		&& !info.connection_string.is_empty()
	{
		let text = info.connection_string.clone();
		button_row(ui, |ui| {
			if ui.button(tr!("connection-copy")).clicked() {
				cx.actions.copy_text(&text);
			}
		});
	}
}

fn body(ui: &mut Ui, cx: &mut Cx) {
	match cx.local.connection.clone() {
		None => kit::help(ui, tr!("asking-daemon")),
		Some(Err(message)) => {
			banner(ui, &message, true);
			kit::help(ui, tr!("connection-error-next"));
			if ui.button(tr!("action-try-again")).clicked() {
				forget(cx.local);
			}
		}
		Some(Ok(info)) => {
			show_info(ui, cx.actions, &info);
			if !info.connection_string.is_empty() {
				kit::gap(ui);
				qr_code(ui, cx.local, &info.connection_string);
			}
			scan(ui, cx);
			details(ui, &info);
		}
	}
}

/// The Scan button under the QR code, shown only when the shell has a camera
/// scanner, and what the last scan said. A scan only fills in the address
/// field of "Add a device by address"; it never trusts or calls anything.
fn scan(ui: &mut Ui, cx: &mut Cx) {
	if !cx.actions.shell.can_scan_qr() {
		return;
	}
	kit::gap(ui);
	let idle = !cx.local.scanning;
	button_row(ui, |ui| {
		if ui
			.add_enabled(idle, egui::Button::new(tr!("connection-scan")))
			.clicked()
		{
			cx.local.scanning = true;
			cx.local.scan_note = None;
			cx.actions.scan_qr();
		}
	});
	let Some(note) = cx.local.scan_note else {
		return;
	};
	if note.is_problem() {
		ui.label(RichText::new(note.message()).color(theme::WARN));
	} else {
		kit::help(ui, note.message());
	}
	if note == ScanNote::Failed(ScanError::CameraBlocked) {
		button_row(ui, |ui| {
			if ui.button(tr!("mic-open-settings")).clicked() {
				cx.actions.shell.open_camera_settings();
			}
		});
	}
}

/// Drop the cached string and QR texture (leaving the page, or retrying).
pub fn forget(local: &mut Local) {
	local.connection = None;
	local.connection_requested = false;
	local.qr = None;
}

fn show_info(ui: &mut Ui, actions: &Actions, info: &ConnectionView) {
	if info.connection_string.is_empty() {
		ui.label(tr!("connection-none"));
		return;
	}
	kit::help(ui, tr!("connection-share"));
	ui.add(egui::Label::new(RichText::new(&info.connection_string).monospace()).selectable(true));
	button_row(ui, |ui| {
		if ui.button(tr!("connection-copy")).clicked() {
			actions.copy_text(&info.connection_string);
		}
	});
	kit::help(ui, tr!("connection-warning"));
}

/// Addresses, port and fingerprint: only needed to check a connection by hand.
fn details(ui: &mut Ui, info: &ConnectionView) {
	kit::details(ui, "connection-details", |ui| {
		if !info.addresses.is_empty() {
			ui.label(tr!(
				"connection-addresses",
				addresses = info.addresses.join(", "),
				port = info.port.to_string()
			));
		}
		ui.add(
			egui::Label::new(
				RichText::new(tr!(
					"connection-fingerprint",
					fingerprint = info.fingerprint.as_str()
				))
				.small()
				.monospace(),
			)
			.selectable(true),
		);
	});
}

fn qr_code(ui: &mut Ui, local: &mut Local, text: &str) {
	if local.qr.as_ref().is_none_or(|(t, _)| t != text) {
		local.qr = qr::render_text(text, QR_SCALE)
			.map(|img| (text.to_owned(), upload(ui.ctx(), "connection-qr", &img)));
	}
	match &local.qr {
		Some((_, texture)) => {
			kit::help(ui, tr!("connection-qr-help"));
			ui.add(egui::Image::new(texture).fit_to_exact_size(texture.size_vec2()));
		}
		None => {
			ui.label(tr!("connection-qr-too-long"));
		}
	}
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
