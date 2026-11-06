//! Connection: how others reach this device. The connection string and QR
//! code are what another person scans or pastes into "Add by address".

use egui::{self, RichText, Ui};

use crate::model::ConnectionView;
use crate::qr;

use super::super::kit::{self, Cx};
use super::super::local::Local;
use super::super::textures::upload;
use super::super::widgets::{banner, button_row};
use crate::actions::Actions;
use crate::tr;

/// Pixels per QR module.
const QR_SCALE: usize = 4;

pub fn show(ui: &mut Ui, cx: &mut Cx) {
	if !cx.local.connection_requested {
		cx.local.connection_requested = true;
		cx.actions.fetch_connection();
	}
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
			kit::card(ui, |ui| {
				show_info(ui, cx.actions, &info);
				details(ui, &info);
			});
			if !info.connection_string.is_empty() {
				kit::gap(ui);
				kit::card(ui, |ui| qr_code(ui, cx.local, &info.connection_string));
			}
		}
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
