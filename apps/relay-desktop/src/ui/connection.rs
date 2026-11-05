//! Connection panel: how others reach this device (string, addresses, QR) and
//! a field to call a pasted `relay://` string.

use eframe::egui::{self, RichText, Ui};
use relay_client::connection;

use crate::qr;
use crate::shell::Actions;

use super::local::Local;
use super::textures::upload;
use super::widgets::banner;

/// Pixels per QR module.
const QR_SCALE: usize = 4;

pub fn connection(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	let header = egui::CollapsingHeader::new(RichText::new("Connection").heading())
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
			ui.label(RichText::new("Asking the daemon…").weak());
		}
		Some(Err(message)) => banner(ui, &message, true),
		Some(Ok(info)) => {
			show_info(ui, &info);
			qr_code(ui, local, &info.connection_string);
		}
	}
	ui.separator();
	paste_field(ui, actions, local);
}

fn show_info(ui: &mut Ui, info: &crate::model::ConnectionView) {
	if info.connection_string.is_empty() {
		ui.label("The daemon did not report a connection string.");
		return;
	}
	ui.label("Share this string to be called from another computer on your network:");
	ui.horizontal(|ui| {
		ui.add(
			egui::Label::new(RichText::new(&info.connection_string).monospace()).selectable(true),
		);
		if ui.button("Copy").clicked() {
			ui.ctx().copy_text(info.connection_string.clone());
		}
	});
	if !info.addresses.is_empty() {
		ui.label(format!(
			"Addresses: {} · port {}",
			info.addresses.join(", "),
			info.port
		));
	}
	ui.label(
		RichText::new(format!("Fingerprint {}", info.fingerprint))
			.small()
			.weak(),
	);
	ui.label(
		RichText::new(
			"Anyone with this string can ring you; it grants no trust and no microphone access.",
		)
		.weak(),
	);
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
			ui.label("The connection string is too long for a QR code.");
		}
	}
}

fn paste_field(ui: &mut Ui, actions: &Actions, local: &mut Local) {
	ui.label("Call a connection string");
	let problem = (!local.paste.trim().is_empty())
		.then(|| connection::check(&local.paste).err())
		.flatten();
	ui.horizontal(|ui| {
		ui.add(
			egui::TextEdit::singleline(&mut local.paste).hint_text("relay://192.168.1.20:7000#…"),
		);
		let ready = !local.paste.trim().is_empty() && problem.is_none();
		if ui.add_enabled(ready, egui::Button::new("Call")).clicked() {
			let text = local.paste.clone();
			actions.run(|c| async move { c.start_call_to(&text).await });
			local.paste.clear();
		}
	});
	if let Some(p) = problem {
		ui.label(RichText::new(p.message()).color(egui::Color32::from_rgb(210, 153, 34)));
	}
}
