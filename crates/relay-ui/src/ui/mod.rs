//! egui front end: screen composition. Logic lives in `viewmodel` and
//! `format`; the platform shell calls [`RelayApp::begin_frame`] then
//! [`RelayApp::ui`] each frame and supplies services through `Shell`.

mod app_panel;
mod audio;
mod channels;
mod connection;
mod host_switches;
mod hotkey;
mod local;
pub mod messages;
mod nav;
mod profile;
mod ring;
mod sections;
mod textures;
mod widgets;

use crate::actions::Actions;
use crate::imaging::{Decoded, decode_for_texture};
use crate::layout::{LayoutClass, Page};
use crate::shell::Insets;
use crate::theme;
use crate::viewmodel::{call_cards, peer_rows};

use crate::tr;
use local::{Local, PickedImage};
use messages::MessagesUi;
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
	messages: MessagesUi,
	/// The Messages side panel of the regular layout is showing.
	messages_open: bool,
	/// Keyboard height and focused field the view was last scrolled for.
	scrolled_for: Option<(u32, egui::Id)>,
	/// The language the installed fonts were built for; `None` before the first frame.
	font_language: Option<crate::i18n::Language>,
}

impl RelayApp {
	pub fn new(actions: Actions, ctx: &egui::Context) -> Self {
		let logo = decode_for_texture(HEADER_LOGO_PNG).unwrap_or(Decoded {
			size: [1, 1],
			rgba: vec![0; 4],
		});
		let zone = actions.shell.time_zone();
		Self {
			textures: Textures::new(ctx),
			logo: upload(ctx, "relay-logo", &logo),
			actions,
			local: Local::default(),
			class: None,
			page: Page::default(),
			messages: MessagesUi::new(zone),
			messages_open: false,
			scrolled_for: None,
			font_language: None,
		}
	}

	/// The layout the last `begin_frame` chose, `None` before the first frame.
	pub fn layout_class(&self) -> Option<LayoutClass> {
		self.class
	}

	/// Show the Messages panel (the compact page or the side panel), for the
	/// session of `call_id` (hex) when given.
	pub fn show_messages(&mut self, call_id: Option<String>) {
		if let Some(call) = call_id {
			self.messages.focus_call(call);
		}
		if self.class.is_some_and(LayoutClass::is_compact) {
			self.page = Page::Messages;
		} else {
			self.messages_open = true;
		}
	}

	/// Start the "send clipboard to a call" flow of the Messages panel: it
	/// reads the clipboard, then asks which call and for confirmation.
	pub fn begin_clipboard_send(&mut self) {
		self.messages.begin_clipboard();
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
		self.sync_language(ctx);
		let class = LayoutClass::for_width(ctx.content_rect().width());
		if self.class != Some(class) {
			theme::install(ctx, class);
			self.class = Some(class);
			ctx.request_repaint();
		}
		self.drain_inbox(ctx);
	}

	/// Follow the shell's language: switch the catalogs and reinstall the
	/// fonts (their order depends on the language) the frame it changes, and on
	/// the first frame.
	fn sync_language(&mut self, ctx: &egui::Context) {
		if let Some(options) = self.actions.shell.host_options() {
			let wanted = options.language();
			if wanted != crate::i18n::active() {
				crate::i18n::set_active(wanted);
			}
		}
		let active = crate::i18n::active();
		if self.font_language != Some(active) {
			crate::fonts::install(ctx, active);
			self.font_language = Some(active);
			ctx.request_repaint();
		}
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
		if let Some(result) = inbox.take_hotkey_loaded() {
			self.local.hotkey.loaded(result);
		}
		if let Some(result) = inbox.take_hotkey_saved() {
			self.local.hotkey.saved(result);
		}
		if let Some(invite) = inbox.take_invite() {
			self.local.invite = Some(invite);
		}
	}

