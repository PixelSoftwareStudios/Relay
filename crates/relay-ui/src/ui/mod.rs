//! egui front end: screen composition. Logic lives in `viewmodel` and
//! `format`; the platform shell calls [`RelayApp::begin_frame`] then
//! [`RelayApp::ui`] each frame and supplies services through `Shell`.

mod channels;
mod connection;
mod local;
mod nav;
mod profile;
mod ring;
mod sections;
mod textures;
mod widgets;

use crate::actions::Actions;
use crate::imaging::{Decoded, decode_for_texture};
use crate::layout::{LayoutClass, Page};
use crate::theme;
use crate::viewmodel::{call_cards, peer_rows};

use local::{Local, PickedImage};
use textures::{Textures, upload};

/// Header logo, embedded from the repo-root assets folder.
const HEADER_LOGO_PNG: &[u8] = include_bytes!("../../../../assets/icon/relay-mark-64.png");

pub struct RelayApp {
	actions: Actions,
	local: Local,
	textures: Textures,
	logo: egui::TextureHandle,
	/// The layout the installed style was built for; `None` before the first frame.
	class: Option<LayoutClass>,
	page: Page,
}

impl RelayApp {
	pub fn new(actions: Actions, ctx: &egui::Context) -> Self {
		let logo = decode_for_texture(HEADER_LOGO_PNG).unwrap_or(Decoded {
			size: [1, 1],
			rgba: vec![0; 4],
		});
		Self {
			textures: Textures::new(ctx),
			logo: upload(ctx, "relay-logo", &logo),
			actions,
			local: Local::default(),
			class: None,
			page: Page::default(),
		}
	}

	/// The layout the last `begin_frame` chose, `None` before the first frame.
	pub fn layout_class(&self) -> Option<LayoutClass> {
		self.class
	}

	/// The page shown in the compact layout.
	pub fn page(&self) -> Page {
		self.page
	}

	pub fn set_page(&mut self, page: Page) {
		self.page = page;
	}

	/// Per-frame work before drawing: window-visible hint, style for the
	/// current width, and moving finished background results into UI state.
	pub fn begin_frame(&mut self, ctx: &egui::Context) {
		self.actions
			.hub
			.set_visible(self.actions.shell.window_visible());
		let class = LayoutClass::for_width(ctx.content_rect().width());
		if self.class != Some(class) {
			theme::install(ctx, class);
			self.class = Some(class);
			ctx.request_repaint();
		}
		self.drain_inbox(ctx);
	}

	/// Move finished background results into UI-owned state and textures.
	fn drain_inbox(&mut self, ctx: &egui::Context) {
		let inbox = &self.actions.shared.inbox;
		self.textures.drain(ctx, inbox);
		if let Some(result) = inbox.take_connection() {
			self.local.connection = Some(result);
		}
		if let Some(p) = inbox.take_picked() {
			let texture = upload(ctx, "picked-avatar", &p.preview);
			self.local.picked = Some(PickedImage {
				bytes: p.bytes,
				media_type: p.media_type,
				texture,
			});
		}
		if let Some(invite) = inbox.take_invite() {
			self.local.invite = Some(invite);
		}
	}

	/// Draw the whole screen into `ui` (the shell's root ui).
	pub fn ui(&mut self, ui: &mut egui::Ui) {
		let class = self.class.unwrap_or(LayoutClass::Regular);
		if class.is_compact() {
			nav::bottom_bar(ui, &mut self.page);
		}
		egui::CentralPanel::default().show(ui, |ui| {
			egui::ScrollArea::vertical().show(ui, |ui| {
				self.header(ui);
				self.pages(ui, class);
			});
		});
	}

	fn header(&self, ui: &mut egui::Ui) {
		let state = self.actions.shared.state();
		ui.horizontal(|ui| {
			ui.image((self.logo.id(), egui::vec2(32.0, 32.0)));
			ui.heading("Relay");
			if let Some(info) = &state.engine {
				ui.label(egui::RichText::new(format!("engine {}", info.version)).weak());
			}
		});
	}

	fn pages(&mut self, ui: &mut egui::Ui, class: LayoutClass) {
		let state = self.actions.shared.state();
		let meters = self.actions.shared.meters();
		let cards = call_cards(&state, &meters);
		let rows = peer_rows(&state);
		let error = self.actions.shared.error();
		let has_tray = self.actions.shell.hides_to_tray();
		let (actions, local, textures) = (&self.actions, &mut self.local, &mut self.textures);
		let show = |page: Page| !class.is_compact() || self.page == page;

		if show(Page::Calls) {
			sections::status(ui, &state, error.as_deref(), actions);
			sections::calls(ui, &cards, actions, local, textures);
		}
		if show(Page::Settings) {
			connection::connection(ui, actions, local);
		}
		if show(Page::People) {
			sections::peers(ui, &rows, actions, local, textures);
		}
		if show(Page::Channels) {
			channels::channels(ui, &state, actions, local);
		}
		if show(Page::Settings) {
			channels::privacy(ui, &state, actions);
		}
		if show(Page::Calls) {
			sections::audio(ui, &state, &meters, actions, local);
		}
		if show(Page::Settings) {
			profile::profile(ui, &state, actions, local, textures);
			sections::app_panel(ui, &state, actions, local, has_tray);
		}
	}
}
