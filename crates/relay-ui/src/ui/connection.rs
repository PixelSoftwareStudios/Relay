//! Connection panel: how others reach this device (string, addresses, QR) and
//! a field to call a pasted `relay://` string.

use egui::{self, RichText, Ui};
use relay_client::connection::{self, ConnectionStringProblem};

use crate::actions::Actions;
use crate::qr;

use super::local::Local;
use super::textures::upload;
use super::widgets::banner;
use crate::tr;

/// Pixels per QR module.
const QR_SCALE: usize = 4;

pub fn connection(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	let header = egui::CollapsingHeader::new(RichText::new(tr!("connection-heading")).heading())
		.id_salt("connection")
		.default_open(false)
		.show(ui, |ui| body(ui, actions, local));
	if header.body_returned.is_none() {
		// Closed: drop the QR texture and the cached string so a reopen is fresh.
		local.connection = None;
		local.connection_requested = false;
		local.qr = None;
	}
}

fn body(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	if !local.connection_requested {
		local.connection_requested = true;
		actions.fetch_connection();
	}
	match local.connection.clone() {
		None => {
			ui.label(RichText::new(tr!("asking-daemon")).weak());
		}
		Some(Err(message)) => banner(ui, &message, true),
		Some(Ok(info)) => {
			show_info(ui, actions, &info);
			qr_code(ui, local, &info.connection_string);
		}
	}
	ui.separator();
	paste_field(ui, actions, local);
}

fn show_info(ui: &mut Ui, actions: &Actions, info: &crate::model::ConnectionView) {
	if info.connection_string.is_empty() {
		ui.label(tr!("connection-none"));
		return;
	}
	ui.label(tr!("connection-share"));
	ui.horizontal(|ui| {
		ui.add(
			egui::Label::new(RichText::new(&info.connection_string).monospace()).selectable(true),
		);
		if ui.button(tr!("action-copy")).clicked() {
			actions.copy_text(&info.connection_string);
		}
	});
	if !info.addresses.is_empty() {
		ui.label(tr!(
			"connection-addresses",
			addresses = info.addresses.join(", "),
			port = info.port.to_string()
		));
	}
	ui.label(
		RichText::new(tr!(
			"connection-fingerprint",
			fingerprint = info.fingerprint.as_str()
		))
		.small()
		.weak(),
	);
	ui.label(RichText::new(tr!("connection-warning")).weak());
}

fn qr_code(ui: &mut Ui, local: &mut Local, text: &str) {
	if text.is_empty() {
		return;
	}
	if local.qr.as_ref().is_none_or(|(t, _)| t != text) {
		local.qr = qr::render_text(text, QR_SCALE)
			.map(|img| (text.to_owned(), upload(ui.ctx(), "connection-qr", &img)));
	}
	match &local.qr {
		Some((_, texture)) => {
			ui.add(egui::Image::new(texture).fit_to_exact_size(texture.size_vec2()));
		}
		None => {
			ui.label(tr!("connection-qr-too-long"));
		}
	}
}

fn paste_field(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	ui.label(tr!("connection-call"));
	let problem = (!local.paste.trim().is_empty())
		.then(|| connection::check(&local.paste).err())
		.flatten();
	ui.horizontal(|ui| {
		ui.add(
			egui::TextEdit::singleline(&mut local.paste).hint_text("relay://192.168.1.20:7000#…"),
		);
		let ready = !local.paste.trim().is_empty() && problem.is_none();
		if ui
			.add_enabled(ready, egui::Button::new(tr!("peer-call")))
			.clicked()
		{
			let text = local.paste.clone();
			actions.run(|c| async move { c.start_call_to(&text).await });
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
			.color(egui::Color32::from_rgb(210, 153, 34)),
		);
	}
}
