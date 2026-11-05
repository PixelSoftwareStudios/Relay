//! egui front end: window glue only; logic lives in `viewmodel` and `format`.

mod sections;
mod widgets;

use std::sync::atomic::Ordering;

use eframe::egui;

use crate::shell::Actions;
use crate::tray::Tray;
use crate::viewmodel::{call_cards, peer_rows};

use sections::Local;

/// Decoded RGBA logo, square.
pub struct Logo {
	pub rgba: Vec<u8>,
	pub size: u32,
}

/// Decode the embedded PNG; falls back to a 1x1 transparent pixel.
pub fn decode_logo(png: &[u8]) -> Logo {
	match image::load_from_memory_with_format(png, image::ImageFormat::Png) {
		Ok(img) => {
			let img = img.into_rgba8();
			Logo {
				size: img.width(),
				rgba: img.into_raw(),
			}
		}
		Err(_) => Logo {
			rgba: vec![0; 4],
			size: 1,
		},
	}
}

pub struct RelayApp {
	actions: Actions,
	tray: Option<Tray>,
	local: Local,
	logo: egui::TextureHandle,
	/// Keeps the tokio runtime alive for the life of the window.
	_runtime: tokio::runtime::Runtime,
}

impl RelayApp {
	pub fn new(
		actions: Actions,
		tray: Option<Tray>,
		runtime: tokio::runtime::Runtime,
		logo: &Logo,
	) -> Self {
		let image = egui::ColorImage::from_rgba_unmultiplied([logo.size as usize; 2], &logo.rgba);
		let texture = actions
			.ctx
			.load_texture("relay-logo", image, egui::TextureOptions::LINEAR);
		Self {
			actions,
			tray,
			local: Local::default(),
			logo: texture,
			_runtime: runtime,
		}
	}

	/// Close hides to the tray when one exists; otherwise it quits this app.
	fn handle_close(&self, ctx: &egui::Context) {
		let wants_close = ctx.input(|i| i.viewport().close_requested());
		if wants_close
			&& self.tray.is_some()
			&& !self.actions.shared.quitting.load(Ordering::Acquire)
		{
			ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
			self.actions.hide_window();
		}
	}
}

impl eframe::App for RelayApp {
	fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
		self.handle_close(ctx);
		let minimized = ctx.input(|i| i.viewport().minimized.unwrap_or(false));
		self.actions.hub.set_visible(!minimized);
		if let (Some(tray), Some(summary)) = (&self.tray, self.actions.shared.take_tray()) {
			tray.apply(&summary);
		}
	}

	fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
		let state = self.actions.shared.state();
		let meters = self.actions.shared.meters();
		let cards = call_cards(&state, &meters);
		let rows = peer_rows(&state);
		let error = self.actions.shared.error();
		let has_tray = self.tray.is_some();

		egui::CentralPanel::default().show(ui, |ui| {
			egui::ScrollArea::vertical().show(ui, |ui| {
				ui.horizontal(|ui| {
					ui.image((self.logo.id(), egui::vec2(32.0, 32.0)));
					ui.heading("Relay");
					if let Some(info) = &state.engine {
						ui.label(egui::RichText::new(format!("engine {}", info.version)).weak());
					}
				});
				sections::status(ui, &state, error.as_deref());
				sections::calls(ui, &cards, &self.actions, &mut self.local);
				sections::peers(ui, &rows, &self.actions);
				sections::audio(ui, &state, &meters, &self.actions, &mut self.local);
				sections::profile(ui, &state, &self.actions, &mut self.local);
				sections::app_panel(ui, &self.actions, has_tray);
			});
		});
	}
}
