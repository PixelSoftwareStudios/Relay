//! "Connect a device": how others reach this device. The address (connection
//! string), its QR code and the paste field for the other device's address
//! sit together at the top of the People page. The section is open while
//! nobody is known yet and collapsed afterwards, unless the user chose.

use egui::{self, RichText, Ui};

use crate::model::ConnectionView;
use crate::qr;
use crate::qr_scan::{ScanError, ScanNote};

use super::kit::{self, Cx};
use super::local::Local;
use super::textures::upload;
use super::widgets::{banner, button_row};
use crate::actions::Actions;
use crate::tr;

/// Pixels per QR module.
const QR_SCALE: usize = 4;

/// Whether the section shows its body: the user's choice, else open exactly
/// when no device is known yet.
pub fn is_open(local: &Local, no_peers: bool) -> bool {
	local.connect_open.unwrap_or(no_peers)
}

pub fn show(ui: &mut Ui, cx: &mut Cx, no_peers: bool) {
	if !cx.local.connection_requested {
		cx.local.connection_requested = true;
		cx.actions.fetch_connection();
	}
	let open = is_open(cx.local, no_peers);
	kit::card(ui, |ui| {
		let header = egui::CollapsingHeader::new(
			RichText::new(tr!("connect-title"))
				.size(crate::theme::TEXT_HEADING)
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
		ui.label(RichText::new(note.message()).color(crate::theme::WARN));
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