	/// Draw the whole screen into `ui` (the shell's root ui).
	pub fn ui(&mut self, ui: &mut egui::Ui) {
		let class = self.class.unwrap_or(LayoutClass::Regular);
		let insets = self.actions.shell.safe_area_insets();
		let keyboard = self.actions.shell.keyboard_height().max(0.0);
		inset_panels(ui, insets, keyboard);
		if class.is_compact() {
			if keyboard <= 0.0 {
				egui::Panel::bottom("relay-nav").show(ui, |ui| nav::bottom_bar(ui, &mut self.page));
			}
		} else if self.messages_open {
			egui::Panel::right("relay-messages")
				.resizable(true)
				.default_size(380.0)
				.size_range(300.0..=640.0)
				.show(ui, |ui| {
					ui.set_width(ui.available_width());
					if self.messages.show(ui, &self.actions, true) {
						self.messages_open = false;
					}
				});
		}
		egui::CentralPanel::default().show(ui, |ui| {
			if class.is_compact() && self.page == Page::Messages {
				self.messages.show(ui, &self.actions, false);
				return;
			}
			// Beside the side panel the narrow column's bar would flip on and
			// off forever (a repaint per frame); wheel and touch still scroll.
			let bar = if self.messages_open {
				egui::scroll_area::ScrollBarVisibility::AlwaysHidden
			} else {
				egui::scroll_area::ScrollBarVisibility::VisibleWhenNeeded
			};
			egui::ScrollArea::vertical()
				.scroll_bar_visibility(bar)
				.show(ui, |ui| {
					self.header(ui, class);
					self.pages(ui, class);
					self.keep_focus_visible(ui, keyboard);
				});
		});
		if let Some(call) = self.local.open_messages.take() {
			self.messages.focus_call(call);
			if class.is_compact() {
				self.page = Page::Messages;
			} else {
				self.messages_open = true;
			}
		}
	}

	/// While the soft keyboard is up, scroll the focused field into view once
	/// per keyboard height or focus change (never fighting a later manual scroll).
	fn keep_focus_visible(&mut self, ui: &mut egui::Ui, keyboard: f32) {
		let focused = if keyboard > 0.0 {
			ui.memory(|m| m.focused())
		} else {
			None
		};
		let key = focused.map(|id| (keyboard.to_bits(), id));
		if key == self.scrolled_for {
			return;
		}
		self.scrolled_for = key;
		if let Some(id) = focused
			&& let Some(response) = ui.ctx().read_response(id)
		{
			ui.scroll_to_rect(response.rect.expand(theme::SPACE_M), None);
		}
	}

	/// A held talk button that was not drawn this frame (page switched, call
	/// ended) counts as released, so the microphone is never left open.
	fn release_unseen_talk(&mut self) {
		if !self.local.talk_drawn
			&& let Some(call) = self.local.talking.take()
		{
			self.actions.shell.push_to_talk(&call, false);
		}
		self.local.talk_drawn = false;
	}

	fn header(&mut self, ui: &mut egui::Ui, class: LayoutClass) {
		let state = self.actions.shared.state();
		ui.horizontal(|ui| {
			ui.image((self.logo.id(), egui::vec2(32.0, 32.0)));
			ui.heading("Relay");
			if let Some(info) = &state.engine {
				ui.label(
					egui::RichText::new(tr!("header-engine", version = info.version.as_str()))
						.weak(),
				);
			}
			if !class.is_compact() {
				ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
					if ui
						.selectable_label(self.messages_open, tr!("nav-messages"))
						.clicked()
					{
						self.messages_open = !self.messages_open;
					}
				});
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
			if actions.shell.has_global_hotkey() {
				hotkey::hotkey(ui, actions, local);
			}
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
			audio::audio(ui, &state, &meters, actions, local);
		}
		if show(Page::Settings) {
			profile::profile(ui, &state, actions, local, textures);
			app_panel::app_panel(ui, &state, actions, local, has_tray);
		}
		self.release_unseen_talk();
	}
}

/// Empty panels that keep the content out of the system bars and the soft
/// keyboard. `keyboard` is the height from the screen bottom, so it covers the
/// bottom inset when shown.
fn inset_panels(ui: &mut egui::Ui, insets: Insets, keyboard: f32) {
	let spacer = |panel: egui::Panel, size: f32, ui: &mut egui::Ui| {
		if size > 0.0 {
			panel
				.resizable(false)
				.exact_size(size)
				.frame(egui::Frame::NONE)
				.show_separator_line(false)
				.show(ui, |_| {});
		}
	};
	spacer(egui::Panel::top("relay-inset-top"), insets.top, ui);
	spacer(egui::Panel::left("relay-inset-left"), insets.left, ui);
	spacer(egui::Panel::right("relay-inset-right"), insets.right, ui);
	let bottom = keyboard.max(insets.bottom);
	spacer(egui::Panel::bottom("relay-inset-bottom"), bottom, ui);
}
